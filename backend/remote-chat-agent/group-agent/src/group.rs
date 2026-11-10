//! The `agent.threads` group (`agent-api-agent`): the eight methods over the thread engine. `thread.*` read and write the engine's log through
//! its port, `thread.subscribe` follows it, and `turn.start` runs an agent CLI whose file and shell tools are off and whose only tool server
//! is the thread's workspace executor (`command_for_remote`), recording what the CLI does as the thread's events.
use crate::port::{NewThread, ThreadEngine};
use crate::staging::AttachmentStaging;
use crate::refusal::{respond, to_value, Answer, Refusal};
use crate::runner::{Known, McpEndpoint, Runner, TurnInput};
use agent_api_agent::{self as threads, message, CreateParams, GetParams, HistoryParams, HistoryResult, InterruptParams, InterruptResult, ListParams, ListResult, StartParams, StartResult, SubscribeParams, SubscribeResult, ThreadView, TranscriptParams, TranscriptResult};
use agent_api_proto::{thread_topic, GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_cli_driver::application::ports::Spawner;
use rca_exec_channel::Executors;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

pub const SERVED: [Served; 8] = [
    Served::service(threads::THREAD_CREATE, &[]),
    Served::service(threads::THREAD_LIST, &[]),
    Served::service(threads::THREAD_GET, &[]),
    Served::service(threads::THREAD_SUBSCRIBE, &[]),
    Served::service(threads::THREAD_HISTORY, &[]),
    Served::service(threads::THREAD_TRANSCRIPT, &[]),
    // Every row that starts a turn takes the checkpoint (`checkpoint::take`); the recorded exchange of this one is the contract's evidence
    Served::cli(threads::TURN_START, &[exec_git::GIT_CHECKPOINT]),
    Served::cli(threads::TURN_INTERRUPT, &[]),
];

/// The title of a thread whose creator named none.
const UNTITLED: &str = "New thread";

pub struct AgentThreadsGroup {
    runner: Arc<Runner>,
    staging: Option<Arc<dyn AttachmentStaging>>,
}

impl AgentThreadsGroup {
    pub fn new(engine: Arc<dyn ThreadEngine>, executors: Arc<dyn Executors>, spawner: Arc<dyn Spawner>, mcp: Arc<dyn McpEndpoint>, cli_cwd: PathBuf) -> AgentThreadsGroup {
        AgentThreadsGroup { runner: Runner::new(engine, executors, spawner, mcp, cli_cwd), staging: None }
    }

    /// The group over a runner the other groups of the thread surface share, so a turn it starts is one they can steer, stop and queue after.
    pub fn with_runner(runner: Arc<Runner>) -> AgentThreadsGroup {
        AgentThreadsGroup { runner, staging: None }
    }

    /// The group with the attachments group behind `turn.start`'s `attachments`: each is written into the workspace before the CLI starts.
    pub fn with_staging(mut self, staging: Arc<dyn AttachmentStaging>) -> AgentThreadsGroup {
        self.staging = Some(staging);
        self
    }

    fn engine(&self) -> &Arc<dyn ThreadEngine> {
        self.runner.engine()
    }

    fn thread(&self, thread: &str) -> Result<ThreadView, Refusal> {
        self.runner.thread(thread)
    }

    fn create(&self, params: &Value) -> Answer {
        let p = CreateParams::parse(params)?;
        let id = p.thread.unwrap_or_else(|| self.engine().new_id());
        let view = self.engine().create(NewThread { id, title: p.title.unwrap_or_else(|| UNTITLED.into()), workspace: p.workspace, driver: p.driver, model: p.model })?;
        Ok(view.to_result())
    }

    fn get(&self, params: &Value) -> Answer {
        Ok(self.thread(&GetParams::parse(params)?.thread)?.to_result())
    }

    fn list(&self, params: &Value) -> Answer {
        ListParams::parse(params)?;
        to_value(&ListResult { threads: self.engine().list()? })
    }

    fn history(&self, params: &Value) -> Answer {
        let p = HistoryParams::parse(params)?;
        let page = self.engine().history(&p.thread, p.after_seq, p.page() as usize)?.ok_or_else(|| Refusal::not_found(&p.thread))?;
        to_value(&HistoryResult::new(&page.events, page.last_seq, page.more))
    }

    fn transcript(&self, params: &Value) -> Answer {
        let p = TranscriptParams::parse(params)?;
        let page = self.engine().history(&p.thread, 0, usize::MAX)?.ok_or_else(|| Refusal::not_found(&p.thread))?;
        to_value(&TranscriptResult::fold(&p.thread, &page.events))
    }

    /// Answers with where the replay ends and leaves the stream on the connection: the replay and then each new event are pushed once the
    /// answer is queued.
    fn subscribe(&self, params: &Value, connection: &rca_agent_core::Connection) -> Answer {
        let p = SubscribeParams::parse(params)?;
        let subscription = self.engine().follow(&p.thread, p.after_seq)?.ok_or_else(|| Refusal::not_found(&p.thread))?;
        let (result, sink) = (SubscribeResult::new(&p.thread, p.after_seq, subscription.last_seq), connection.sink());
        connection.follow(thread_topic(&p.thread), async move {
            for event in &subscription.replay {
                if !sink.send_replay(event).await {
                    return;
                }
            }
            let mut live = subscription.live;
            while let Some(event) = live.next().await {
                if !sink.send(&event) {
                    return;
                }
            }
        });
        to_value(&result)
    }

    async fn start(&self, params: &Value) -> Answer {
        let p = StartParams::parse(params)?;
        let ids = p.attachments.clone().unwrap_or_default();
        if !ids.is_empty() && self.staging.is_none() {
            return Err(Refusal::invalid("attachments are not served: this service lists no attachments group"));
        }
        let thread = self.thread(&p.thread)?;
        let input = TurnInput { workspace: p.workspace.clone(), driver: p.driver.clone(), model: p.model.clone(), prompt: p.prompt.clone(), runtime_mode: p.runtime_mode.clone(), interaction_mode: p.interaction_mode.clone() };
        let mut prepared = self.runner.prepare(&p.thread, &Known::of(&thread), None, input)?;
        if let Some(staging) = self.staging.as_ref().filter(|_| !ids.is_empty()) {
            prepared.note_files(&staging.stage(&p.workspace, &ids).await?);
        }
        let turn = prepared.launch().await?;
        to_value(&StartResult { thread: p.thread, turn })
    }

    fn interrupt(&self, params: &Value) -> Answer {
        let p = InterruptParams::parse(params)?;
        self.thread(&p.thread)?;
        let turn = self.runner.interrupt(&p.thread, p.turn.as_deref()).ok_or_else(|| Refusal::new(threads::code::NO_TURN, message::no_turn(&p.thread, p.turn.as_deref())))?;
        to_value(&InterruptResult { thread: p.thread, turn })
    }
}

#[async_trait]
impl Group for AgentThreadsGroup {
    fn name(&self) -> GroupName {
        threads::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, connection, .. } = call;
        let answer = match method.name() {
            "thread.create" => self.create(params),
            "thread.list" => self.list(params),
            "thread.get" => self.get(params),
            "thread.subscribe" => self.subscribe(params, connection),
            "thread.history" => self.history(params),
            "thread.transcript" => self.transcript(params),
            "turn.start" => self.start(params).await,
            "turn.interrupt" => self.interrupt(params),
            other => unreachable!("{other} is no method of agent.threads (SERVED)"),
        };
        respond(id, answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MemoryThreads;
    use agent_api_agent::Status;
    use agent_api_proto::Event;
    use serde_json::json;
    use rca_agent_core::{AgentApi, Build, Connection, EventSink};
    use rca_cli_driver::testing::{CliScript, ScriptedSpawner};
    use rca_exec_channel::remote::{ActionOutput, ExecutorChannel};
    use rca_exec_channel::testing::ScriptedChannel;
    use rca_exec_channel::Registry;
    use std::sync::Mutex;
    use std::time::Duration;

    const BUILD: Build<'static> = Build { version: "1", commit: "c" };

    struct Mcp;

    impl McpEndpoint for Mcp {
        fn url(&self, thread: &str) -> String {
            format!("http://127.0.0.1:1/mcp/{thread}")
        }
    }

    /// What a socket was sent, in order.
    #[derive(Default)]
    struct Pushed(Mutex<Vec<Event>>);

    #[async_trait]
    impl EventSink for Pushed {
        fn send(&self, event: &Event) -> bool {
            self.0.lock().unwrap().push(event.clone());
            true
        }

        async fn send_replay(&self, event: &Event) -> bool {
            self.send(event)
        }
    }

    impl Pushed {
        fn seqs(&self) -> Vec<u64> {
            self.0.lock().unwrap().iter().map(|e| e.seq).collect()
        }
    }

    struct Rig {
        api: AgentApi,
        engine: Arc<MemoryThreads>,
        spawner: Arc<ScriptedSpawner>,
        executors: Arc<Registry>,
        channel: Arc<ScriptedChannel>,
    }

    fn rig() -> Rig {
        let (engine, spawner, executors) = (Arc::new(MemoryThreads::default()), ScriptedSpawner::new(), Arc::new(Registry::default()));
        let channel = ScriptedChannel::new(|_, _| Ok(ActionOutput { result: json!({}), output: String::new() }));
        executors.open("w1", channel.clone() as Arc<dyn ExecutorChannel>).unwrap();
        let group = AgentThreadsGroup::new(engine.clone(), executors.clone(), spawner.clone(), Arc::new(Mcp), PathBuf::from("/srv/rca"));
        Rig { api: AgentApi::new(vec![Arc::new(group)]).unwrap(), engine, spawner, executors, channel }
    }

    impl Rig {
        async fn ask(&self, method: &str, params: Value) -> Value {
            self.api.dispatch_text(BUILD, &json!({"id": 1, "method": method, "params": params}).to_string()).await
        }

        /// A request on a connection that collects what is pushed; the streams the request opened run, as the transport runs them.
        async fn ask_on(&self, method: &str, params: Value) -> (Value, Arc<Pushed>) {
            let pushed = Arc::new(Pushed::default());
            let connection = Connection::new(pushed.clone());
            let answer = self.api.dispatch_on(BUILD, &json!({"id": 1, "method": method, "params": params}).to_string(), &connection).await;
            for stream in connection.take_following() {
                tokio::spawn(stream.job);
            }
            (answer, pushed)
        }

        async fn create(&self, thread: &str) {
            let made = self.ask("thread.create", json!({"thread": thread, "workspace": "w1", "driver": "claudeAgent", "model": "default-model"})).await;
            assert!(made.get("result").is_some(), "{made}");
        }

        async fn start(&self, thread: &str) -> Value {
            self.ask("turn.start", json!({"thread": thread, "workspace": "w1", "driver": "claudeAgent", "prompt": "make a note"})).await
        }

        async fn wait_idle(&self, thread: &str) {
            for _ in 0..200 {
                if self.engine.get(thread).unwrap().unwrap().status == Status::Idle {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the turn of {thread} never ended: {:?}", self.engine.events_of(thread).iter().map(|e| e.kind.clone()).collect::<Vec<_>>());
        }

        fn kinds(&self, thread: &str) -> Vec<String> {
            self.engine.events_of(thread).iter().map(|e| e.kind.clone()).collect()
        }
    }

    fn said(text: &str) -> String {
        json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}).to_string()
    }

    fn code_of(answer: &Value) -> Value {
        answer["error"]["code"].clone()
    }

    #[tokio::test]
    async fn given_threads_when_created_read_and_listed_then_a_create_for_an_id_that_exists_answers_that_thread_and_the_newest_is_listed_first() {
        // Given a service with no thread
        let rig = rig();
        // When one is created with defaults, one without an id or title, and the first again with another title
        let first = rig.ask("thread.create", json!({"thread": "a", "title": "First", "workspace": "w1", "driver": "claudeAgent", "model": "m"})).await;
        let second = rig.ask("thread.create", json!({})).await;
        let again = rig.ask("thread.create", json!({"thread": "a", "title": "Other"})).await;
        // Then the first has what it was given and is idle, the second was named by the service and titled, and the retry changed nothing
        assert_eq!(first["result"]["thread"]["title"], json!("First"));
        assert_eq!((first["result"]["thread"]["status"].clone(), first["result"]["thread"]["last_seq"].clone()), (json!("idle"), json!(0)));
        assert_eq!((second["result"]["thread"]["id"].clone(), second["result"]["thread"]["title"].clone()), (json!("id-1"), json!("New thread")));
        assert_eq!(again["result"], first["result"]);
        // And get answers the thread, and the list has the newest first
        assert_eq!(rig.ask("thread.get", json!({"thread": "a"})).await["result"], first["result"]);
        let listed = rig.ask("thread.list", json!({})).await;
        assert_eq!(listed["result"]["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect::<Vec<_>>(), ["id-1", "a"]);
    }

    #[tokio::test]
    async fn given_requests_that_cannot_be_answered_when_made_then_each_is_refused_with_its_code_and_a_broken_log_is_unavailable() {
        // Given a thread and a service
        let rig = rig();
        rig.create("t").await;
        // When requests name a thread nobody created, leave out a param, add one, and the engine's log breaks
        let codes = [
            code_of(&rig.ask("thread.get", json!({"thread": "nope"})).await),
            code_of(&rig.ask("thread.subscribe", json!({"thread": "nope"})).await),
            code_of(&rig.ask("thread.history", json!({"thread": "nope"})).await),
            code_of(&rig.ask("thread.transcript", json!({"thread": "nope"})).await),
            code_of(&rig.ask("turn.interrupt", json!({"thread": "nope"})).await),
            code_of(&rig.ask("turn.start", json!({"thread": "nope", "workspace": "w1", "driver": "claudeAgent", "prompt": "p"})).await),
            code_of(&rig.ask("thread.get", json!({})).await),
            code_of(&rig.ask("thread.list", json!({"colour": "red"})).await),
        ];
        // Then the first six are THREAD_NOT_FOUND and the last two INVALID_PARAMS
        assert_eq!(codes.iter().map(|c| c.as_str().unwrap()).collect::<Vec<_>>(), ["THREAD_NOT_FOUND", "THREAD_NOT_FOUND", "THREAD_NOT_FOUND", "THREAD_NOT_FOUND", "THREAD_NOT_FOUND", "THREAD_NOT_FOUND", "INVALID_PARAMS", "INVALID_PARAMS"]);
        rig.engine.break_with("disk full");
        let broken = rig.ask("thread.get", json!({"thread": "t"})).await;
        assert_eq!(broken["error"], json!({"code": "THREADS_UNAVAILABLE", "message": "disk full"}));
    }

    #[tokio::test]
    async fn given_a_turn_that_cannot_run_when_it_is_started_then_it_is_refused_before_any_cli_starts_and_no_event_is_recorded() {
        // Given a thread whose workspace has an executor
        let rig = rig();
        rig.create("t").await;
        let turn = |driver: &str, workspace: &str, extra: Value| {
            let mut params = json!({"thread": "t", "workspace": workspace, "driver": driver, "prompt": "p"});
            params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            params
        };
        // When a turn names a driver nobody runs, Codex (which cannot use the executor yet), a workspace with no executor, and an attachment
        let unknown = rig.ask("turn.start", turn("nobody", "w1", json!({}))).await;
        let codex = rig.ask("turn.start", turn("codex", "w1", json!({}))).await;
        let away = rig.ask("turn.start", turn("claudeAgent", "w9", json!({}))).await;
        let attached = rig.ask("turn.start", turn("claudeAgent", "w1", json!({"attachments": ["a1"]}))).await;
        // Then the codes are UNKNOWN_DRIVER (twice), NO_EXECUTOR and INVALID_PARAMS, with the contract's messages where it has them
        assert_eq!([code_of(&unknown), code_of(&codex), code_of(&away), code_of(&attached)], [json!("UNKNOWN_DRIVER"), json!("UNKNOWN_DRIVER"), json!("NO_EXECUTOR"), json!("INVALID_PARAMS")]);
        assert_eq!(unknown["error"]["message"], json!("no driver named nobody"));
        assert!(codex["error"]["message"].as_str().unwrap().starts_with("no driver named codex: codex cannot use the executor channel"), "{codex}");
        assert_eq!(away["error"]["message"], json!("no executor is attached for workspace w9"));
        // And nothing ran and nothing was recorded
        assert!(rig.spawner.spawned().is_empty());
        assert!(rig.kinds("t").is_empty());
    }

    struct Staging(Mutex<Vec<(String, Vec<String>)>>, Option<Refusal>);

    #[async_trait]
    impl AttachmentStaging for Staging {
        async fn stage(&self, workspace: &str, ids: &[String]) -> Result<Vec<String>, Refusal> {
            self.0.lock().unwrap().push((workspace.to_string(), ids.to_vec()));
            match &self.1 {
                Some(refusal) => Err(refusal.clone()),
                None => Ok(ids.iter().map(|id| format!(".rca/attachments/{id}/f.txt")).collect()),
            }
        }
    }

    fn rig_with_staging(staging: Arc<Staging>) -> Rig {
        let (engine, spawner, executors) = (Arc::new(MemoryThreads::default()), ScriptedSpawner::new(), Arc::new(Registry::default()));
        let channel = ScriptedChannel::new(|_, _| Ok(ActionOutput { result: json!({}), output: String::new() }));
        executors.open("w1", channel.clone() as Arc<dyn ExecutorChannel>).unwrap();
        let group = AgentThreadsGroup::new(engine.clone(), executors.clone(), spawner.clone(), Arc::new(Mcp), PathBuf::from("/srv/rca")).with_staging(staging);
        Rig { api: AgentApi::new(vec![Arc::new(group)]).unwrap(), engine, spawner, executors, channel }
    }

    #[tokio::test]
    async fn given_a_turn_with_attachments_when_started_then_they_are_staged_first_the_cli_is_told_where_and_the_log_keeps_only_what_was_said() {
        // Given a service whose staging answers a path per id, and a thread
        let staging = Arc::new(Staging(Mutex::default(), None));
        let rig = rig_with_staging(staging.clone());
        rig.create("t").await;
        rig.spawner.script("claude", CliScript { lines: vec![said("seen")], ..CliScript::default() });
        // When a turn starts naming two attachments
        let started = rig.ask("turn.start", json!({"thread": "t", "workspace": "w1", "driver": "claudeAgent", "prompt": "look at these", "attachments": ["a1", "a2"]})).await;
        rig.wait_idle("t").await;
        // Then staging was asked once, for that workspace and those ids in order
        assert!(started.get("result").is_some(), "{started}");
        assert_eq!(*staging.0.lock().unwrap(), [("w1".to_string(), vec!["a1".to_string(), "a2".to_string()])]);
        // And the CLI's prompt has what was said and then each path, while the thread records only what was said
        let args = rig.spawner.spawned()[0].args.clone();
        let prompt = args.iter().find(|a| a.starts_with("look at these")).unwrap_or_else(|| panic!("{args:?}"));
        assert!(prompt.ends_with("\n- .rca/attachments/a1/f.txt\n- .rca/attachments/a2/f.txt"), "{prompt}");
        assert_eq!(rig.engine.events_of("t")[1].data["text"], json!("look at these"));
    }

    #[tokio::test]
    async fn given_staging_that_refuses_when_a_turn_with_attachments_starts_then_the_refusal_is_the_answer_no_cli_starts_and_the_thread_is_free_again() {
        // Given staging that refuses with the attachments group's code
        let staging = Arc::new(Staging(Mutex::default(), Some(Refusal::new("ATTACHMENT_NOT_FOUND", "no attachment has the id a1"))));
        let rig = rig_with_staging(staging);
        rig.create("t").await;
        // When a turn names an attachment
        let refused = rig.ask("turn.start", json!({"thread": "t", "workspace": "w1", "driver": "claudeAgent", "prompt": "p", "attachments": ["a1"]})).await;
        // Then the answer is that refusal, nothing ran or was recorded, and a turn without attachments still starts on the thread
        assert_eq!(refused["error"], json!({"code": "ATTACHMENT_NOT_FOUND", "message": "no attachment has the id a1"}));
        assert!(rig.spawner.spawned().is_empty() && rig.kinds("t").is_empty());
        assert!(rig.start("t").await.get("result").is_some());
    }

    #[tokio::test]
    async fn given_a_cli_that_writes_a_file_through_its_tool_when_a_turn_runs_then_the_thread_records_its_events_in_order_and_is_idle_again() {
        // Given a CLI that says it will write, calls the executor's write_file and sees it succeed, then says it is done
        let rig = rig();
        rig.create("t").await;
        let call = json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "k1", "name": "mcp__executor__write_file", "input": {"path": "notes/a.txt", "content": "hi"}}]}});
        let result = json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "k1", "content": "wrote 2 bytes", "is_error": false}]}});
        rig.spawner.script("claude", CliScript { lines: vec![said("writing"), call.to_string(), result.to_string(), said("done")], ..CliScript::default() });
        // When a turn starts on the thread
        let started = rig.start("t").await;
        rig.wait_idle("t").await;
        // Then the answer names the turn, and the log has the turn's start, the prompt, the CLI's events and the end, each of the turn, in seq order
        let turn = started["result"]["turn"].as_str().unwrap().to_string();
        assert_eq!(started["result"]["thread"], json!("t"));
        assert_eq!(rig.kinds("t"), ["turn.started", "message.delta", "message.delta", "tool.started", "tool.output", "tool.finished", "file.changed", "message.delta", "turn.finished"]);
        let events = rig.engine.events_of("t");
        assert!(events.iter().all(|e| e.turn.as_deref() == Some(turn.as_str())));
        assert!(events.windows(2).all(|pair| pair[0].seq < pair[1].seq));
        assert_eq!((events[1].data["role"].clone(), events[1].data["text"].clone()), (json!("user"), json!("make a note")));
        assert_eq!((events[6].data["path"].clone(), events[6].data["op"].clone()), (json!("notes/a.txt"), json!("write")));
        assert_eq!(events[8].data["reason"], json!("completed"));
        // And the CLI was started in the service's directory with the thread's own tool server and the thread's default model
        let spawned = rig.spawner.spawned();
        assert_eq!((spawned.len(), spawned[0].cwd.clone()), (1, PathBuf::from("/srv/rca")));
        let args = &spawned[0].args;
        assert!(args.iter().any(|a| a.contains("http://127.0.0.1:1/mcp/t")) && args.windows(2).any(|w| w == ["--model", "default-model"]), "{args:?}");
        // And the thread is no longer bound to the executor, so its tool server resolves to none
        assert!(rig.executors.for_thread("t").is_none());
        // And the transcript is the rows a person reads
        let transcript = rig.ask("thread.transcript", json!({"thread": "t"})).await;
        let kinds: Vec<&str> = transcript["result"]["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["message", "message", "tool", "file", "message"]);
    }

    #[tokio::test]
    async fn given_a_running_turn_when_another_starts_or_the_turn_is_interrupted_then_the_second_is_refused_and_only_the_running_turn_can_be_stopped() {
        // Given a CLI that stays up after its first line
        let rig = rig();
        rig.create("t").await;
        rig.spawner.script("claude", CliScript { lines: vec![said("working")], hold: true, ..CliScript::default() });
        let turn = rig.start("t").await["result"]["turn"].as_str().unwrap().to_string();
        // When a second turn starts, and an interrupt names another turn, then no turn, on this thread, and one on a thread that is idle
        rig.create("idle").await;
        let busy = rig.start("t").await;
        let wrong = rig.ask("turn.interrupt", json!({"thread": "t", "turn": "turn-0"})).await;
        let idle = rig.ask("turn.interrupt", json!({"thread": "idle"})).await;
        // Then the second turn is TURN_RUNNING and the running one was not touched, the others are NO_TURN with their messages
        assert_eq!(busy["error"], json!({"code": "TURN_RUNNING", "message": "thread t already has a running turn"}));
        assert_eq!(wrong["error"], json!({"code": "NO_TURN", "message": "thread t is not running turn turn-0"}));
        assert_eq!(idle["error"], json!({"code": "NO_TURN", "message": "thread idle has no running turn"}));
        assert_eq!(rig.spawner.kills(), 0);
        assert_eq!(rig.engine.get("t").unwrap().unwrap().turn.as_deref(), Some(turn.as_str()));
        // When the running turn is interrupted
        let stopped = rig.ask("turn.interrupt", json!({"thread": "t"})).await;
        rig.wait_idle("t").await;
        // Then the answer names it, the CLI was killed, the turn ended as interrupted, and the thread runs a new turn
        assert_eq!(stopped["result"], json!({"thread": "t", "turn": turn}));
        assert_eq!(rig.spawner.kills(), 1);
        assert_eq!(rig.engine.events_of("t").last().unwrap().data["reason"], json!("interrupted"));
        assert!(rig.start("t").await.get("result").is_some());
    }

    #[tokio::test]
    async fn given_an_executor_that_goes_away_mid_turn_when_its_channel_closes_then_the_cli_is_stopped_and_the_turn_ends_failed_with_the_executor_gone() {
        // Given a CLI that stays up
        let rig = rig();
        rig.create("t").await;
        rig.spawner.script("claude", CliScript { lines: vec![said("working")], hold: true, ..CliScript::default() });
        rig.start("t").await;
        // When the workspace's executor closes its channel
        rig.channel.close(1006);
        rig.wait_idle("t").await;
        // Then the CLI was killed and the log ends with the error and a failed finish
        assert_eq!(rig.spawner.kills(), 1);
        let events = rig.engine.events_of("t");
        let (error, finished) = (&events[events.len() - 2], &events[events.len() - 1]);
        assert_eq!((error.kind.as_str(), error.data["code"].clone()), ("error", json!("EXECUTOR_GONE")));
        assert_eq!(finished.data["reason"], json!("failed"));
    }

    #[tokio::test]
    async fn given_a_cli_that_cannot_start_or_fails_when_a_turn_runs_then_the_turn_is_started_and_ends_failed_with_the_reason_and_the_thread_is_free() {
        // Given Claude Code that is not installed, and Opencode that exits with a failure
        let rig = rig();
        rig.create("t").await;
        rig.spawner.script("claude", CliScript { not_started: Some("not found".into()), ..CliScript::default() });
        // opencode's tools are configured through its environment, so the service starts it as `env OPENCODE_CONFIG_CONTENT=... opencode ...`
        rig.spawner.script("env", CliScript { exit: Some("exited with 1: not logged in".into()), ..CliScript::default() });
        // When a turn starts for each
        let missing = rig.start("t").await;
        let failing = rig.ask("turn.start", json!({"thread": "t", "workspace": "w1", "driver": "opencode", "prompt": "p"})).await;
        rig.wait_idle("t").await;
        // Then both answered with a turn, and the log has the start, the error and a failed finish of each
        assert!(missing["result"]["turn"].is_string() && failing["result"]["turn"].is_string());
        let events = rig.engine.events_of("t");
        let errors: Vec<(Value, Value)> = events.iter().filter(|e| e.kind == "error").map(|e| (e.data["code"].clone(), e.data["message"].clone())).collect();
        assert_eq!(errors, [(json!("CLI_START_FAILED"), json!("claude could not start: not found")), (json!("CLI_ERROR"), json!("exited with 1: not logged in"))]);
        assert_eq!(events.iter().filter(|e| e.kind == "turn.finished").map(|e| e.data["reason"].clone()).collect::<Vec<_>>(), [json!("failed"), json!("failed")]);
    }

    #[tokio::test]
    async fn given_events_recorded_before_and_after_a_subscribe_when_a_client_subscribes_twice_then_it_gets_each_event_once_in_order_and_the_resume_has_no_gap() {
        // Given a thread with a finished turn
        let rig = rig();
        rig.create("t").await;
        rig.spawner.script("claude", CliScript { lines: vec![said("one"), said("two")], ..CliScript::default() });
        rig.start("t").await;
        rig.wait_idle("t").await;
        let all = rig.engine.events_of("t").iter().map(|e| e.seq).collect::<Vec<_>>();
        // When a client subscribes from the start, and then a new turn adds events
        let (answer, first) = rig.ask_on("thread.subscribe", json!({"thread": "t"})).await;
        rig.spawner.script("claude", CliScript { lines: vec![said("three")], ..CliScript::default() });
        rig.start("t").await;
        rig.wait_idle("t").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Then the answer names the stream and where the replay ends, and the client got the replay and then the live events, once each, in order
        assert_eq!(answer["result"], json!({"topic": "thread/t", "after_seq": 0, "last_seq": all.last().unwrap()}));
        let everything: Vec<u64> = rig.engine.events_of("t").iter().map(|e| e.seq).collect();
        assert_eq!(first.seqs(), everything);
        // When it comes back on a new connection from the seq it last saw before the third turn
        let seen = all[all.len() - 1];
        let (resumed, second) = rig.ask_on("thread.subscribe", json!({"thread": "t", "after_seq": seen})).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Then it gets exactly the events after it: no loss and no duplicate
        assert_eq!(second.seqs(), everything.iter().copied().filter(|s| *s > seen).collect::<Vec<_>>());
        assert_eq!(resumed["result"]["last_seq"], json!(everything.last().unwrap()));
        // And a client already at the newest gets none from the replay and the live events only
        let (caught_up, third) = rig.ask_on("thread.subscribe", json!({"thread": "t", "after_seq": everything.last().unwrap()})).await;
        assert_eq!((caught_up["result"]["after_seq"].clone(), caught_up["result"]["last_seq"].clone()), (json!(everything.last().unwrap()), json!(everything.last().unwrap())));
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(third.seqs().is_empty());
    }

    #[tokio::test]
    async fn given_a_thread_with_many_events_when_history_is_paged_then_each_page_continues_where_the_last_ended_and_the_last_says_no_more() {
        // Given a thread with a turn of five CLI lines
        let rig = rig();
        rig.create("t").await;
        rig.spawner.script("claude", CliScript { lines: (1..=5).map(|n| said(&format!("line {n}"))).collect(), ..CliScript::default() });
        rig.start("t").await;
        rig.wait_idle("t").await;
        // When the history is read two events at a time, each after the last seq of the page before
        let mut after = 0;
        let mut seen = Vec::new();
        let mut pages = Vec::new();
        loop {
            let page = rig.ask("thread.history", json!({"thread": "t", "after_seq": after, "limit": 2})).await;
            let events = page["result"]["events"].as_array().unwrap().clone();
            seen.extend(events.iter().map(|e| e["seq"].as_u64().unwrap()));
            pages.push((events.len(), page["result"]["more"].as_bool().unwrap()));
            match events.last() {
                Some(last) if page["result"]["more"] == json!(true) => after = last["seq"].as_u64().unwrap(),
                _ => break,
            }
        }
        // Then there were four pages (the 8 events: start, prompt, five lines, finish), the last without more, and every event came once in order
        assert_eq!(pages, [(2, true), (2, true), (2, true), (2, false)]);
        assert_eq!(seen, rig.engine.events_of("t").iter().map(|e| e.seq).collect::<Vec<_>>());
    }
}
