//! One socket's RPC session as a state machine without I/O: the adapter feeds it each text message and polls it for live frames.
//! Scope checks come first, from the table in `t3-contracts`; an RPC this server does not build answers a declared failure.
use crate::domain::command::Family;
use crate::ports::AgentError;
use crate::usecases::engine::{thread_event_frame, Failure, Published, Subscription};
use crate::usecases::launch::LaunchFailure;
use crate::usecases::queries::QueryFailure;
use crate::usecases::service::{Principal, Service};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use t3_contracts::rpc::{self, FromClient, StreamPacer};
use t3_contracts::scopes::required_scopes;
use t3_contracts::wire;

/// Tells the thread following a worktree's setup to stop once the stream it feeds is gone.
struct StopSetup(Arc<AtomicBool>);

impl Drop for StopSetup {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

enum Kind {
    Shell,
    Thread(String),
    Idle,
    Followed { _stop: StopSetup },
}

struct Stream {
    id: Value,
    kind: Kind,
    floor: u64,
    pacer: StreamPacer,
    /// The follower said the stream is over: it ends when the pacer has sent and been acked everything.
    ending: bool,
}

pub struct Connection {
    svc: Service,
    principal: Principal,
    live: Subscription,
    streams: Vec<Stream>,
    fibers: u64,
    /// The snapshots the threads following worktree setups send, with the id of the stream each is for.
    setups: Receiver<(Value, Value)>,
    setup_tx: Sender<(Value, Value)>,
}

impl Connection {
    pub fn new(svc: Service, principal: Principal) -> Connection {
        let (tx, live) = channel();
        svc.engine().register(tx);
        let (setup_tx, setups) = channel();
        Connection { svc, principal, live, streams: Vec::new(), fibers: 0, setups, setup_tx }
    }

    /// The frames to send for one inbound text message.
    pub fn on_message(&mut self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        match rpc::parse_client_message(text) {
            Err(e) => out.push(rpc::protocol_error(&e.0)),
            Ok(frames) => {
                for frame in frames {
                    match frame {
                        FromClient::Ping => out.push(rpc::pong()),
                        FromClient::Ack { request_id } => {
                            if let Some(s) = self.streams.iter_mut().find(|s| s.id == request_id) {
                                s.pacer.ack();
                            }
                        }
                        FromClient::Interrupt { request_id } => {
                            if self.streams.iter().any(|s| s.id == request_id) {
                                self.streams.retain(|s| s.id != request_id);
                                self.fibers += 1;
                                out.push(rpc::exit_interrupted(&request_id, self.fibers));
                            }
                        }
                        FromClient::Eof => {}
                        FromClient::Request { id, tag, payload } => out.extend(self.request(id, &tag, &payload)),
                    }
                }
            }
        }
        out.extend(self.flush());
        out.iter().map(Value::to_string).collect()
    }

    /// Live events that arrived since the last call, as the chunks their streams may send now.
    pub fn poll(&mut self) -> Vec<String> {
        let pending: Vec<Arc<Published>> = self.live.try_iter().collect();
        for published in pending {
            let seq = published.event.sequence;
            for s in self.streams.iter_mut().filter(|s| seq > s.floor) {
                match &s.kind {
                    Kind::Shell => published.shell.iter().for_each(|f| s.pacer.push(f.clone())),
                    Kind::Thread(id) if published.event.thread_id.as_deref() == Some(id) => s.pacer.push(thread_event_frame(&published.event)),
                    _ => {}
                }
                s.floor = seq;
            }
        }
        let snapshots: Vec<(Value, Value)> = self.setups.try_iter().collect();
        for (id, snapshot) in snapshots {
            if let Some(stream) = self.streams.iter_mut().find(|s| s.id == id && matches!(s.kind, Kind::Followed { .. })) {
                if snapshot == rpc::end_marker() {
                    stream.ending = true;
                } else {
                    stream.pacer.push(snapshot);
                }
            }
        }
        self.flush().iter().map(Value::to_string).collect()
    }

    fn flush(&mut self) -> Vec<Value> {
        let mut out = Vec::new();
        for s in &mut self.streams {
            match s.pacer.next_chunk() {
                Some(values) => out.push(rpc::chunk(&s.id, values)),
                None if s.ending && s.pacer.drained() => {
                    out.push(rpc::exit_success(&s.id, None));
                    s.id = Value::Null;
                }
                None => {}
            }
        }
        self.streams.retain(|s| !s.id.is_null());
        out
    }

    fn open(&mut self, id: Value, kind: Kind, floor: u64, first: Vec<Value>) {
        let mut pacer = StreamPacer::default();
        first.into_iter().for_each(|v| pacer.push(v));
        self.streams.retain(|s| s.id != id);
        self.streams.push(Stream { id, kind, floor, pacer, ending: false });
    }

    fn request(&mut self, id: Value, tag: &str, payload: &Value) -> Vec<Value> {
        match required_scopes(tag) {
            None => {
                self.svc.log("rpc.unsupported", &[("tag", tag.to_string()), ("known", "false".into())]);
                return vec![rpc::exit_defect(&id, &format!("Unknown request tag: {tag}"))];
            }
            Some(needed) => {
                if let Some(missing) = needed.iter().find(|n| !self.principal.scopes.iter().any(|s| s == *n)) {
                    self.svc.log("rpc.denied", &[("tag", tag.to_string()), ("scope", missing.to_string()), ("tokenId", self.principal.token_id.clone())]);
                    return vec![rpc::exit_failure(&id, wire::authorization_error(missing))];
                }
            }
        }
        let after = payload.get("afterSequence").and_then(Value::as_u64);
        let marker = payload.get("requestCompletionMarker").and_then(Value::as_bool).unwrap_or(false);
        let synchronized = |mut frames: Vec<Value>| {
            if marker {
                frames.push(json!({"kind": "synchronized"}));
            }
            frames
        };
        let command = payload.get("command").filter(|c| c.is_object()).unwrap_or(payload);
        match tag {
            "server.probe" => vec![rpc::exit_success(&id, Some(json!({})))],
            "server.getConfig" => vec![rpc::exit_success(&id, Some(self.svc.server_config()))],
            "subscribeServerConfig" => {
                self.open(id, Kind::Idle, 0, vec![json!({"version": 1, "type": "snapshot", "config": self.svc.server_config()})]);
                vec![]
            }
            "subscribeServerLifecycle" => {
                let env = self.svc.env();
                let cwd = env.cwd.clone();
                let project_name = cwd.rsplit('/').find(|p| !p.is_empty()).unwrap_or("").to_string();
                let frames = vec![
                    json!({"version": 1, "sequence": 1, "type": "welcome", "payload": {"environment": self.svc.descriptor(), "cwd": cwd, "projectName": project_name}}),
                    json!({"version": 1, "sequence": 2, "type": "ready", "payload": {"at": t3_contracts::time::iso(self.svc.now_ms()), "environment": self.svc.descriptor()}}),
                ];
                self.open(id, Kind::Idle, 0, frames);
                vec![]
            }
            "orchestration.subscribeShell" => match self.svc.engine().shell_start(after) {
                Ok((frames, floor)) => {
                    self.open(id, Kind::Shell, floor, synchronized(frames));
                    vec![]
                }
                Err(m) => vec![rpc::exit_defect(&id, &m)],
            },
            "orchestration.subscribeThread" => {
                let Some(thread) = payload.get("threadId").and_then(Value::as_str) else {
                    return vec![rpc::exit_failure(&id, wire::not_found_error("thread", ""))];
                };
                match self.svc.engine().thread_start(thread, after) {
                    Ok((frames, floor)) => {
                        self.open(id, Kind::Thread(thread.to_string()), floor, synchronized(frames));
                        vec![]
                    }
                    Err(_) => vec![rpc::exit_failure(&id, wire::not_found_error("thread", thread))],
                }
            }
            "orchestration.getThreadProjection" => {
                let thread = payload.get("threadId").and_then(Value::as_str).unwrap_or("");
                match self.svc.engine().thread_projection(thread) {
                    Some(p) => vec![rpc::exit_success(&id, Some(p))],
                    None => vec![rpc::exit_failure(&id, wire::not_found_error("thread", thread))],
                }
            }
            "orchestration.dispatchCommand" | "projects.mutate" => {
                let project = tag == "projects.mutate";
                match self.svc.dispatch(if project { Family::Project } else { Family::Thread }, command) {
                    Ok(done) => vec![rpc::exit_success(&id, Some(done.row.unwrap_or_else(|| json!({"sequence": done.sequence}))))],
                    Err(Failure::Defect(message)) => {
                        self.svc.log("rpc.command.undecodable", &[("tag", tag.to_string())]);
                        vec![rpc::exit_defect(&id, &message)]
                    }
                    Err(Failure::Refused { command_id, command_type, message }) => {
                        self.svc.log("rpc.command.refused", &[("type", command_type.clone()), ("message", message.clone())]);
                        let error = if project { wire::project_mutation_error(&command_id, wire::PROJECT_MUTATION_FAILED) } else { wire::dispatch_error(&command_id, &command_type, &message) };
                        vec![rpc::exit_failure(&id, error)]
                    }
                    Err(Failure::ProjectNotEmpty { command_id, project_id }) => {
                        self.svc.log("rpc.command.refused", &[("type", "project.delete".into()), ("message", wire::project_not_empty(&project_id))]);
                        vec![rpc::exit_failure(&id, wire::project_mutation_error(&command_id, &wire::project_not_empty(&project_id)))]
                    }
                }
            }
            "orchestration.launchThread" => match self.svc.launch_thread(payload) {
                Ok(result) => vec![rpc::exit_success(&id, Some(result))],
                Err(LaunchFailure::Defect(message)) => {
                    self.svc.log("rpc.command.undecodable", &[("tag", tag.to_string())]);
                    vec![rpc::exit_defect(&id, &message)]
                }
                Err(LaunchFailure::Refused(error)) => vec![rpc::exit_failure(&id, error)],
            },
            "orchestration.getTurnItem" | "orchestration.searchThreads" | "orchestration.getThreadFamily" | "orchestration.getOlderSettledThreads" | "orchestration.forceStopThread" | "orchestration.getWorkflowScript" => {
                let answer = match tag {
                    "orchestration.getTurnItem" => self.svc.turn_item(payload),
                    "orchestration.searchThreads" => self.svc.search_threads(payload),
                    "orchestration.getThreadFamily" => self.svc.thread_family(payload),
                    "orchestration.getOlderSettledThreads" => self.svc.older_settled(payload),
                    "orchestration.forceStopThread" => self.svc.force_stop(payload),
                    _ => self.svc.workflow_script(payload),
                };
                vec![self.answered(&id, tag, answer.map(Some))]
            }
            "orchestration.getArchivedShellSnapshot" => vec![rpc::exit_success(&id, Some(self.svc.archived_shell()))],
            "orchestration.stopWorkflow" => vec![self.answered(&id, tag, self.svc.stop_workflow(payload).map(|()| None))],
            "server.reportClientActivity" => vec![self.answered(&id, tag, self.svc.report_client_activity(payload).map(|()| None))],
            "peerLinks.list" => vec![rpc::exit_success(&id, Some(self.svc.peer_links_list()))],
            "peerLinks.unlink" => vec![self.answered(&id, tag, self.svc.peer_links_unlink(payload).map(Some))],
            "peerLinks.link" => vec![self.answered(&id, tag, Err(self.svc.peer_links_link(payload)))],
            "peerLinks.answerRequest" => vec![self.answered(&id, tag, Err(self.svc.peer_links_answer(payload)))],
            "secrets.answerRequest" => vec![self.answered(&id, tag, self.svc.answer_secret(payload).map(|()| None))],
            "orchestration.subscribeCommandOutput" => match self.svc.command_output(payload) {
                Ok(frames) => {
                    self.open(id, Kind::Idle, 0, frames);
                    vec![]
                }
                Err(failure) => vec![self.answered(&id, tag, Err(failure))],
            },
            "projects.ensureScratch" | "projects.createNew" | "projects.createFromThread" => {
                let answer = match tag {
                    "projects.ensureScratch" => self.svc.ensure_scratch(),
                    "projects.createNew" => self.svc.create_new(payload),
                    _ => self.svc.create_from_thread(payload),
                };
                vec![self.answered(&id, tag, answer.map(Some))]
            }
            "projects.listEntries" | "projects.readFile" => self.files(id, tag, payload),
            "projects.writeFile" | "filesystem.browse" | "filesystem.getMetadata" | "projects.searchEntries" | "orchestration.getTurnDiff" | "orchestration.getFullThreadDiff" | "vcs.createWorktree" | "worktreeSetup.cancel" => {
                let answer = match tag {
                    "projects.writeFile" => self.svc.write_file(payload),
                    "filesystem.browse" => self.svc.browse(payload),
                    "filesystem.getMetadata" => self.svc.get_metadata(payload),
                    "projects.searchEntries" => self.svc.search_entries(payload),
                    "orchestration.getTurnDiff" => self.svc.thread_diff(payload, false),
                    "orchestration.getFullThreadDiff" => self.svc.thread_diff(payload, true),
                    "vcs.createWorktree" => self.svc.create_worktree(payload),
                    _ => self.svc.cancel_setup(payload),
                };
                vec![self.answered(&id, tag, answer.map(Some))]
            }
            "subscribeWorktreeSetup" => {
                let Some(thread) = payload.get("threadId").and_then(Value::as_str) else { return vec![rpc::exit_failure(&id, wire::not_found_error("thread", ""))] };
                match self.svc.open_setup(thread) {
                    Some((fold, first)) => {
                        let stop = Arc::new(AtomicBool::new(false));
                        if !fold.finished() {
                            self.svc.follow_setup(thread, fold, id.clone(), self.setup_tx.clone(), stop.clone());
                        }
                        self.open(id, Kind::Followed { _stop: StopSetup(stop) }, 0, vec![first]);
                    }
                    None => self.open(id, Kind::Idle, 0, vec![Value::Null]),
                }
                vec![]
            }
            "terminal.open" | "terminal.write" | "terminal.resize" | "terminal.clear" | "terminal.close" => {
                let answer = match tag {
                    "terminal.open" => self.svc.terminal_open(payload).map(Some),
                    "terminal.close" => self.svc.terminal_close(payload).map(|()| None),
                    _ => self.svc.terminal_act(tag, payload).map(|()| None),
                };
                vec![self.answered(&id, tag, answer)]
            }
            "terminal.attach" | "terminal.observe" => match self.svc.open_terminal_stream(payload, tag == "terminal.attach") {
                Ok((follow, first)) => {
                    let stop = Arc::new(AtomicBool::new(false));
                    if !follow.ended() {
                        self.svc.follow_terminal(follow, id.clone(), self.setup_tx.clone(), stop.clone());
                    }
                    self.open(id, Kind::Followed { _stop: StopSetup(stop) }, 0, first);
                    vec![]
                }
                Err(failure) => vec![self.answered(&id, tag, Err(failure))],
            },
            "vcs.refreshStatus" | "vcs.listRefs" | "vcs.switchRef" | "vcs.createRef" | "vcs.pull" | "review.getDiffPreview" | "review.getDiffFileContents" => {
                let answer = match tag {
                    "vcs.refreshStatus" => self.svc.vcs_refresh_status(payload),
                    "vcs.listRefs" => self.svc.vcs_list_refs(payload),
                    "vcs.switchRef" => self.svc.vcs_switch_ref(payload),
                    "vcs.createRef" => self.svc.vcs_create_ref(payload),
                    "vcs.pull" => self.svc.vcs_pull(payload),
                    "review.getDiffPreview" => self.svc.review_diff_preview(payload),
                    _ => self.svc.review_diff_file_contents(payload),
                };
                vec![self.answered(&id, tag, answer.map(Some))]
            }
            "subscribeVcsStatus" => match self.svc.open_vcs_status(payload) {
                Ok((fold, first)) => {
                    let stop = Arc::new(AtomicBool::new(false));
                    self.svc.follow_vcs_status(fold, id.clone(), self.setup_tx.clone(), stop.clone());
                    self.open(id, Kind::Followed { _stop: StopSetup(stop) }, 0, vec![first]);
                    vec![]
                }
                Err(failure) => vec![self.answered(&id, tag, Err(failure))],
            },
            "git.runStackedAction" => {
                let stop = Arc::new(AtomicBool::new(false));
                match self.svc.run_stacked_action(payload, id.clone(), self.setup_tx.clone(), stop.clone()) {
                    Ok(()) => {
                        self.open(id, Kind::Followed { _stop: StopSetup(stop) }, 0, vec![]);
                        vec![]
                    }
                    Err(failure) => vec![self.answered(&id, tag, Err(failure))],
                }
            }
            "subscribeTerminalMetadata" => match self.svc.open_terminal_metadata() {
                Ok((fold, first)) => {
                    let stop = Arc::new(AtomicBool::new(false));
                    self.svc.follow_terminal_metadata(fold, id.clone(), self.setup_tx.clone(), stop.clone());
                    self.open(id, Kind::Followed { _stop: StopSetup(stop) }, 0, vec![first]);
                    vec![]
                }
                Err(failure) => vec![self.answered(&id, tag, Err(failure))],
            },
            "provider.auth.start" | "provider.auth.respond" | "provider.auth.complete" | "provider.auth.cancel" | "provider.auth.logout" | "provider.chatgpt.reconnect-profile" | "provider.chatgpt.import-profile" | "provider.consumeResetCredit" | "server.refreshProviders" | "server.updateProvider" | "server.updateSettings" | "server.getUsageSummary" | "server.refreshUsageRates" | "server.getHostResources" => {
                let answer = match tag {
                    "provider.auth.start" => self.svc.provider_auth_start(payload),
                    "provider.auth.respond" => self.svc.provider_auth_respond(payload),
                    "provider.auth.complete" => self.svc.provider_auth_complete(payload),
                    "provider.auth.cancel" => self.svc.provider_auth_cancel(payload),
                    "provider.auth.logout" => self.svc.provider_auth_logout(payload),
                    "provider.chatgpt.reconnect-profile" => self.svc.chatgpt_reconnect(payload),
                    "provider.chatgpt.import-profile" => Err(self.svc.chatgpt_refused(payload, "import-profile")),
                    "provider.consumeResetCredit" => self.svc.consume_reset_credit(payload),
                    "server.refreshProviders" => self.svc.refresh_providers(payload),
                    "server.updateProvider" => self.svc.update_provider(payload),
                    "server.updateSettings" => self.svc.update_settings(payload),
                    "server.getUsageSummary" => self.svc.usage_summary(payload),
                    "server.refreshUsageRates" => Ok(self.svc.refresh_usage_rates()),
                    _ => self.svc.host_resources(),
                };
                vec![self.answered(&id, tag, answer.map(Some))]
            }
            "provider.chatgpt.handoff.subscribe" => vec![self.answered(&id, tag, Err(self.svc.chatgpt_refused(payload, "handoff")))],
            "provider.auth.subscribe" => match self.svc.open_auth_stream(payload) {
                Ok((fold, first)) => {
                    let stop = Arc::new(AtomicBool::new(false));
                    self.svc.follow_auth(fold, id.clone(), self.setup_tx.clone(), stop.clone());
                    self.open(id, Kind::Followed { _stop: StopSetup(stop) }, 0, vec![first]);
                    vec![]
                }
                Err(failure) => vec![self.answered(&id, tag, Err(failure))],
            },
            "pullRequests.list" | "pullRequests.listStats" | "pullRequests.filterOptions" | "pullRequests.summary" | "pullRequests.preview" | "pullRequests.routing" | "pullRequests.routingIdentity" | "pullRequests.detail" | "pullRequests.checks" | "pullRequests.activity" | "pullRequests.threadComments" | "pullRequests.changeBreakdown" | "pullRequests.labelCandidates" | "pullRequests.reviewerCandidates" | "pullRequests.filesViewed" | "pullRequests.linkedThreads" | "sourceControl.lookupRepository" | "sourceControl.publishRepository" | "sourceControl.appAccess" | "sourceControl.signInStart" | "sourceControl.signInWait" | "projectClone.start" | "projectClone.cancel" | "projectClone.retry" => {
                let answer = match tag {
                    "pullRequests.list" => self.svc.pr_list(payload),
                    "pullRequests.listStats" => self.svc.pr_list_stats(payload),
                    "pullRequests.filterOptions" => self.svc.pr_filter_options(payload),
                    "pullRequests.summary" => self.svc.pr_summary(payload),
                    "pullRequests.preview" => self.svc.pr_preview(payload),
                    "pullRequests.routing" => self.svc.pr_routing(payload),
                    "pullRequests.routingIdentity" => self.svc.pr_routing_identity(payload),
                    "pullRequests.detail" => self.svc.pr_detail(payload),
                    "pullRequests.checks" => self.svc.pr_checks(payload),
                    "pullRequests.activity" => self.svc.pr_activity(payload),
                    "pullRequests.threadComments" => self.svc.pr_thread_comments(payload),
                    "pullRequests.changeBreakdown" => self.svc.pr_change_breakdown(payload),
                    "pullRequests.labelCandidates" => self.svc.pr_label_candidates(payload),
                    "pullRequests.reviewerCandidates" => self.svc.pr_reviewer_candidates(payload),
                    "pullRequests.filesViewed" => self.svc.pr_files_viewed(payload),
                    "pullRequests.linkedThreads" => self.svc.pr_linked_threads(payload),
                    "sourceControl.lookupRepository" => self.svc.sc_lookup_repository(payload),
                    "sourceControl.publishRepository" => self.svc.sc_publish_repository(payload),
                    "sourceControl.appAccess" => self.svc.sc_app_access(payload),
                    "sourceControl.signInStart" => self.svc.sc_sign_in_start(payload),
                    "sourceControl.signInWait" => self.svc.sc_sign_in_wait(payload),
                    "projectClone.start" => self.svc.project_clone_start(payload),
                    "projectClone.cancel" => self.svc.project_clone_cancel(payload),
                    _ => self.svc.project_clone_retry(payload),
                };
                vec![self.answered(&id, tag, answer.map(Some))]
            }
            "pullRequests.comment" | "pullRequests.updateComment" | "pullRequests.replyToThread" | "pullRequests.setThreadResolution" | "pullRequests.setReaction" | "pullRequests.requestReviewers" | "pullRequests.setLabels" | "pullRequests.submitReview" | "pullRequests.update" | "pullRequests.runAction" | "pullRequests.setFilesViewed" | "sourceControl.signInCancel" | "pullRequests.invalidate" | "pullRequests.reportState" => {
                let answer = match tag {
                    "pullRequests.comment" => self.svc.pr_comment(payload),
                    "pullRequests.updateComment" => self.svc.pr_update_comment(payload),
                    "pullRequests.replyToThread" => self.svc.pr_reply(payload),
                    "pullRequests.setThreadResolution" => self.svc.pr_set_thread_resolution(payload),
                    "pullRequests.setReaction" => self.svc.pr_set_reaction(payload),
                    "pullRequests.requestReviewers" => self.svc.pr_request_reviewers(payload),
                    "pullRequests.setLabels" => self.svc.pr_set_labels(payload),
                    "pullRequests.submitReview" => self.svc.pr_submit_review(payload),
                    "pullRequests.update" => self.svc.pr_update(payload),
                    "pullRequests.runAction" => self.svc.pr_run_action(payload),
                    "pullRequests.setFilesViewed" => self.svc.pr_set_files_viewed(payload),
                    "sourceControl.signInCancel" => self.svc.sc_sign_in_cancel(payload),
                    _ => Ok(()),
                };
                vec![self.answered(&id, tag, answer.map(|()| None))]
            }
            "pullRequests.stack" => vec![self.answered(&id, tag, self.svc.pr_stack(payload).map(Some))],
            "pullRequests.reviewGuide" => vec![self.answered(&id, tag, self.svc.pr_review_guide(payload).map(Some))],
            "pullRequests.generateReviewGuide" => vec![self.answered(&id, tag, self.svc.pr_generate_review_guide(payload).map(Some))],
            "pullRequests.diffFileContents" => vec![self.answered(&id, tag, self.svc.pr_diff_file_contents(payload).map(Some))],
            "git.preparePullRequestThread" => vec![self.answered(&id, tag, self.svc.git_prepare_pull_request_thread(payload).map(Some))],
            "server.discoverSourceControl" => vec![rpc::exit_success(&id, Some(self.svc.discover_source_control()))],
            "pullRequests.subscribeRefreshes" => {
                self.open(id, Kind::Idle, 0, vec![json!(0)]);
                vec![]
            }
            "subscribeProjectClones" => {
                let (fold, first) = self.svc.open_project_clones();
                let stop = Arc::new(AtomicBool::new(false));
                self.svc.follow_project_clones(fold, id.clone(), self.setup_tx.clone(), stop.clone());
                self.open(id, Kind::Followed { _stop: StopSetup(stop) }, 0, vec![first]);
                vec![]
            }
            "attachments.createUploadUrl" | "attachments.delete" | "assets.createUrl" | "assets.persistChatAttachments" => {
                let answer = match tag {
                    "attachments.createUploadUrl" => self.svc.attachment_upload_url(payload).map(Some),
                    "attachments.delete" => self.svc.attachment_delete(payload).map(|()| None),
                    "assets.createUrl" => self.svc.asset_create_url(payload).map(Some),
                    _ => self.svc.persist_chat_attachments(payload).map(Some),
                };
                vec![self.answered(&id, tag, answer)]
            }
            "preview.open" | "preview.resize" | "preview.adjust" | "preview.close" | "preview.list" | "preview.reportStatus" => {
                let answer = match tag {
                    "preview.open" => self.svc.preview_open(payload).map(Some),
                    "preview.resize" => self.svc.preview_resize(payload).map(Some),
                    "preview.adjust" => self.svc.preview_adjust(payload).map(Some),
                    "preview.close" => self.svc.preview_close(payload).map(|()| None),
                    "preview.list" => self.svc.preview_list(payload).map(Some),
                    _ => self.svc.preview_report_status(payload).map(|()| None),
                };
                vec![self.answered(&id, tag, answer)]
            }
            "subscribePreviewEvents" => match self.svc.open_preview_events() {
                Ok(cursor) => {
                    // The stream is open before the follower may send: a change it sends first would find no stream to go to
                    let stop = Arc::new(AtomicBool::new(false));
                    self.open(id.clone(), Kind::Followed { _stop: StopSetup(stop.clone()) }, 0, vec![]);
                    self.svc.follow_preview_events(cursor, id, self.setup_tx.clone(), stop);
                    vec![]
                }
                Err(failure) => vec![self.answered(&id, tag, Err(failure))],
            },
            "scheduledTasks.upsert" | "scheduledTasks.setEnabled" | "scheduledTasks.delete" | "scheduledTasks.runNow" | "scheduledTasks.rotateWebhookToken" => {
                let answer = match tag {
                    "scheduledTasks.upsert" => self.svc.scheduled_upsert(payload),
                    "scheduledTasks.setEnabled" => self.svc.scheduled_set_enabled(payload),
                    "scheduledTasks.delete" => self.svc.scheduled_delete(payload),
                    "scheduledTasks.runNow" => self.svc.scheduled_run_now(payload),
                    _ => self.svc.scheduled_rotate_token(payload),
                };
                vec![self.answered(&id, tag, answer.map(Some))]
            }
            "scheduledTasks.subscribe" => match self.svc.open_schedules() {
                Ok((fold, first)) => {
                    let stop = Arc::new(AtomicBool::new(false));
                    self.svc.follow_schedules(fold, id.clone(), self.setup_tx.clone(), stop.clone());
                    self.open(id, Kind::Followed { _stop: StopSetup(stop) }, 0, vec![first]);
                    vec![]
                }
                Err(failure) => vec![self.answered(&id, tag, Err(failure))],
            },
            "server.getDatabaseStorage" => {
                let (fold, first) = self.svc.open_storage_stream();
                let stop = Arc::new(AtomicBool::new(false));
                self.svc.follow_storage(fold, id.clone(), self.setup_tx.clone(), stop.clone());
                self.open(id, Kind::Followed { _stop: StopSetup(stop) }, 0, vec![first]);
                vec![]
            }
            other => {
                self.svc.log("rpc.unsupported", &[("tag", other.to_string()), ("known", "true".into())]);
                vec![rpc::exit_failure(&id, wire::unsupported_error(other))]
            }
        }
    }

    /// The answer of a query: its result, the failure the RPC declares, or a defect when the payload does not decode.
    fn answered(&self, id: &Value, tag: &str, answer: Result<Option<Value>, QueryFailure>) -> Value {
        match answer {
            Ok(result) => rpc::exit_success(id, result),
            Err(QueryFailure::Defect(message)) => {
                self.svc.log("rpc.command.undecodable", &[("tag", tag.to_string())]);
                rpc::exit_defect(id, &message)
            }
            Err(QueryFailure::Error(error)) => {
                self.svc.log("rpc.refused", &[("tag", tag.to_string())]);
                rpc::exit_failure(id, error)
            }
        }
    }

    /// The listing and reading RPCs: the project's directory resolved here, every read done by the agent API under it.
    fn files(&mut self, id: Value, tag: &str, payload: &Value) -> Vec<Value> {
        let text = |key: &str| payload.get(key).and_then(Value::as_str);
        let root = text("cwd").map(str::to_string).or_else(|| text("projectId").and_then(|p| self.svc.engine().project_root(p))).unwrap_or_default();
        let path = text("relativePath").or_else(|| text("path")).or_else(|| text("directory")).unwrap_or("");
        let agent = self.svc.agent();
        let fail = |e: AgentError| vec![rpc::exit_failure(&id, wire::host_error(&e.message()))];
        let ok = |v: Value| vec![rpc::exit_success(&id, Some(v))];
        match tag {
            "projects.listEntries" => match agent.list_dir(&root, path) {
                Ok(entries) => {
                    let join = |name: &str| if path.is_empty() { name.to_string() } else { format!("{}/{name}", path.trim_end_matches('/')) };
                    let rows: Vec<Value> = entries.iter().map(|e| json!({"path": join(&e.name), "kind": if e.directory { "directory" } else { "file" }, "parentPath": if path.is_empty() { Value::Null } else { json!(path) }})).collect();
                    ok(json!({"entries": rows, "truncated": false}))
                }
                Err(e) => fail(e),
            },
            "projects.readFile" => match agent.read_file(&root, path) {
                Ok(contents) => ok(json!({"relativePath": path, "contents": contents})),
                Err(e) => fail(e),
            },
            _ => fail(AgentError::Failed(format!("{tag} is not a file RPC"))),
        }
    }
}
