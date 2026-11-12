//! What runs inside agent-shell: detached script jobs, their status and cancel, the two ledgers, an artifact and the credential files.
//! The app container is looked up for each call: one that is not running (exited, created) is 503, the same as one that is not listed.
use crate::application::ports::{Clock, Containers, ContainersError, EventLog, ExecOptions, Event, Ids};
use crate::application::scripts::agent_shell_job::{agent_shell_cancel_argv, agent_shell_job_argv, JOB_CANCEL_GRACE_SEC};
use crate::domain::agent_shell::{
    self as shell, JobStatus, AGENT_SHELL_HOME, AGENT_SHELL_UID, EXEC_SCRIPT_BACKSTOP_SEC, JOBS_CLEANUP, JOB_STATUS_SCRIPT,
};
use crate::domain::auth::lenient_base64;
use crate::domain::containers::pick_app_container;
use crate::domain::js::JsString;
use crate::domain::json_body::{parse_record, string_or_null};
use crate::domain::tar::{build_single_file_tar, SingleFile};
use crate::domain::validators::is_valid_job_id;
use std::sync::Arc;

const APP: &str = "agent-shell";
const ARCHIVE_BODY_UNITS: usize = 500;

/// Why a call has no result: the request is refused with its own reply, or the Engine failed, which is a 502 `request failed`.
#[derive(Debug, Clone, PartialEq)]
pub enum Failed {
    /// agent-shell is not among the listed containers.
    NotRunning,
    EmptyScript,
    BadJobId,
    UnknownLedger,
    LedgerNotFound,
    BadArtifactPath,
    ArtifactNotFound(String),
    ArtifactEscapes,
    NotJson,
    UnknownTool,
    EmptyContent,
    InvalidBase64,
    MkdirFailed { exit_code: Option<i64>, output: String },
    /// The Engine answered the archive put with something but 200; `body` is its first 500 UTF-16 units.
    ArchiveFailed { status: u16, body: JsString },
    Engine(ContainersError),
}

impl From<ContainersError> for Failed {
    fn from(error: ContainersError) -> Self {
        Failed::Engine(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    pub content_type: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub path: String,
    pub base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialWritten {
    pub tool: String,
    pub path: String,
    pub bytes: usize,
}

pub struct AgentShell {
    containers: Arc<dyn Containers>,
    ids: Arc<dyn Ids>,
    clock: Arc<dyn Clock>,
    log: Arc<dyn EventLog>,
}

impl AgentShell {
    pub fn new(containers: Arc<dyn Containers>, ids: Arc<dyn Ids>, clock: Arc<dyn Clock>, log: Arc<dyn EventLog>) -> Self {
        Self { containers, ids, clock, log }
    }

    async fn container_id(&self) -> Result<String, Failed> {
        let listed = self.containers.list_by_app(APP).await?;
        pick_app_container(&listed).map(|container| container.id.clone()).ok_or(Failed::NotRunning)
    }

    async fn run(&self, id: &str, cmd: &[String]) -> Result<crate::application::ports::ExecResult, Failed> {
        Ok(self.containers.exec(id, cmd, &ExecOptions::default()).await?)
    }

    fn tar_of(&self, file: &SingleFile) -> Vec<u8> {
        build_single_file_tar(file, u64::try_from(self.clock.now_ms() / 1000).unwrap_or(0))
    }

    async fn put_archive(&self, id: &str, dir: &str, tar: &[u8]) -> Result<(), Failed> {
        let reply = self.containers.put_file(id, dir, tar).await?;
        if reply.status == 200 {
            return Ok(());
        }
        Err(Failed::ArchiveFailed { status: reply.status, body: JsString::from(reply.body.as_str()).first(ARCHIVE_BODY_UNITS) })
    }

    /// The dir is made private and the file goes in by tar, never through a shell string.
    async fn write_file_into(&self, id: &str, dir: &str, tar: &[u8]) -> Result<(), Failed> {
        let mkdir = self.run(id, &["mkdir", "-p", "-m", "700", dir].map(String::from)).await?;
        if mkdir.exit_code != Some(0) {
            return Err(Failed::MkdirFailed { exit_code: mkdir.exit_code, output: mkdir.output });
        }
        self.put_archive(id, dir, tar).await
    }

    /// Lands the script in a job dir and starts it detached; returns the job id.
    pub async fn start_job(&self, script: &[u8]) -> Result<String, Failed> {
        let id = self.container_id().await?;
        if script.is_empty() {
            return Err(Failed::EmptyScript);
        }
        self.run(&id, &["sh".to_string(), "-c".to_string(), JOBS_CLEANUP.to_string()]).await?;
        let job_id = self.ids.uuid();
        let dir = shell::job_dir(&job_id);
        let text = String::from_utf8_lossy(script);
        let tar = self.tar_of(&SingleFile { mode: 0o700, ..SingleFile::new("script.sh", text.as_bytes()) });
        self.write_file_into(&id, &dir, &tar).await?;
        let detached = ExecOptions { detach: true, env: Vec::new() };
        self.containers.exec(&id, &agent_shell_job_argv(&dir, EXEC_SCRIPT_BACKSTOP_SEC), &detached).await?;
        Ok(job_id)
    }

    /// A job with no files reads as running with no output.
    pub async fn job_status(&self, job_id: &str) -> Result<JobStatus, Failed> {
        if !is_valid_job_id(job_id) {
            return Err(Failed::BadJobId);
        }
        let id = self.container_id().await?;
        let cmd = ["sh", "-c", JOB_STATUS_SCRIPT, "sh", &shell::job_dir(job_id)].map(String::from);
        Ok(JobStatus::read(&self.run(&id, &cmd).await?.output))
    }

    /// TERMs the job's group and KILLs what outlives the grace; what the script exits with is not read.
    pub async fn cancel_job(&self, job_id: &str) -> Result<(), Failed> {
        if !is_valid_job_id(job_id) {
            return Err(Failed::BadJobId);
        }
        let id = self.container_id().await?;
        self.run(&id, &agent_shell_cancel_argv(&shell::job_dir(job_id), JOB_CANCEL_GRACE_SEC)).await?;
        Ok(())
    }

    pub async fn read_ledger(&self, name: &str) -> Result<Ledger, Failed> {
        let content_type = shell::ledger_content_type(name).ok_or(Failed::UnknownLedger)?;
        let id = self.container_id().await?;
        let cat = self.run(&id, &["cat".to_string(), shell::ledger_file(name)]).await?;
        if cat.exit_code != Some(0) {
            return Err(Failed::LedgerNotFound);
        }
        Ok(Ledger { content_type, text: cat.output })
    }

    /// `path` is the request's decoded query value, `''` when there is none.
    pub async fn read_artifact(&self, path: &str) -> Result<Artifact, Failed> {
        if !shell::is_valid_artifact_path(path) {
            return Err(Failed::BadArtifactPath);
        }
        let id = self.container_id().await?;
        let result = self.run(&id, &["bash".to_string(), "-c".to_string(), shell::artifact_guard_script(path)]).await?;
        let printed = result.output.trim_matches(crate::domain::js::is_space);
        if result.exit_code != Some(0) || printed == "NOT_FOUND" {
            return Err(Failed::ArtifactNotFound(path.to_string()));
        }
        if printed == "NOT_ALLOWED" {
            return Err(Failed::ArtifactEscapes);
        }
        Ok(Artifact { path: path.to_string(), base64: printed.to_string() })
    }

    /// Writes one tool's credential file as the `agent` user; the body is the vnc-auth relay's `{tool, content}` with the file's
    /// bytes in base64. The body is checked before agent-shell is looked for.
    pub async fn write_credentials(&self, body: &[u8]) -> Result<CredentialWritten, Failed> {
        let post = parse_record(body).ok_or(Failed::NotJson)?;
        let tool = string_or_null(post.get("tool")).ok_or(Failed::UnknownTool)?;
        let relative = shell::credential_path(tool).ok_or(Failed::UnknownTool)?;
        let content = string_or_null(post.get("content")).filter(|content| !content.is_empty()).ok_or(Failed::EmptyContent)?;
        let bytes = lenient_base64(content);
        if bytes.is_empty() {
            return Err(Failed::InvalidBase64);
        }
        let id = self.container_id().await?;
        let (relative_dir, file_name) = relative.rsplit_once('/').unwrap_or(("", relative));
        let dir = format!("{AGENT_SHELL_HOME}/{relative_dir}");
        let made = self.run(&id, &["mkdir", "-p", "-m", "700", dir.as_str()].map(String::from)).await?;
        if made.exit_code != Some(0) {
            return Err(Failed::MkdirFailed { exit_code: made.exit_code, output: made.output });
        }
        self.run(&id, &["chown".to_string(), format!("{AGENT_SHELL_UID}:{AGENT_SHELL_UID}"), dir.clone()]).await?;
        let tar = self.tar_of(&SingleFile { uid: AGENT_SHELL_UID, gid: AGENT_SHELL_UID, ..SingleFile::new(file_name, &bytes) });
        self.put_archive(&id, &dir, &tar).await?;
        self.log.emit(Event::info("agent_shell_credentials.written").with("tool", tool));
        Ok(CredentialWritten { tool: tool.to_string(), path: format!("{AGENT_SHELL_HOME}/{relative}"), bytes: bytes.len() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::{EngineReply, ExecResult};
    use crate::domain::containers::DockerContainer;
    use crate::testing::{ContainersCall, FakeClock, FakeContainers, FakeEventLog, FakeIds};

    const JOB: &str = "0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab";

    fn container(id: &str, name: &str) -> DockerContainer {
        DockerContainer { id: id.to_string(), names: Some(vec![format!("/{name}")]), state: Some("running".to_string()), ..Default::default() }
    }

    fn ok(output: &str) -> Result<ExecResult, ContainersError> {
        Ok(ExecResult::attached(Some(0), output.to_string()))
    }

    fn exits(code: i64, output: &str) -> Result<ExecResult, ContainersError> {
        Ok(ExecResult::attached(Some(code), output.to_string()))
    }

    struct World {
        shell: AgentShell,
        containers: Arc<FakeContainers>,
        log: Arc<FakeEventLog>,
    }

    fn world(containers: FakeContainers) -> World {
        let containers = Arc::new(containers.with_app_containers(APP, vec![container("as2", "agent-shell-sessions"), container("as1", "agent-shell"), container("as3", "agent-shell-tailscale")]));
        let log = Arc::new(FakeEventLog::default());
        let shell = AgentShell::new(containers.clone(), Arc::new(FakeIds::default()), Arc::new(FakeClock::at(1_700_000_000_123)), log.clone());
        World { shell, containers, log }
    }

    fn execs(world: &World) -> Vec<Vec<String>> {
        world.containers.calls().into_iter().filter_map(|call| if let ContainersCall::Exec { cmd, .. } = call { Some(cmd) } else { None }).collect()
    }

    #[tokio::test]
    async fn given_a_script_when_started_then_it_is_cleaned_up_after_landed_by_tar_and_started_detached_in_the_apps_container() {
        // Given agent-shell, where every exec exits 0
        let w = world(FakeContainers::new().with_exec(&[], vec![ok("")]));
        // When a script is started
        let job = w.shell.start_job(b"echo hi\n").await.expect("started");
        // Then the job dir is private, holds the script by tar (mode 0700, root) and the wrapper runs detached with the backstop
        let dir = format!("/tmp/exec-jobs/{job}");
        let calls = w.containers.calls();
        let ContainersCall::PutFile { container_id, target_dir, tar } = &calls[3] else { panic!("no put in {calls:?}") };
        assert_eq!((container_id.as_str(), target_dir.as_str()), ("as1", dir.as_str()));
        assert_eq!(&tar[100..108], b"0000700\0");
        assert_eq!(&tar[512..520], b"echo hi\n");
        assert_eq!(execs(&w)[..2], [vec!["sh".to_string(), "-c".to_string(), JOBS_CLEANUP.to_string()], ["mkdir", "-p", "-m", "700", dir.as_str()].map(String::from).to_vec()]);
        let ContainersCall::Exec { cmd, options, .. } = &calls[4] else { panic!("no start in {calls:?}") };
        assert_eq!((cmd, options.detach), (&agent_shell_job_argv(&dir, 1800), true));
    }

    #[tokio::test]
    async fn given_a_service_whose_only_container_is_exited_when_a_start_is_made_then_it_is_not_running_and_only_the_list_is_asked() {
        let exited = DockerContainer { state: Some("exited".to_string()), ..container("as1", "agent-shell") };
        let containers = Arc::new(FakeContainers::new().with_app_containers(APP, vec![exited]));
        let shell = AgentShell::new(containers.clone(), Arc::new(FakeIds::default()), Arc::new(FakeClock::at(0)), Arc::new(FakeEventLog::default()));
        assert_eq!(shell.start_job(b"echo hi").await, Err(Failed::NotRunning));
        assert_eq!(containers.calls(), [ContainersCall::ListByApp(APP.to_string())]);
    }

    #[tokio::test]
    async fn given_a_start_when_agent_shell_is_missing_or_the_script_empty_then_the_container_is_looked_for_first() {
        let none = AgentShell::new(
            Arc::new(FakeContainers::new()),
            Arc::new(FakeIds::default()),
            Arc::new(FakeClock::at(0)),
            Arc::new(FakeEventLog::default()),
        );
        assert_eq!(none.start_job(b"").await, Err(Failed::NotRunning));
        let w = world(FakeContainers::new());
        assert_eq!(w.shell.start_job(b"").await, Err(Failed::EmptyScript));
        assert_eq!(w.containers.calls(), [ContainersCall::ListByApp(APP.to_string())]);
    }

    #[tokio::test]
    async fn given_a_failing_mkdir_or_archive_when_started_then_each_is_its_own_refusal_and_no_job_starts() {
        let long = format!("{}\u{20ac}\u{1f600}{}", "x".repeat(498), "y".repeat(100));
        let w = world(
            FakeContainers::new()
                .with_exec(&["sh", "-c"], vec![exits(1, "")])
                .with_exec(&["mkdir"], vec![exits(1, "no space\n"), ok("")])
                .with_put_replies(vec![Ok(EngineReply { status: 500, body: long })]),
        );

        let refused_mkdir = w.shell.start_job(b"x").await;
        let refused_put = w.shell.start_job(b"x").await;

        assert_eq!(refused_mkdir, Err(Failed::MkdirFailed { exit_code: Some(1), output: "no space\n".to_string() }));
        let Err(Failed::ArchiveFailed { status, body }) = refused_put else { panic!("{refused_put:?}") };
        assert_eq!((status, body.len(), body.to_json().ends_with("\\ud83d\"")), (500, 500, true));
        assert!(execs(&w).iter().all(|cmd| cmd[0] != "setsid"));
    }

    #[tokio::test]
    async fn given_job_ids_when_status_is_read_or_cancelled_then_a_non_uuid_is_refused_before_the_engine_is_asked() {
        let w = world(FakeContainers::new().with_exec(&["sh", "-c"], vec![ok(&format!("log{}0", shell::EXIT_MARKER))]));

        assert_eq!(w.shell.job_status("not-a-uuid").await, Err(Failed::BadJobId));
        assert_eq!(w.shell.cancel_job("../x").await, Err(Failed::BadJobId));
        assert_eq!(w.containers.calls(), Vec::<ContainersCall>::new());
        let status = w.shell.job_status(JOB).await.expect("read");
        w.shell.cancel_job(JOB).await.expect("cancelled");

        assert_eq!((status.running, status.exit_code_json()), (false, "0".to_string()));
        assert_eq!(execs(&w)[1], agent_shell_cancel_argv(&format!("/tmp/exec-jobs/{JOB}"), 10));
    }

    #[tokio::test]
    async fn given_a_cancel_that_exits_1_when_cancelled_then_it_is_still_done() {
        let w = world(FakeContainers::new().with_exec(&["sh"], vec![exits(1, "")]));
        assert_eq!(w.shell.cancel_job(JOB).await, Ok(()));
    }

    #[tokio::test]
    async fn given_ledgers_when_read_then_a_listed_name_is_catted_and_any_other_is_refused_without_an_exec() {
        let w = world(FakeContainers::new().with_exec(&["cat", "/workspace/vm-ledger/pool-cooldowns.json"], vec![ok("{}\n")]).with_exec(&["cat", "/workspace/vm-ledger/opencode-dispatch-runs.jsonl"], vec![exits(1, "cat: no such file\n")]));

        let cooldowns = w.shell.read_ledger("pool-cooldowns.json").await;
        let other = w.shell.read_ledger("secrets.env").await;
        let missing = w.shell.read_ledger("opencode-dispatch-runs.jsonl").await;

        assert_eq!(cooldowns, Ok(Ledger { content_type: "application/json; charset=utf-8", text: "{}\n".to_string() }));
        assert_eq!((other, missing), (Err(Failed::UnknownLedger), Err(Failed::LedgerNotFound)));
        assert_eq!(execs(&w).len(), 2);
    }

    #[tokio::test]
    async fn given_artifact_paths_when_read_then_a_bad_path_never_reaches_agent_shell_and_a_failed_guard_is_not_found() {
        let w = world(FakeContainers::new().with_exec(&["bash", "-c"], vec![ok("aGVsbG8=\n"), exits(1, "NOT_ALLOWED\n"), ok("NOT_FOUND\n")]));

        let found = w.shell.read_artifact("runs/r1/shot.png").await;
        let escaped = w.shell.read_artifact("link.png").await;
        let gone = w.shell.read_artifact("gone.png").await;
        let bad = w.shell.read_artifact("a/../b").await;

        assert_eq!(found, Ok(Artifact { path: "runs/r1/shot.png".to_string(), base64: "aGVsbG8=".to_string() }));
        assert_eq!((escaped, gone, bad), (Err(Failed::ArtifactNotFound("link.png".to_string())), Err(Failed::ArtifactNotFound("gone.png".to_string())), Err(Failed::BadArtifactPath)));
        assert_eq!(execs(&w).len(), 3);
    }

    #[tokio::test]
    async fn given_a_credential_post_when_written_then_the_dir_goes_to_uid_1001_the_file_lands_by_tar_and_the_event_follows() {
        let w = world(FakeContainers::new().with_exec(&["mkdir"], vec![ok("")]).with_exec(&["chown"], vec![exits(1, "not permitted")]));

        let written = w.shell.write_credentials(br#"{"tool":"claude","content":"aGk"}"#).await;

        assert_eq!(written, Ok(CredentialWritten { tool: "claude".to_string(), path: "/workspace/home/.claude/.credentials.json".to_string(), bytes: 2 }));
        let calls = w.containers.calls();
        let ContainersCall::PutFile { target_dir, tar, .. } = calls.last().expect("calls") else { panic!("no put in {calls:?}") };
        assert_eq!(target_dir, "/workspace/home/.claude");
        assert_eq!((&tar[100..108], &tar[108..116], &tar[116..124], &tar[512..514]), (&b"0000600\0"[..], &b"0001751\0"[..], &b"0001751\0"[..], &b"hi"[..]));
        assert_eq!(w.log.events(), [Event::info("agent_shell_credentials.written").with("tool", "claude")]);
    }

    #[tokio::test]
    async fn given_a_bad_credential_post_when_written_then_it_is_refused_in_node_s_order_before_agent_shell_is_looked_for() {
        let none = AgentShell::new(Arc::new(FakeContainers::new()), Arc::new(FakeIds::default()), Arc::new(FakeClock::at(0)), Arc::new(FakeEventLog::default()));
        let refused = |body: &'static str| {
            let shell = &none;
            async move { shell.write_credentials(body.as_bytes()).await }
        };

        assert_eq!(refused("{oops").await, Err(Failed::NotJson));
        for body in [r#"{"tool":"vim","content":"aGk="}"#, r#"{"content":"aGk="}"#, r#"{"tool":5,"content":"aGk="}"#, "", "[1]", r#"{"tool":"toString","content":"aGk="}"#, r#"{"tool":"__proto__","content":"aGk="}"#] {
            assert_eq!(refused(body).await, Err(Failed::UnknownTool), "{body}");
        }
        for body in [r#"{"tool":"claude"}"#, r#"{"tool":"claude","content":""}"#, r#"{"tool":"claude","content":5}"#] {
            assert_eq!(refused(body).await, Err(Failed::EmptyContent), "{body}");
        }
        for body in [r#"{"tool":"claude","content":"!!!"}"#, r#"{"tool":"claude","content":"=aGk"}"#] {
            assert_eq!(refused(body).await, Err(Failed::InvalidBase64), "{body}");
        }
        assert_eq!(refused(r#"{"tool":"claude","content":"aGk="}"#).await, Err(Failed::NotRunning));
    }
}
