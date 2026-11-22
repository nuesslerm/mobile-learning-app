//! The drivers, the request of one turn, and the CLI invocation each driver runs for it. Claude Code and Codex have none: they run as live processes
//! the service talks to (`claude`, `codex`, and `application::claude_sessions`, `application::codex_sessions`).
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Driver {
    Opencode,
    ClaudeAgent,
    Codex,
}

pub const DRIVERS: [Driver; 3] = [Driver::Opencode, Driver::ClaudeAgent, Driver::Codex];

impl Driver {
    pub fn parse(instance: &str) -> Option<Driver> {
        DRIVERS.into_iter().find(|d| d.id() == instance)
    }

    pub fn id(self) -> &'static str {
        match self {
            Driver::Opencode => "opencode",
            Driver::ClaudeAgent => "claudeAgent",
            Driver::Codex => "codex",
        }
    }

    /// Whether the CLI stays up between turns and the service talks to it (a session), instead of being started for each turn.
    pub fn lives(self) -> bool {
        matches!(self, Driver::Codex | Driver::ClaudeAgent)
    }

    /// The arguments that start the CLI as a live process, for a driver that lives: Codex's server for all its threads, Claude Code's stream-json
    /// conversation (one process for one thread, which the session completes with the thread's own flags).
    pub fn server_args(self) -> Option<&'static [&'static str]> {
        match self {
            Driver::Codex => Some(&["app-server"]),
            Driver::ClaudeAgent => Some(&["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages", "--replay-user-messages", "--permission-prompt-tool", "stdio"]),
            Driver::Opencode => None,
        }
    }

    pub fn program(self) -> &'static str {
        match self {
            Driver::Opencode => "opencode",
            Driver::ClaudeAgent => "claude",
            Driver::Codex => "codex",
        }
    }
}

/// The service's tool server a CLI is told of (`ask_user`, which waits for the person), as the CLI at the workspace host reaches it: the turn's token is a bearer.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolServer {
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnRequest {
    pub thread_id: String,
    pub run_id: String,
    pub instance_id: String,
    pub model: Option<String>,
    pub cwd: String,
    pub prompt: String,
    pub runtime_mode: String,
    pub interaction_mode: String,
    /// The CLI's conversation to go on with (Claude Code's `--resume`, opencode's `--session`, Codex's `thread/resume`), when the thread has one.
    pub resume: Option<String>,
    /// The tool server a live CLI is started with (a process keeps the one it started with); none for a driver that is told of it by its command.
    pub tools: Option<ToolServer>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
    /// Where the CLI starts: `.` for the executor's own directory, else a directory below it or a worktree's absolute path (the turn's request names it).
    pub cwd: String,
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// The CLI invocation of one turn: non-interactive, JSON lines on stdout, the prompt as the last argument. None for a driver that lives.
pub fn command_for(driver: Driver, req: &TurnRequest) -> Option<Command> {
    let mut a = match driver {
        Driver::Codex | Driver::ClaudeAgent => return None,
        Driver::Opencode => {
            let mut base = args(&["run", "--format", "json"]);
            if let Some(session) = &req.resume {
                base.extend(args(&["--session", session]));
            }
            base
        }
    };
    if let Some(model) = &req.model {
        a.extend(args(&["-m", model]));
    }
    a.push(req.prompt.clone());
    Some(Command { program: driver.program().into(), args: a, cwd: req.cwd.clone() })
}

/// The invocation of one turn at the workspace host: the CLI keeps its own tools and credentials. The only addition is the service's tool server at
/// `mcp_url` (`ask_user`, which waits for the person), which opencode is told about, with the turn's `mcp_token` as a bearer (the CLI may run on another
/// host than the service). None for a driver that lives.
pub fn command_for_host(driver: Driver, req: &TurnRequest, mcp_url: &str, mcp_token: &str) -> Option<Command> {
    let mut command = command_for(driver, req)?;
    let prompt = command.args.pop().unwrap_or_default();
    match driver {
        Driver::Opencode => {
            let config = json!({"mcp": {"executor": {"type": "remote", "url": mcp_url, "enabled": true, "headers": {"Authorization": format!("Bearer {mcp_token}")}}}});
            command.args.push(prompt);
            let mut wrapped = args(&[&format!("OPENCODE_CONFIG_CONTENT={config}"), &command.program]);
            wrapped.extend(command.args);
            Some(Command { program: "env".into(), args: wrapped, cwd: req.cwd.clone() })
        }
        Driver::Codex | Driver::ClaudeAgent => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(model: Option<&str>) -> TurnRequest {
        TurnRequest { thread_id: "t".into(), run_id: "r".into(), instance_id: "opencode".into(), model: model.map(str::to_string), cwd: "/w/p".into(), prompt: "hi there".into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: None, tools: None }
    }

    fn host(driver: Driver, req: &TurnRequest, url: &str, token: &str) -> Command {
        command_for_host(driver, req, url, token).expect("a driver that is started for each turn")
    }

    fn turn(driver: Driver, req: &TurnRequest) -> Command {
        command_for(driver, req).expect("a driver that is started for each turn")
    }

    #[test]
    fn given_a_conversation_to_go_on_with_when_an_opencode_turn_is_built_then_it_goes_on_with_its_own_session_by_the_flag_it_has_for_it() {
        // Given a thread whose CLI conversation is s-1
        let going_on = TurnRequest { resume: Some("s-1".into()), ..req(None) };
        // When the turn is built, with and without the conversation
        let opencode = turn(Driver::Opencode, &going_on);
        // Then it resumes the conversation by --session, and a first turn names none
        assert!(opencode.args.windows(2).any(|w| w == ["--session", "s-1"]), "{:?}", opencode.args);
        assert!(!turn(Driver::Opencode, &req(None)).args.contains(&"--session".to_string()));
    }

    #[test]
    fn given_the_drivers_that_live_when_a_command_is_asked_for_then_there_is_none_because_they_live_and_their_process_is_started_by_its_session() {
        // Given a turn of each driver that lives
        for (driver, args) in [(Driver::Codex, &["app-server"][..]), (Driver::ClaudeAgent, &["-p", "--input-format", "stream-json"][..])] {
            let live = TurnRequest { instance_id: driver.id().into(), ..req(Some("m")) };
            // When a command per turn is asked for, for the plain and the host invocation
            // Then there is none of either, and the driver names the arguments that start its process
            assert_eq!((command_for(driver, &live), command_for_host(driver, &live, "u", "t")), (None, None), "{}", driver.id());
            assert!(driver.lives() && driver.server_args().is_some_and(|a| a.starts_with(args)), "{}", driver.id());
        }
        // And Claude Code's process takes its permission prompts and every message over its own stdio, with the stream the client sees live
        let claude = Driver::ClaudeAgent.server_args().unwrap();
        assert!(claude.windows(2).any(|w| w == ["--permission-prompt-tool", "stdio"]) && claude.contains(&"--replay-user-messages") && claude.contains(&"--include-partial-messages"));
        // And opencode lives no more than a turn
        assert!(!Driver::Opencode.lives() && Driver::Opencode.server_args().is_none() && command_for(Driver::Opencode, &req(None)).is_some());
    }

    #[test]
    fn a_driver_runs_its_cli_non_interactively_with_the_model_and_the_prompt_last() {
        let opencode = turn(Driver::Opencode, &req(Some("m/x")));
        assert_eq!(opencode.program, "opencode");
        assert!(opencode.args.windows(2).any(|w| w == ["-m", "m/x"]));
        assert_eq!(opencode.args.last().map(String::as_str), Some("hi there"));
    }

    #[test]
    fn a_host_turn_keeps_the_cli_s_own_tools_and_only_names_the_service_s_tool_server() {
        let opencode = host(Driver::Opencode, &req(None), "http://x/mcp/t", "tok-1");
        assert_eq!((opencode.program.as_str(), opencode.args[1].as_str(), opencode.args.last().map(String::as_str)), ("env", "opencode", Some("hi there")));
        assert!(opencode.args[0].starts_with("OPENCODE_CONFIG_CONTENT=") && opencode.args[0].contains("Bearer tok-1") && !opencode.args[0].contains(r#""bash":false"#));
    }

    #[test]
    fn a_driver_is_found_by_the_instance_id_a_request_names() {
        // Given every driver and an instance that is none
        // When each id is parsed, Then it names its driver and the unknown one names none
        assert!(DRIVERS.iter().all(|d| Driver::parse(d.id()) == Some(*d)));
        assert_eq!(Driver::parse("nobody"), None);
    }
}
