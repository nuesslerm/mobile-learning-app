use crate::refusal::{channel_of, refuse, Refused};
use agent_api_hosted::clone::{self, CloneRef, SubscribeParams};
use agent_api_hosted::{self as api, code as hosted_code};
use agent_api_proto::{code, text, GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Connection, Group, Served};
use rca_exec_channel::remote::ExecutorChannel;
use rca_exec_channel::Executors;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio::task::AbortHandle;

struct Entry {
    clone: String,
    state: &'static str,
    repo: String,
    detail: Value,
}

#[derive(Default)]
struct Log {
    entries: Vec<Entry>,
    running: HashMap<String, AbortHandle>,
}

/// The clones of each workspace: one numbered log of state changes, which a subscriber replays and then follows. The log is the service's own
/// memory of a request it made; the clone itself is the executor's process, cancelled by dropping the request.
struct Clones {
    logs: Mutex<HashMap<String, Log>>,
    changed: watch::Sender<u64>,
    next: AtomicU64,
}

impl Clones {
    fn log<R>(&self, workspace: &str, f: impl FnOnce(&mut Log) -> R) -> R {
        let mut logs = self.logs.lock().unwrap_or_else(|p| p.into_inner());
        f(logs.entry(workspace.to_string()).or_default())
    }

    fn push(&self, workspace: &str, clone: &str, state: &'static str, repo: &str, detail: Value) {
        self.log(workspace, |log| log.entries.push(Entry { clone: clone.into(), state, repo: repo.into(), detail }));
        self.changed.send_modify(|n| *n += 1);
    }

    fn latest(&self, workspace: &str, clone: &str) -> Option<Value> {
        self.log(workspace, |log| log.entries.iter().rev().find(|e| e.clone == clone).map(|e| json!({"workspace": workspace, "clone": e.clone, "repo": e.repo, "state": e.state, "detail": e.detail})))
    }

    /// Records `running` and starts the executor's clone; its end is the next entry unless a cancel came first.
    fn start(self: &Arc<Self>, workspace: &str, channel: Arc<dyn ExecutorChannel>, args: Value, repo: String) -> String {
        let id = format!("c{}", self.next.fetch_add(1, Ordering::SeqCst));
        self.push(workspace, &id, "running", &repo, json!({}));
        let (me, ws, clone) = (self.clone(), workspace.to_string(), id.clone());
        // Spawned under the log's lock: the task's own end takes it, so it cannot look for its handle before the handle is there.
        self.log(workspace, |log| {
            let task = tokio::spawn(async move {
                let outcome = channel.call(exec_hosted::REPO_CLONE, args).await;
                let still_running = me.log(&ws, |log| log.running.remove(&clone).is_some());
                if !still_running {
                    return;
                }
                match outcome {
                    Ok(done) => me.push(&ws, &clone, "done", &repo, json!({"path": done.result["path"]})),
                    Err(e) => me.push(&ws, &clone, "failed", &repo, json!({"message": Refused::from(e).message})),
                }
            });
            log.running.insert(id.clone(), task.abort_handle());
        });
        id
    }

    /// `Some(true)` when a running clone was stopped, `Some(false)` when it had ended already, `None` when no clone has the id.
    fn cancel(&self, workspace: &str, clone: &str) -> Option<bool> {
        let known = self.log(workspace, |log| log.entries.iter().any(|e| e.clone == clone));
        if !known {
            return None;
        }
        let handle = self.log(workspace, |log| log.running.remove(clone));
        let repo = self.latest(workspace, clone).map(|l| l["repo"].as_str().unwrap_or_default().to_string()).unwrap_or_default();
        match handle {
            Some(handle) => {
                handle.abort();
                self.push(workspace, clone, "cancelled", &repo, json!({"message": "cancelled"}));
                Some(true)
            }
            None => Some(false),
        }
    }

    fn since(&self, workspace: &str, seen: u64) -> Vec<(u64, Entry)> {
        self.log(workspace, |log| log.entries.iter().enumerate().skip(seen as usize).map(|(i, e)| (i as u64 + 1, Entry { clone: e.clone.clone(), state: e.state, repo: e.repo.clone(), detail: e.detail.clone() })).collect())
    }
}

/// `hosted.clone`: `clone.start` answers at once with the clone's id and the executor's clone runs on; the other methods answer from the log.
pub struct HostedCloneGroup {
    executors: Arc<dyn Executors>,
    clones: Arc<Clones>,
    served: Vec<Served>,
}

impl HostedCloneGroup {
    pub fn new(executors: Arc<dyn Executors>) -> HostedCloneGroup {
        let clones = Arc::new(Clones { logs: Mutex::default(), changed: watch::channel(0).0, next: AtomicU64::new(1) });
        HostedCloneGroup { executors, clones, served: crate::pr::rows(clone::SPECS) }
    }

    fn start(&self, params: &Value) -> Result<Value, Refused> {
        let workspace = api::workspace_param(params)?;
        let spec = &clone::SPECS[0];
        let args = api::executor_args(spec, exec_hosted::REPO_CLONE, params);
        let repo = text(&args, "repo").ok_or_else(|| refuse(code::INVALID_PARAMS, "repo is required"))?.to_string();
        if text(&args, "dest").is_none() {
            return Err(refuse(code::INVALID_PARAMS, "dest is required"));
        }
        let mut args = args;
        if args.get("provider").is_none() {
            args["provider"] = json!("github");
        }
        let channel = channel_of(&self.executors, &workspace)?;
        let id = self.clones.start(&workspace, channel, args, repo.clone());
        Ok(json!({"workspace": workspace, "clone": id, "repo": repo, "state": "running"}))
    }

    fn status(&self, params: &Value) -> Result<Value, Refused> {
        let r = CloneRef::parse(params)?;
        self.clones.latest(&r.workspace, &r.clone).ok_or_else(|| not_found(&r.clone))
    }

    fn cancel(&self, params: &Value) -> Result<Value, Refused> {
        let r = CloneRef::parse(params)?;
        let cancelled = self.clones.cancel(&r.workspace, &r.clone).ok_or_else(|| not_found(&r.clone))?;
        let mut answer = self.clones.latest(&r.workspace, &r.clone).unwrap_or(Value::Null);
        answer["cancelled"] = json!(cancelled);
        Ok(answer)
    }

    /// Answers with where the numbering stands, then replays the log after `after_seq` and follows it until the peer goes.
    fn subscribe(&self, params: &Value, connection: &Connection) -> Result<Value, Refused> {
        let p = SubscribeParams::parse(params)?;
        let sink = connection.sink();
        let (clones, workspace, mut seen) = (self.clones.clone(), p.workspace.clone(), p.after_seq);
        let mut changed = clones.changed.subscribe();
        connection.follow(format!("hosted/{}/clones", p.workspace), async move {
            loop {
                for (seq, e) in clones.since(&workspace, seen) {
                    if !sink.send_replay(&api::clone_event(seq, &workspace, &e.clone, e.state, &e.repo, e.detail)).await {
                        return;
                    }
                    seen = seq;
                }
                if changed.changed().await.is_err() {
                    return;
                }
            }
        });
        Ok(json!({"workspace": p.workspace, "seq": p.after_seq}))
    }
}

fn not_found(clone: &str) -> Refused {
    refuse(hosted_code::CLONE_NOT_FOUND, format!("no clone {clone}"))
}

#[async_trait]
impl Group for HostedCloneGroup {
    fn name(&self) -> GroupName {
        api::GROUP_CLONE
    }

    fn methods(&self) -> &[Served] {
        &self.served
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let answer = match call.method {
            clone::START => self.start(call.params),
            clone::STATUS => self.status(call.params),
            clone::CANCEL => self.cancel(call.params),
            clone::SUBSCRIBE => self.subscribe(call.params, call.connection),
            other => unreachable!("{} is not a method of hosted.clone", other.name()),
        };
        match answer {
            Ok(result) => Response::result(call.id, result),
            Err(r) => Response::error(call.id, &r.code, r.message),
        }
    }
}
