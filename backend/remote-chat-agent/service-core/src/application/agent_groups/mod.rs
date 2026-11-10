//! The capability groups this service registers on the agent API (`rca-agent-core`): `service` and `service.activity`, each answering
//! the methods of its contract crate (`libraries/agent-api`). `state` and `state.watch` are `rca-group-state`. `files`, `files.edit`, `workspace` and
//! `workspace.worktrees` are `rca-group-workspace`; `terminal` is `rca-group-terminal` and `git` is `rca-group-git` and `hosted.pr`, `hosted.clone` and `hosted.auth` are `rca-group-hosted` (the executor machine's own gh/glab sign-in: the service holds no token), over the same executors (`rca-exec-channel`). `agent.threads` is its own crate (`rca-group-agent`); `engine_threads` is the adapter that gives it the thread engine. The
//! composition root (`rs`) builds them and registers them; the dispatch and the listing of `service.status` are the core's.

#[cfg(test)]
mod engine_contract;
#[cfg(test)]
mod engine_groups;
pub mod activity;
mod engine_lifecycle;
pub mod engine_threads;
pub mod attachment_staging;
pub mod schedule_trigger;
pub mod service;

pub use engine_threads::EngineThreads;

pub use activity::{ActivityClock, ActivityGroup, SystemClock};
pub use attachment_staging::StagesAttachments;
pub use rca_group_attachments::{Attachments, AttachmentsGroup, BlobStore, Clock as AttachmentClock, Ids as AttachmentIds, Put as BlobPut};
pub use rca_group_catalog::{CatalogGroup, Catalog, Clock as CatalogClock};
pub use rca_group_git::GitGroup;
pub use rca_group_hosted::{ExecutorCredentials, HostCredentials, HostedAuthGroup, HostedCloneGroup, HostedPrGroup};
pub use rca_group_preview::{Clock as PreviewClock, Ids as PreviewIds, PreviewGroup, PreviewSessionGroup, PreviewWatchGroup, Previews};
pub use rca_group_schedules::{Clock as ScheduleClock, Ids as ScheduleIds, ScheduleGroup, ScheduleStore, ScheduleWatchGroup, Schedules, Stored as StoredSchedule, WebhookOutcome, WebhookRequest};
pub use rca_group_terminal::TerminalGroup;
pub use schedule_trigger::ThreadsTrigger;
pub use rca_group_workspace::{FilesEditGroup, FilesGroup, WorkspaceGroup, WorktreesGroup};
pub use service::ServiceGroup;
pub use rca_group_state::{StateGroup, StateHub, StateStore, StateWatchGroup};

#[cfg(test)]
mod tests {
    use super::*;
    use agent_api_agent as threads_api;
    use agent_api_proto::Method;
    use agent_api_service as service_api;
    use agent_api_state as state_api;
    use agent_api_workspace as workspace_api;
    use rca_agent_core::{AgentApi, Build, Group, Served, Uses};
    use rca_cli_driver::testing::ScriptedSpawner;
    use rca_group_state::testing::MemoryStore;
    use rca_exec_channel::mcp::TOOLS;
    use rca_exec_channel::remote::{ActionOutput, ExecutorChannel, RemoteError};
    use rca_exec_channel::testing::ScriptedChannel;
    use rca_exec_channel::{contract_actions, Registry};
    use serde_json::{json, Value};
    use std::sync::Arc;

    struct NoMcp;

    impl rca_group_agent::McpEndpoint for NoMcp {
        fn url(&self, thread: &str) -> String {
            format!("http://127.0.0.1:1/mcp/{thread}")
        }
    }

    fn groups(executors: &Arc<Registry>) -> Vec<Arc<dyn Group>> {
        let runner = rca_group_agent::Runner::new(Arc::new(rca_group_agent::testing::MemoryThreads::default()), executors.clone(), ScriptedSpawner::new(), Arc::new(NoMcp), std::path::PathBuf::from("/srv/rca"));
        let hub = Arc::new(StateHub::new(Arc::new(MemoryStore::default())));
        let previews = Previews::new(Arc::new(rca_group_preview::testing::FakeClock::at(1)), Arc::new(rca_group_preview::testing::CountingIds::default()), 0);
        vec![
            Arc::new(ServiceGroup),
            Arc::new(ActivityGroup::new(Arc::new(SystemClock))),
            Arc::new(StateGroup::new(hub.clone())),
            Arc::new(StateWatchGroup::new(hub)),
            Arc::new(FilesGroup::new(executors.clone())),
            Arc::new(FilesEditGroup::new(executors.clone())),
            Arc::new(WorkspaceGroup::new(executors.clone())),
            Arc::new(WorktreesGroup::new(executors.clone())),
            Arc::new(TerminalGroup::new(executors.clone(), Arc::new(SystemClock))),
            Arc::new(GitGroup::new(executors.clone())),
            Arc::new(HostedPrGroup::new(executors.clone())),
            Arc::new(HostedCloneGroup::new(executors.clone())),
            Arc::new(HostedAuthGroup::new(executors.clone(), Arc::new(ExecutorCredentials::new(executors.clone())))),
            Arc::new(rca_group_agent::AgentThreadsGroup::with_runner(runner.clone())),
            Arc::new(rca_group_agent::AgentRequestsGroup::new(runner.clone())),
            Arc::new(rca_group_agent::AgentQueueGroup::new(runner.clone())),
            Arc::new(rca_group_agent::AgentLifecycleGroup::new(runner)),
            Arc::new(AttachmentsGroup::new(Attachments::new(Arc::new(rca_group_attachments::testing::MemoryBlobs::default()), Arc::new(rca_group_attachments::testing::FixedClock(1)), Arc::new(rca_group_attachments::testing::CountingIds::default()), executors.clone()))),
            Arc::new(CatalogGroup::new(Catalog::new(executors.clone(), Arc::new(rca_group_catalog::testing::SetClock::at(1))))),
            Arc::new(PreviewGroup::new(executors.clone())),
            Arc::new(PreviewSessionGroup::new(previews.clone())),
            Arc::new(PreviewWatchGroup::new(previews)),
        ]
    }

    fn registered() -> AgentApi {
        AgentApi::new(groups(&Arc::default())).unwrap()
    }

    const BUILD: Build<'static> = Build { version: "1.2.3", commit: "abc" };

    async fn call(api: &AgentApi, frame: Value) -> Value {
        api.dispatch_text(BUILD, &frame.to_string()).await
    }

    fn contract_methods() -> Vec<Method> {
        [service_api::METHODS, state_api::METHODS, state_api::watch::METHODS, workspace_api::METHODS, workspace_api::edit::METHODS, workspace_api::worktrees::METHODS, agent_api_terminal::METHODS, agent_api_git::METHODS, agent_api_hosted::all_methods().as_slice(), threads_api::METHODS, threads_api::requests::METHODS, threads_api::queue::METHODS, threads_api::lifecycle::METHODS, agent_api_attachments::METHODS, agent_api_catalog::METHODS, agent_api_preview::METHODS, agent_api_preview::sessions::METHODS, agent_api_preview::watch::METHODS].concat()
    }

    #[tokio::test]
    async fn given_bad_frames_when_handled_then_each_gets_its_error_code() {
        let api = registered();
        let code = |frame: Value| {
            let api = &api;
            async move { call(api, frame).await["error"]["code"].clone() }
        };
        assert_eq!(code(json!({"method": "service.status"})).await, json!("INVALID_REQUEST"));
        assert_eq!(code(json!({"id": 1})).await, json!("INVALID_REQUEST"));
        assert_eq!(code(json!({"id": 1, "method": "terminal.destroy"})).await, json!("UNKNOWN_METHOD"));
    }

    #[tokio::test]
    async fn given_the_twenty_two_groups_when_status_is_asked_then_it_names_the_protocol_the_build_and_the_complete_groups_in_order() {
        // Given a service that registered service, service.activity, state, state.watch, files, files.edit, workspace, workspace.worktrees, terminal, git, hosted.pr, hosted.clone, hosted.auth, agent.threads and its three sub-groups, preview and its two sub-groups, workspace having methods of its owner set still to serve
        let api = registered();
        // When the status is asked
        let status = call(&api, json!({"id": 7, "method": "service.status"})).await;
        // Then the answer lists the twenty-one complete groups, in registration order, and not workspace, though workspace.list is answered
        assert_eq!(status, json!({"id": 7, "result": {"protocol": 1, "version": "1.2.3", "commit": "abc", "groups": ["service", "service.activity", "state", "state.watch", "files", "files.edit", "workspace.worktrees", "terminal", "git", "hosted.pr", "hosted.clone", "hosted.auth", "agent.threads", "agent.requests", "agent.queue", "agent.lifecycle", "attachments", "agent.catalog", "preview", "preview.sessions", "preview.watch"]}}));
        assert_eq!(call(&api, json!({"id": 8, "method": "workspace.list"})).await, json!({"id": 8, "result": {"workspaces": []}}));
    }

    #[tokio::test]
    async fn given_executors_attached_when_the_file_methods_are_called_then_the_executor_of_the_workspace_answers_and_its_failures_keep_their_code() {
        // Given an executor attached for w1 that answers read_file and refuses anything with an INVALID_ARGS it is not given a number for
        let executors: Arc<Registry> = Arc::default();
        let w1 = ScriptedChannel::new(|tool, args| match (tool, args.get("limit")) {
            (_, Some(limit)) if !limit.is_number() => Err(RemoteError::Action { code: "INVALID_ARGS".into(), message: "limit must be a number".into() }),
            ("git_diff", _) => Ok(ActionOutput { result: json!({"diff": args["path"].as_str().map_or("the tree", |p| p)}), output: String::new() }),
            (_, _) => Ok(ActionOutput { result: json!({"content": "x"}), output: String::new() }),
        });
        executors.open("w1", w1.clone() as Arc<dyn ExecutorChannel>).unwrap();
        let api = AgentApi::new(groups(&executors)).unwrap();
        // When a read forwards offset, limit, depth and base, one with a limit that is no number, a diff whose path is no string, and a read in a workspace nobody attached
        let forwarded = call(&api, json!({"id": 1, "method": "file.read", "params": {"workspace": "w1", "path": "a", "offset": 2, "limit": 5, "depth": 1, "base": "HEAD", "other": true}})).await;
        let refused = call(&api, json!({"id": 2, "method": "file.read", "params": {"workspace": "w1", "path": "a", "limit": "ten"}})).await;
        let tree = call(&api, json!({"id": 3, "method": "file.diff", "params": {"workspace": "w1", "path": 7}})).await;
        let detached = call(&api, json!({"id": 4, "method": "file.read", "params": {"workspace": "w2", "path": "a"}})).await;
        // Then the executor was given the four params as sent and nothing else, its refusal came back with its own code, the diff was the whole tree, and the detached one is NO_EXECUTOR
        assert_eq!(forwarded, json!({"id": 1, "result": {"content": "x"}}));
        assert_eq!(w1.calls()[0], ("read_file".to_string(), json!({"path": "a", "offset": 2, "limit": 5, "depth": 1, "base": "HEAD"})));
        assert_eq!(refused, json!({"id": 2, "error": {"code": "INVALID_ARGS", "message": "limit must be a number"}}));
        assert_eq!(tree, json!({"id": 3, "result": {"diff": "the tree"}}));
        assert_eq!((detached["error"]["code"].clone(), detached["error"]["message"].clone()), (json!("NO_EXECUTOR"), json!("no executor is attached for workspace w2")));
        // And a missing workspace or path is INVALID_PARAMS before any executor is asked
        let asked = w1.calls().len();
        assert_eq!(call(&api, json!({"id": 5, "method": "file.list", "params": {"path": "."}})).await["error"]["code"], json!("INVALID_PARAMS"));
        assert_eq!(call(&api, json!({"id": 6, "method": "file.read", "params": {"workspace": "w1"}})).await["error"]["message"], json!("path is required"));
        assert_eq!(w1.calls().len(), asked);
    }

    /// The methods of the contract crates that no row of `served` names.
    fn undeclared(contract: &[Method], served: &[Served]) -> Vec<&'static str> {
        contract.iter().filter(|m| !served.iter().any(|s| s.method == **m)).map(|m| m.name()).collect()
    }

    /// The actions that no row of `served` calls and that no MCP tool of `tools` reaches. A tool reaches an action only for the model of a CLI
    /// turn, so the tools count while some row starts one.
    fn unused<'a>(actions: impl Iterator<Item = &'a str>, served: &[Served], tools: &[&str]) -> Vec<&'a str> {
        let a_model_calls_tools = served.iter().any(|s| s.uses == Uses::Cli);
        actions.filter(|a| !served.iter().any(|s| s.actions.contains(a)) && !(a_model_calls_tools && tools.contains(a))).collect()
    }

    /// The rows of the registered groups.
    fn served_rows() -> Vec<Served> {
        registered().served().copied().collect()
    }

    #[test]
    fn given_the_methods_of_every_contract_crate_when_checked_against_the_registered_groups_then_each_is_served_and_each_served_method_is_the_contracts() {
        // Given the methods the contract crates define and the rows the registered groups answer
        let (contract, served) = (contract_methods(), served_rows());
        // When they are compared, Then none of the contract lacks a row (a method with no actions has a row with none), and no row names another method
        assert_eq!(undeclared(&contract, &served), Vec::<&str>::new());
        assert!(served.iter().all(|s| contract.contains(&s.method)));
    }

    #[test]
    fn given_a_method_added_to_a_contract_crate_without_a_row_when_checked_then_it_is_reported() {
        // Given the workspace crate's methods and a planted file.chmod that no group serves
        let planted = [workspace_api::METHODS, &[Method::new(workspace_api::FILES, "file.chmod")]].concat();
        // When they are compared with the rows, Then only the planted method has no row
        assert_eq!(undeclared(&planted, &served_rows()), ["file.chmod"]);
    }

    #[test]
    fn given_the_actions_of_the_executor_contract_when_checked_against_the_rows_and_the_mcp_tools_then_each_is_used_and_each_row_names_a_real_one() {
        // Given the actions the family crates define, and the rows of the registered groups
        let (actions, served): (Vec<&str>, _) = (contract_actions().map(|a| a.name).collect(), served_rows());
        // Then each is called by a served method or is an MCP tool
        assert_eq!(unused(actions.iter().copied(), &served, &TOOLS), Vec::<&str>::new());
        // And every action a row calls, and every MCP tool, is an action of the contract
        assert!(served.iter().flat_map(|s| s.actions).all(|a| actions.contains(a)));
        assert!(TOOLS.iter().all(|t| actions.contains(t)));
        // And an action added to a family crate that nothing uses is reported after them
        assert_eq!(unused(actions.iter().copied().chain(["format_disk"]), &served, &TOOLS), ["format_disk"]);
    }

    #[test]
    fn given_no_method_that_starts_a_cli_when_the_actions_are_checked_then_the_actions_only_a_tool_reaches_are_reported_unused() {
        // Given the rows with the two methods that drive a CLI taken out, and the actions of the contract
        let (actions, served): (Vec<&str>, Vec<Served>) = (contract_actions().map(|a| a.name).collect(), served_rows().into_iter().filter(|s| s.uses != Uses::Cli).collect());
        // When the actions are checked
        let reported = unused(actions.iter().copied(), &served, &TOOLS);
        // Then they are the tools that no row calls itself: with a CLI turn they were used, with none no model calls them
        let called_by_a_row: Vec<&str> = served.iter().flat_map(|s| s.actions.iter().copied()).collect();
        let mut only_a_tool_reaches: Vec<&str> = TOOLS.iter().copied().filter(|t| !called_by_a_row.contains(t)).collect();
        // and the one a row that starts a turn calls itself (the checkpoint), which this test took out with the rows
        only_a_tool_reaches.extend(served_rows().iter().filter(|s| s.uses == Uses::Cli).flat_map(|s| s.actions.iter().copied()).filter(|a| !called_by_a_row.contains(a)));
        only_a_tool_reaches.dedup();
        let mut reported_sorted: Vec<&str> = reported.clone();
        only_a_tool_reaches.sort_unstable();
        reported_sorted.sort_unstable();
        assert_eq!(reported_sorted, only_a_tool_reaches);
        assert!(reported.contains(&"edit_file") && reported.contains(&"run_command") && !reported.contains(&"read_file") && !reported.contains(&"write_file"), "{reported:?}");
    }

    #[test]
    fn given_the_eight_agent_threads_rows_when_compared_with_the_contract_then_they_are_its_methods_and_only_the_turn_methods_use_a_cli() {
        // Given the group's rows and the contract's methods
        let declared = rca_group_agent::SERVED;
        // Then there is one row for each method of agent_api_agent::METHODS and no other, in the contract's order
        assert_eq!(declared.iter().map(|s| s.method).collect::<Vec<_>>(), threads_api::METHODS);
        // And the thread methods use the service, turn.start and turn.interrupt the CLI, and only turn.start calls an executor action itself (the checkpoint)
        let uses: Vec<(&str, Uses)> = declared.iter().map(|s| (s.method.name(), s.uses)).collect();
        assert_eq!(uses.iter().filter(|(_, u)| *u == Uses::Cli).map(|(m, _)| *m).collect::<Vec<_>>(), ["turn.start", "turn.interrupt"]);
        assert_eq!(uses.iter().filter(|(_, u)| *u == Uses::Service).count(), 6);
        assert_eq!(declared.iter().filter(|s| !s.actions.is_empty()).map(|s| (s.method.name(), s.actions.to_vec())).collect::<Vec<_>>(), [("turn.start", vec![exec_git::GIT_CHECKPOINT])]);
    }

    #[test]
    fn given_the_rows_of_the_three_sub_groups_when_compared_with_the_contract_then_each_is_its_modules_methods_in_order_and_only_the_ones_that_start_or_signal_a_cli_use_one() {
        // Given each sub-group's rows and its module's methods
        let groups = [(rca_group_agent::requests::SERVED.as_slice(), threads_api::requests::METHODS), (rca_group_agent::queue::SERVED.as_slice(), threads_api::queue::METHODS), (rca_group_agent::lifecycle::SERVED.as_slice(), threads_api::lifecycle::METHODS)];
        for (declared, contract) in groups {
            // Then there is one row for each method and no other, in the module's order, and only the rollback that restores files calls an executor
            // action itself (the discard: the checkpoint of a turn that starts is declared once, by turn.start)
            assert_eq!(declared.iter().map(|s| s.method).collect::<Vec<_>>(), contract);
        }
        let calling: Vec<(&str, Vec<&str>)> = [rca_group_agent::requests::SERVED.as_slice(), rca_group_agent::queue::SERVED.as_slice(), rca_group_agent::lifecycle::SERVED.as_slice()].concat().iter().filter(|s| !s.actions.is_empty()).map(|s| (s.method.name(), s.actions.to_vec())).collect();
        assert_eq!(calling, [("thread.rollback", vec![exec_git::GIT_DISCARD])]);
        // And the methods that start a turn or end one are the ones that use a CLI
        let cli: Vec<&str> = [rca_group_agent::requests::SERVED.as_slice(), rca_group_agent::queue::SERVED.as_slice(), rca_group_agent::lifecycle::SERVED.as_slice()].concat().iter().filter(|s| s.uses == Uses::Cli).map(|s| s.method.name()).collect();
        assert_eq!(cli, ["queue.resume", "turn.steer", "thread.launch", "thread.forceStop", "workflow.stop", "turn.retry"]);
    }

    #[test]
    fn given_a_method_added_to_agent_api_agent_without_a_row_when_checked_then_the_check_names_it() {
        // Given the contract's methods with a planted turn.steer, and the rows as declared
        let planted = [threads_api::METHODS, &[Method::new(threads_api::GROUP, "turn.steer")]].concat();
        // When they are compared, Then only the planted method has no row
        assert_eq!(undeclared(&planted, &served_rows()), ["turn.steer"]);
    }

    /// The methods of `contract` that belong to `group` and that it neither serves nor declares pending.
    fn unaccounted(group: &dyn Group, contract: &[Method]) -> Vec<&'static str> {
        let owned = contract.iter().filter(|m| m.group() == group.name()).map(|m| m.name());
        owned.filter(|name| !group.methods().iter().any(|s| s.method.name() == *name) && !group.pending().contains(name)).collect()
    }

    #[test]
    fn given_each_registered_group_when_checked_against_the_contract_then_it_serves_every_method_of_its_name_but_those_it_declares_pending() {
        // Given the registered groups and the contract's methods
        let contract = contract_methods();
        for group in registered().groups() {
            // Then every contract method of the group is served or pending, and nothing is both
            assert_eq!(unaccounted(group.as_ref(), &contract), Vec::<&str>::new(), "{}", group.name().as_str());
            assert!(group.pending().iter().all(|p| !group.methods().iter().any(|s| s.method.name() == *p)), "{} serves a method it declares pending", group.name().as_str());
        }
    }

    #[test]
    fn given_a_method_added_to_a_group_of_the_contract_when_no_group_serves_or_declares_it_then_the_check_names_it() {
        // Given the contract with a planted file.chmod in the files group, and the files group as registered
        let contract = [contract_methods(), vec![Method::new(workspace_api::FILES, "file.chmod")]].concat();
        let files = FilesGroup::new(Arc::new(Registry::default()));
        // When the group is checked against it, Then the planted method is the one unaccounted for
        assert_eq!(unaccounted(&files, &contract), ["file.chmod"]);
    }
}
