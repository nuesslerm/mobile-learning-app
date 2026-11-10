//! The pure core of `rca-exec`: the executor port with the actions every model uses (`docs/plans/rca-client/protocol.md`), and the one function
//! that maps a wire request `{tool, args}` onto it. The arguments and results are the contract crates' (`libraries/agent-api`). No I/O;
//! paths are relative to the executor's root.
use exec_fs::{Browse, BrowseResult, ChunkResult, CreateFile, EditFile, EditResult, EntriesResult, Entry, Found, ListDir, ListResult, MakeDir, MakeDirResult, PutBlob, PutBlobResult, ReadChunk, ReadFile, ReadResult, Search, SearchEntries, SkillScan, SkillsFound, Stat, StatResult, WriteFile, WriteResult};
use exec_hosted::HostedOp;
use exec_preview::PreviewOp;
use exec_git::{DiffResult, GitDiff, GitOp, SetupEvent, SetupResult, WorktreeAdd, WorktreeAdded, WorktreeInfo, WorktreeListed, WorktreeRemove, WorktreeRemoved, WorktreeSetup};
use exec_proc::{Cancel, CancelResult, CliDetect, CliDetectResult, CliFound, RunCommand, RunResult};
use exec_pty::{PtyAttach, PtyAttached, PtyClose, PtyClosed, PtyEvent, PtyInfo, PtyListed, PtyOpen, PtyOpened, PtyResize, PtyResized, PtyWrite, PtyWritten};
use exec_proto::{to_value, Code, ExecError, ExecResult, Stream};
use serde_json::Value;

/// Performs the actions in one directory. `id` names a `run_command` so `cancel` can reach it from another thread.
pub trait ToolExecutor: Send + Sync {
    /// Blocks until the process ends, calling `output` for each chunk as it arrives.
    fn run_command(&self, id: &str, args: &RunCommand, output: &mut dyn FnMut(Stream, &str)) -> ExecResult<RunResult>;
    fn read_file(&self, args: &ReadFile) -> ExecResult<ReadResult>;
    /// Returns the bytes written.
    fn write_file(&self, args: &WriteFile) -> ExecResult<usize>;
    /// Returns the replacements made.
    fn edit_file(&self, args: &EditFile) -> ExecResult<usize>;
    fn list_dir(&self, args: &ListDir) -> ExecResult<Vec<Entry>>;
    fn search(&self, args: &Search) -> ExecResult<Found>;
    /// Kills the process group of the running `run_command` `id`; false when none is running under that id.
    fn cancel(&self, id: &str) -> bool;
    /// The working tree's `git diff` against `base` (the index when none), limited to `path` when given.
    fn git_diff(&self, args: &GitDiff) -> ExecResult<String>;
    fn stat(&self, args: &Stat) -> ExecResult<StatResult>;
    fn read_chunk(&self, args: &ReadChunk) -> ExecResult<ChunkResult>;
    /// Returns the bytes written.
    fn create_file(&self, args: &CreateFile) -> ExecResult<usize>;
    /// Writes the decoded bytes to a path that does not exist yet; returns the bytes written.
    fn put_blob(&self, args: &PutBlob) -> ExecResult<u64>;
    /// Makes the directory, and its parents when asked; false when it was there already (only `parents` accepts that).
    fn make_dir(&self, args: &MakeDir) -> ExecResult<bool>;
    fn browse(&self, args: &Browse) -> ExecResult<BrowseResult>;
    /// The agent CLIs named, in order, with where each lies on the executor's path and what its `--version` prints.
    fn cli_detect(&self, args: &CliDetect) -> ExecResult<Vec<CliFound>>;
    /// The skills of a directory of the workspace and, when asked, of the executor's own home.
    fn skill_scan(&self, args: &SkillScan) -> ExecResult<SkillsFound>;
    fn search_entries(&self, args: &SearchEntries) -> ExecResult<EntriesResult>;
    fn git_worktree_add(&self, args: &WorktreeAdd) -> ExecResult<WorktreeAdded>;
    /// True when the worktree was removed.
    fn git_worktree_remove(&self, args: &WorktreeRemove) -> ExecResult<bool>;
    fn git_worktree_list(&self) -> ExecResult<Vec<WorktreeInfo>>;
    /// Runs the commands in the worktree, calling `events` for each progress event; `id` lets `cancel` stop it.
    fn worktree_setup(&self, id: &str, args: &WorktreeSetup, events: &mut dyn FnMut(SetupEvent)) -> ExecResult<SetupResult>;
    fn pty_open(&self, args: &PtyOpen) -> ExecResult<PtyOpened>;
    /// Returns the bytes written.
    fn pty_write(&self, args: &PtyWrite) -> ExecResult<usize>;
    fn pty_resize(&self, args: &PtyResize) -> ExecResult<()>;
    fn pty_close(&self, args: &PtyClose) -> ExecResult<PtyClosed>;
    fn pty_list(&self) -> ExecResult<Vec<PtyInfo>>;
    /// Blocks until the pty ends or `cancel(id)` is called, calling `output(offset, bytes)` for each chunk and `events` for the attach event.
    fn pty_attach(&self, id: &str, args: &PtyAttach, output: &mut dyn FnMut(u64, &[u8]), events: &mut dyn FnMut(PtyEvent)) -> ExecResult<PtyAttached>;
    /// One repository action (`git_status` .. `git_watch`) and its result as the wire carries it; `git_watch` calls `events` and runs until
    /// `cancel(id)`.
    fn git_op(&self, id: &str, op: &GitOp, events: &mut dyn FnMut(Value)) -> ExecResult<Value>;
    /// One hosted source control action (`hosted_*`) and its result as the wire carries it; `hosted_repo_clone` and `hosted_auth_login_wait`
    /// block until the CLI ends or `cancel(id)`.
    fn hosted_op(&self, id: &str, op: &HostedOp) -> ExecResult<Value>;
    /// One preview action (`preview_start`, `preview_stop`, `preview_list`) and its result as the wire carries it; `preview_start` returns once
    /// the server accepts connections.
    fn preview_op(&self, op: &PreviewOp) -> ExecResult<Value>;
}

/// Runs one wire request on the executor and returns its `result` object. `stream` receives `run_command`'s output frames, `events` the
/// events of the actions that report progress (`worktree_setup`, `pty_attach`), as the `event` of a subscribe frame.
pub fn execute(ex: &dyn ToolExecutor, id: &str, tool: &str, args: &Value, stream: &mut dyn FnMut(Stream, &str), events: &mut dyn FnMut(Value)) -> ExecResult<Value> {
    execute_binary(ex, id, tool, args, stream, events, &mut |_| {})
}

/// `execute` with a sink for the binary frames (`exec_pty::encode_frame`) of `pty_attach`.
pub fn execute_binary(ex: &dyn ToolExecutor, id: &str, tool: &str, args: &Value, stream: &mut dyn FnMut(Stream, &str), events: &mut dyn FnMut(Value), binary: &mut dyn FnMut(Vec<u8>)) -> ExecResult<Value> {
    match tool {
        exec_proc::RUN_COMMAND => Ok(to_value(&ex.run_command(id, &RunCommand::parse(args)?, stream)?)),
        exec_fs::READ_FILE => Ok(to_value(&ex.read_file(&ReadFile::parse(args)?)?)),
        exec_fs::WRITE_FILE => Ok(to_value(&WriteResult { bytes: ex.write_file(&WriteFile::parse(args)?)? })),
        exec_fs::EDIT_FILE => Ok(to_value(&EditResult { replacements: ex.edit_file(&EditFile::parse(args)?)? })),
        exec_fs::LIST_DIR => Ok(to_value(&ListResult { entries: ex.list_dir(&ListDir::parse(args)?)? })),
        exec_fs::SEARCH => Ok(to_value(&ex.search(&Search::parse(args)?)?)),
        exec_proc::CANCEL => Ok(to_value(&CancelResult { cancelled: Cancel::parse(args).id.is_some_and(|target| ex.cancel(&target)) })),
        exec_git::GIT_DIFF => Ok(to_value(&DiffResult { diff: ex.git_diff(&GitDiff::parse(args)?)? })),
        exec_fs::STAT => Ok(to_value(&ex.stat(&Stat::parse(args)?)?)),
        exec_fs::READ_CHUNK => Ok(to_value(&ex.read_chunk(&ReadChunk::parse(args)?)?)),
        exec_fs::CREATE_FILE => Ok(to_value(&WriteResult { bytes: ex.create_file(&CreateFile::parse(args)?)? })),
        exec_fs::PUT_BLOB => {
            let blob = PutBlob::parse(args)?;
            Ok(to_value(&PutBlobResult { bytes: ex.put_blob(&blob)?, path: blob.path }))
        }
        exec_fs::MAKE_DIR => {
            let dir = MakeDir::parse(args)?;
            Ok(to_value(&MakeDirResult { created: ex.make_dir(&dir)?, path: dir.path }))
        }
        exec_fs::BROWSE => Ok(to_value(&ex.browse(&Browse::parse(args)?)?)),
        exec_proc::CLI_DETECT => Ok(to_value(&CliDetectResult { clis: ex.cli_detect(&CliDetect::parse(args)?)? })),
        exec_fs::SKILL_SCAN => Ok(to_value(&ex.skill_scan(&SkillScan::parse(args)?)?)),
        exec_fs::SEARCH_ENTRIES => Ok(to_value(&ex.search_entries(&SearchEntries::parse(args)?)?)),
        exec_git::GIT_WORKTREE_ADD => Ok(to_value(&ex.git_worktree_add(&WorktreeAdd::parse(args)?)?)),
        exec_git::GIT_WORKTREE_REMOVE => Ok(to_value(&WorktreeRemoved { removed: ex.git_worktree_remove(&WorktreeRemove::parse(args)?)? })),
        exec_git::GIT_WORKTREE_LIST => Ok(to_value(&WorktreeListed { worktrees: ex.git_worktree_list()? })),
        exec_git::WORKTREE_SETUP => Ok(to_value(&ex.worktree_setup(id, &WorktreeSetup::parse(args)?, &mut |e| events(to_value(&e)))?)),
        exec_pty::PTY_OPEN => Ok(to_value(&ex.pty_open(&PtyOpen::parse(args)?)?)),
        exec_pty::PTY_WRITE => Ok(to_value(&PtyWritten { bytes: ex.pty_write(&PtyWrite::parse(args)?)? })),
        exec_pty::PTY_RESIZE => {
            let size = PtyResize::parse(args)?;
            ex.pty_resize(&size)?;
            Ok(to_value(&PtyResized { cols: size.cols, rows: size.rows }))
        }
        exec_pty::PTY_CLOSE => Ok(to_value(&ex.pty_close(&PtyClose::parse(args)?)?)),
        exec_pty::PTY_LIST => Ok(to_value(&PtyListed { ptys: ex.pty_list()? })),
        exec_pty::PTY_ATTACH => Ok(to_value(&ex.pty_attach(id, &PtyAttach::parse(args)?, &mut |offset, bytes| binary(exec_pty::encode_frame(id, offset, bytes)), &mut |e| events(to_value(&e)))?)),
        other => match (GitOp::parse(other, args), HostedOp::parse(other, args), PreviewOp::parse(other, args)) {
            (Some(op), _, _) => ex.git_op(id, &op?, events),
            (None, Some(op), _) => ex.hosted_op(id, &op?),
            (None, None, Some(op)) => ex.preview_op(&op?),
            (None, None, None) => Err(ExecError::new(Code::UnknownTool, format!("unknown tool {other}"))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    /// Records the action each call reached and answers with fixed values.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl Recorder {
        fn note(&self, action: &str) {
            self.0.lock().unwrap().push(action.to_string());
        }

        fn reached(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    impl ToolExecutor for Recorder {
        fn run_command(&self, id: &str, _: &RunCommand, output: &mut dyn FnMut(Stream, &str)) -> ExecResult<RunResult> {
            self.note(exec_proc::RUN_COMMAND);
            output(Stream::Stdout, id);
            Ok(RunResult { exit_code: 0, timed_out: false, cancelled: false, truncated: false })
        }
        fn read_file(&self, _: &ReadFile) -> ExecResult<ReadResult> {
            self.note(exec_fs::READ_FILE);
            Ok(ReadResult { content: String::new(), total_lines: 0, truncated: false })
        }
        fn write_file(&self, _: &WriteFile) -> ExecResult<usize> {
            self.note(exec_fs::WRITE_FILE);
            Ok(0)
        }
        fn edit_file(&self, _: &EditFile) -> ExecResult<usize> {
            self.note(exec_fs::EDIT_FILE);
            Ok(0)
        }
        fn list_dir(&self, _: &ListDir) -> ExecResult<Vec<Entry>> {
            self.note(exec_fs::LIST_DIR);
            Ok(Vec::new())
        }
        fn search(&self, _: &Search) -> ExecResult<Found> {
            self.note(exec_fs::SEARCH);
            Ok(Found { matches: Vec::new(), truncated: false })
        }
        fn cancel(&self, id: &str) -> bool {
            self.note(exec_proc::CANCEL);
            id == "running"
        }
        fn git_diff(&self, _: &GitDiff) -> ExecResult<String> {
            self.note(exec_git::GIT_DIFF);
            Ok(String::new())
        }
        fn stat(&self, _: &Stat) -> ExecResult<StatResult> {
            self.note(exec_fs::STAT);
            Ok(StatResult { kind: exec_fs::EntryKind::File, size: 0, modified_ms: 0 })
        }
        fn read_chunk(&self, _: &ReadChunk) -> ExecResult<ChunkResult> {
            self.note(exec_fs::READ_CHUNK);
            Ok(ChunkResult { data: String::new(), offset: 0, bytes: 0, total_bytes: 0, eof: true })
        }
        fn put_blob(&self, _: &PutBlob) -> ExecResult<u64> {
            self.note(exec_fs::PUT_BLOB);
            Ok(0)
        }
        fn create_file(&self, _: &CreateFile) -> ExecResult<usize> {
            self.note(exec_fs::CREATE_FILE);
            Ok(0)
        }
        fn make_dir(&self, _: &MakeDir) -> ExecResult<bool> {
            self.note(exec_fs::MAKE_DIR);
            Ok(true)
        }
        fn cli_detect(&self, _: &CliDetect) -> ExecResult<Vec<CliFound>> {
            self.note(exec_proc::CLI_DETECT);
            Ok(Vec::new())
        }
        fn skill_scan(&self, _: &SkillScan) -> ExecResult<SkillsFound> {
            self.note(exec_fs::SKILL_SCAN);
            Ok(SkillsFound { skills: Vec::new(), truncated: false })
        }
        fn browse(&self, _: &Browse) -> ExecResult<BrowseResult> {
            self.note(exec_fs::BROWSE);
            Ok(BrowseResult { path: "/".into(), parent: None, entries: Vec::new() })
        }
        fn search_entries(&self, _: &SearchEntries) -> ExecResult<EntriesResult> {
            self.note(exec_fs::SEARCH_ENTRIES);
            Ok(EntriesResult { entries: Vec::new(), truncated: false })
        }
        fn git_worktree_add(&self, _: &WorktreeAdd) -> ExecResult<WorktreeAdded> {
            self.note(exec_git::GIT_WORKTREE_ADD);
            Ok(WorktreeAdded { path: "/w".into(), branch: "b".into(), head: "h".into() })
        }
        fn git_worktree_remove(&self, _: &WorktreeRemove) -> ExecResult<bool> {
            self.note(exec_git::GIT_WORKTREE_REMOVE);
            Ok(true)
        }
        fn git_worktree_list(&self) -> ExecResult<Vec<WorktreeInfo>> {
            self.note(exec_git::GIT_WORKTREE_LIST);
            Ok(Vec::new())
        }
        fn worktree_setup(&self, id: &str, _: &WorktreeSetup, events: &mut dyn FnMut(SetupEvent)) -> ExecResult<SetupResult> {
            self.note(exec_git::WORKTREE_SETUP);
            events(SetupEvent::Started { steps: id.len() });
            Ok(SetupResult { ok: true, steps: Vec::new(), timed_out: false, cancelled: false })
        }
        fn pty_open(&self, args: &PtyOpen) -> ExecResult<PtyOpened> {
            self.note(exec_pty::PTY_OPEN);
            Ok(PtyOpened { pty: args.pty.clone(), pid: 1, cwd: "/w".into(), cols: args.cols, rows: args.rows })
        }
        fn pty_write(&self, args: &PtyWrite) -> ExecResult<usize> {
            self.note(exec_pty::PTY_WRITE);
            Ok(args.data.len())
        }
        fn pty_resize(&self, _: &PtyResize) -> ExecResult<()> {
            self.note(exec_pty::PTY_RESIZE);
            Ok(())
        }
        fn pty_close(&self, _: &PtyClose) -> ExecResult<PtyClosed> {
            self.note(exec_pty::PTY_CLOSE);
            Ok(PtyClosed { closed: true, exit_code: None })
        }
        fn pty_list(&self) -> ExecResult<Vec<PtyInfo>> {
            self.note(exec_pty::PTY_LIST);
            Ok(Vec::new())
        }
        fn pty_attach(&self, id: &str, args: &PtyAttach, output: &mut dyn FnMut(u64, &[u8]), events: &mut dyn FnMut(PtyEvent)) -> ExecResult<PtyAttached> {
            self.note(exec_pty::PTY_ATTACH);
            events(PtyEvent::Attached { pty: args.pty.clone(), requested_offset: 0, start_offset: 0, reset: false, offset: 0 });
            output(0, id.as_bytes());
            Ok(PtyAttached { pty: args.pty.clone(), offset: id.len() as u64, exited: false, exit_code: None, detached: true })
        }
        fn git_op(&self, _: &str, op: &GitOp, _: &mut dyn FnMut(Value)) -> ExecResult<Value> {
            self.note(op.action());
            Ok(json!({}))
        }
        fn hosted_op(&self, _: &str, op: &HostedOp) -> ExecResult<Value> {
            self.note(op.call.action());
            Ok(json!({}))
        }
        fn preview_op(&self, op: &PreviewOp) -> ExecResult<Value> {
            self.note(match op {
                PreviewOp::Start(_) => exec_preview::PREVIEW_START,
                PreviewOp::Stop(_) => exec_preview::PREVIEW_STOP,
                PreviewOp::List => exec_preview::PREVIEW_LIST,
            });
            Ok(json!({}))
        }
    }

    fn actions() -> Vec<&'static exec_proto::Action> {
        [exec_fs::ACTIONS, exec_proc::ACTIONS, exec_git::ACTIONS, exec_pty::ACTIONS, exec_hosted::ACTIONS, exec_preview::ACTIONS].into_iter().flatten().collect()
    }

    /// A value of the kind the action's table gives the property (a search kind is "content"), so the minimal request parses.
    fn placeholder(action: &exec_proto::Action, key: &str) -> Value {
        match (key, action.properties.iter().find(|p| p.name == key).map(|p| p.kind)) {
            ("kind", _) => json!("content"),
            ("programs", _) => json!(["claude"]),
            ("op", _) => json!("abort"),
            ("resolution", _) => json!("resolved"),
            ("provider", _) => json!("github"),
            ("repo", _) => json!("o/r"),
            ("port", _) => json!(4000),
            ("verdict", _) => json!("approve"),
            ("content", _) => json!("+1"),
            (_, Some("array")) => json!(["v"]),
            (_, Some("integer")) => json!(1),
            _ => json!("v"),
        }
    }

    fn run(ex: &Recorder, tool: &str, args: Value) -> ExecResult<Value> {
        execute(ex, "id-1", tool, &args, &mut |_, _| {}, &mut |_| {})
    }

    #[test]
    fn given_every_action_of_the_contract_when_a_request_names_it_then_the_executor_is_asked_for_that_action_and_no_other() {
        for action in actions() {
            // Given a request with the arguments the action requires
            let mut args: serde_json::Map<String, Value> = action.required.iter().map(|k| (k.to_string(), placeholder(action, k))).collect();
            // Two hosted actions need one of several optional arguments
            match action.name {
                exec_hosted::PR_EDIT => args.insert("title".into(), json!("t")),
                exec_hosted::PR_LABELS | exec_hosted::PR_REQUEST_REVIEWERS => args.insert("add".into(), json!(["a"])),
                _ => None,
            };
            let ex = Recorder::default();
            // When it is executed
            let result = run(&ex, action.name, Value::Object(args));
            // Then the executor was asked for exactly that action, and the request was answered
            assert!(result.is_ok(), "{}: {result:?}", action.name);
            assert_eq!(ex.reached(), [action.name], "{}", action.name);
        }
    }

    #[test]
    fn given_a_tool_the_contract_does_not_name_or_arguments_it_refuses_when_executed_then_the_executor_is_not_asked() {
        // Given a recording executor
        let ex = Recorder::default();
        // When a tool that is no action, then an action with an argument of the wrong kind, is executed
        let unknown = run(&ex, "format_disk", json!({})).unwrap_err();
        let bad = run(&ex, exec_fs::WRITE_FILE, json!({"path": 5})).unwrap_err();
        // Then each is refused with its code and the executor was asked for nothing
        assert_eq!((unknown.code, unknown.message.as_str()), (Code::UnknownTool, "unknown tool format_disk"));
        assert_eq!(bad.code, Code::InvalidArgs);
        assert!(ex.reached().is_empty());
    }

    #[test]
    fn given_a_cancel_when_it_names_a_running_action_or_none_then_it_answers_and_never_fails() {
        // Given an executor that is running one action
        let ex = Recorder::default();
        // When a cancel names it, one names another, and one names nothing
        let hit = run(&ex, exec_proc::CANCEL, json!({"id": "running"}));
        let miss = run(&ex, exec_proc::CANCEL, json!({"id": "other"}));
        let none = run(&ex, exec_proc::CANCEL, json!({}));
        // Then the first is cancelled, the others are answered as found nothing, and a cancel without an id never reached the executor
        assert_eq!((hit, miss, none), (Ok(json!({"cancelled": true})), Ok(json!({"cancelled": false})), Ok(json!({"cancelled": false}))));
        assert_eq!(ex.reached(), [exec_proc::CANCEL, exec_proc::CANCEL]);
    }

    #[test]
    fn given_a_command_that_prints_when_executed_then_its_output_goes_to_the_stream_and_its_result_has_the_contract_fields() {
        // Given a command run under an id
        let ex = Recorder::default();
        let mut seen = Vec::new();
        // When it is executed
        let result = execute(&ex, "job-9", exec_proc::RUN_COMMAND, &json!({"command": "x"}), &mut |s, data| seen.push((s, data.to_string())), &mut |_| {}).unwrap();
        // Then the executor's output reached the stream and the result is the run's four fields
        assert_eq!(seen, [(Stream::Stdout, "job-9".to_string())]);
        assert_eq!(result, json!({"exit_code": 0, "timed_out": false, "cancelled": false, "truncated": false}));
    }
    #[test]
    fn given_a_worktree_setup_when_executed_then_the_actions_events_reach_the_events_sink_as_json_and_not_the_output_stream() {
        // Given a setup request under an id and two sinks
        let ex = Recorder::default();
        let (mut out, mut events) = (Vec::new(), Vec::new());
        // When it is executed
        let result = execute(&ex, "abc", exec_git::WORKTREE_SETUP, &json!({"name": "w", "commands": ["x"]}), &mut |s, d| out.push((s, d.to_string())), &mut |e| events.push(e)).unwrap();
        // Then the event is the contract's JSON, the output stream saw nothing, and the result has the four fields
        assert_eq!((out.len(), events), (0, vec![json!({"kind": "setup.started", "steps": 3})]));
        assert_eq!(result, json!({"ok": true, "steps": [], "timed_out": false, "cancelled": false}));
    }

    #[test]
    fn given_an_attach_when_executed_then_output_arrives_as_binary_frames_of_its_id_and_the_event_as_json() {
        // Given an attach request under an id and the three sinks
        let ex = Recorder::default();
        let (mut frames, mut events) = (Vec::new(), Vec::new());
        // When it is executed
        let result = execute_binary(&ex, "att-1", exec_pty::PTY_ATTACH, &json!({"pty": "t"}), &mut |_, _| {}, &mut |e| events.push(e), &mut |f| frames.push(f)).unwrap();
        // Then the frame decodes to the id and offset, the event is the contract's JSON, and the result has the contract fields
        assert_eq!(frames.iter().map(|f| exec_pty::decode_frame(f).map(|(id, off, data)| (id, off, data.to_vec()))).collect::<Vec<_>>(), [Some(("att-1".to_string(), 0, b"att-1".to_vec()))]);
        assert_eq!(events, [json!({"kind": "pty.attached", "pty": "t", "requested_offset": 0, "start_offset": 0, "reset": false, "offset": 0})]);
        assert_eq!(result, json!({"pty": "t", "offset": 5, "exited": false, "exit_code": null, "detached": true}));
    }
}
