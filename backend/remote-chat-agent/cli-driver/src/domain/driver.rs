//! The drivers and the request of one turn. Every driver runs as a live process or server the service talks to (`application::claude_sessions`,
//! `application::codex_sessions`, `application::opencode_sessions`): there is no CLI invocation per turn.

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

    /// The arguments that start the CLI as a live process the service speaks to over its stdio: Codex's server for all its threads, Claude Code's stream-json
    /// conversation (one process for one thread, which the session completes with the thread's own flags). None for opencode, whose server is reached over
    /// HTTP through the executor's bridge (`session_open` at a workspace host starts it).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_drivers_when_their_process_arguments_are_asked_for_then_the_ones_on_stdio_name_them_and_opencode_names_none_because_it_is_reached_over_http() {
        // Given every driver
        // When the arguments that start its live process are asked for
        // Then Codex starts its app-server, Claude Code takes its permission prompts and every message over its own stdio with the stream the client sees live, and opencode has none
        assert_eq!(Driver::Codex.server_args(), Some(&["app-server"][..]));
        let claude = Driver::ClaudeAgent.server_args().unwrap();
        assert!(claude.starts_with(&["-p", "--input-format", "stream-json"]) && claude.windows(2).any(|w| w == ["--permission-prompt-tool", "stdio"]) && claude.contains(&"--replay-user-messages") && claude.contains(&"--include-partial-messages"));
        assert_eq!(Driver::Opencode.server_args(), None);
    }

    #[test]
    fn a_driver_is_found_by_the_instance_id_a_request_names() {
        // Given every driver and an instance that is none
        // When each id is parsed, Then it names its driver and the unknown one names none
        assert!(DRIVERS.iter().all(|d| Driver::parse(d.id()) == Some(*d)));
        assert_eq!(Driver::parse("nobody"), None);
    }
}
