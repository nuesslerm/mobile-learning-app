//! The drivers, the request of one turn, and the CLI invocation each driver runs for it.
use serde_json::{json, Value};

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

    pub fn program(self) -> &'static str {
        match self {
            Driver::Opencode => "opencode",
            Driver::ClaudeAgent => "claude",
            Driver::Codex => "codex",
        }
    }
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
    /// The CLI's conversation to go on with (Claude Code's `--resume`), when the thread has one.
    pub resume: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// The CLI invocation of one turn: non-interactive, JSON lines on stdout, the prompt as the last argument.
pub fn command_for(driver: Driver, req: &TurnRequest) -> Command {
    let mut a = match driver {
        Driver::ClaudeAgent => {
            let mode = if req.interaction_mode == "plan" {
                "plan"
            } else {
                match req.runtime_mode.as_str() {
                    "approval-required" => "default",
                    "auto-accept-edits" => "acceptEdits",
                    _ => "bypassPermissions",
                }
            };
            let mut base = args(&["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", mode]);
            if let Some(session) = &req.resume {
                base.extend(args(&["--resume", session]));
            }
            base
        }
        Driver::Codex => {
            let sandbox = match (req.interaction_mode.as_str(), req.runtime_mode.as_str()) {
                ("plan", _) => "read-only",
                (_, "full-access") => "danger-full-access",
                _ => "workspace-write",
            };
            args(&["exec", "--json", "--skip-git-repo-check", "--sandbox", sandbox])
        }
        Driver::Opencode => args(&["run", "--format", "json"]),
    };
    if let Some(model) = &req.model {
        a.extend(args(&[if driver == Driver::Codex || driver == Driver::Opencode { "-m" } else { "--model" }, model]));
    }
    a.push(req.prompt.clone());
    Command { program: driver.program().into(), args: a }
}

/// The built-in file and shell tools of Claude Code that the executor replaces.
const CLAUDE_BUILTINS: &str = "Bash,BashOutput,KillShell,Read,Write,Edit,MultiEdit,NotebookEdit,Glob,Grep,LS";
/// The same for opencode's `tools` map.
const OPENCODE_BUILTINS: [&str; 9] = ["bash", "read", "write", "edit", "patch", "glob", "grep", "list", "todoread"];
/// The tool of the executor's MCP server that Claude Code asks whether a tool may run, in the permission mode that asks (`approval-required`).
pub const CLAUDE_PERMISSION_PROMPT_TOOL: &str = "mcp__executor__approval_prompt";

/// Why a driver cannot run against the executor channel.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteUnsupported(pub String);

/// The invocation of one turn whose file and shell tools are the executor's: the CLI's built-ins off and the MCP server at `mcp_url` the only
/// tool source. Claude Code takes the config inline (`--strict-mcp-config` ignores every other server); opencode takes it as per-turn config
/// content through `env`, since a spawn carries no environment. Codex is not wired (its flags are unconfirmed, see the plan's spike).
pub fn command_for_remote(driver: Driver, req: &TurnRequest, mcp_url: &str) -> Result<Command, RemoteUnsupported> {
    let mut command = command_for(driver, req);
    let prompt = command.args.pop().unwrap_or_default();
    match driver {
        Driver::ClaudeAgent => {
            let config = json!({"mcpServers": {"executor": {"type": "http", "url": mcp_url}}});
            // `--mcp-config` and `--disallowedTools` take lists, so the prompt goes right after `-p` rather than last.
            command.args.insert(1, prompt);
            command.args.extend(args(&["--mcp-config", &config.to_string(), "--strict-mcp-config", "--disallowedTools", CLAUDE_BUILTINS]));
            if req.runtime_mode == "approval-required" && req.interaction_mode != "plan" {
                command.args.extend(args(&["--permission-prompt-tool", CLAUDE_PERMISSION_PROMPT_TOOL]));
            }
            Ok(command)
        }
        Driver::Opencode => {
            let tools: serde_json::Map<String, Value> = OPENCODE_BUILTINS.iter().map(|t| (t.to_string(), json!(false))).collect();
            let config = json!({"mcp": {"executor": {"type": "remote", "url": mcp_url, "enabled": true}}, "tools": tools});
            command.args.push(prompt);
            let mut wrapped = args(&[&format!("OPENCODE_CONFIG_CONTENT={config}"), &command.program]);
            wrapped.extend(command.args);
            Ok(Command { program: "env".into(), args: wrapped })
        }
        Driver::Codex => Err(RemoteUnsupported("codex cannot use the executor channel yet: its MCP and shell-tool flags are not confirmed".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(model: Option<&str>) -> TurnRequest {
        TurnRequest { thread_id: "t".into(), run_id: "r".into(), instance_id: "claudeAgent".into(), model: model.map(str::to_string), cwd: "/w/p".into(), prompt: "hi there".into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: None }
    }

    #[test]
    fn given_a_conversation_to_go_on_with_and_a_mode_that_asks_when_a_claude_turn_is_built_then_it_resumes_and_names_the_tool_that_asks() {
        // Given a thread whose CLI conversation is s-1, a turn in the mode that asks before each tool, and one in the mode that never does
        let asking = TurnRequest { runtime_mode: "approval-required".into(), resume: Some("s-1".into()), ..req(None) };
        // When the turn is built for the executor channel, and with the mode that never asks
        let asks = command_for_remote(Driver::ClaudeAgent, &asking, "http://x/mcp/t").unwrap();
        let never = command_for_remote(Driver::ClaudeAgent, &req(None), "http://x/mcp/t").unwrap();
        let plan = command_for_remote(Driver::ClaudeAgent, &TurnRequest { interaction_mode: "plan".into(), ..asking.clone() }, "http://x/mcp/t").unwrap();
        // Then the first resumes the conversation and names the executor's approval tool for permission prompts, the others name none, and a plan never asks
        assert!(asks.args.windows(2).any(|w| w == ["--resume", "s-1"]), "{:?}", asks.args);
        assert!(asks.args.windows(2).any(|w| w == ["--permission-prompt-tool", "mcp__executor__approval_prompt"]), "{:?}", asks.args);
        assert!(!never.args.iter().any(|a| a == "--resume" || a == "--permission-prompt-tool"), "{:?}", never.args);
        assert!(!plan.args.contains(&"--permission-prompt-tool".to_string()));
        // And the prompt still follows -p, and a driver that cannot resume ignores the conversation
        assert_eq!((asks.args[0].as_str(), asks.args[1].as_str()), ("-p", "hi there"));
        assert!(!command_for(Driver::Opencode, &asking).args.contains(&"--resume".to_string()));
    }

    #[test]
    fn a_driver_runs_its_cli_non_interactively_with_the_model_and_the_prompt_last() {
        let claude = command_for(Driver::ClaudeAgent, &req(Some("opus")));
        assert_eq!(claude.program, "claude");
        assert!(claude.args.windows(2).any(|w| w == ["--model", "opus"]) && claude.args.windows(2).any(|w| w == ["--permission-mode", "bypassPermissions"]));
        assert_eq!(claude.args.last().map(String::as_str), Some("hi there"));
        let plan = command_for(Driver::Codex, &TurnRequest { interaction_mode: "plan".into(), ..req(None) });
        assert!(plan.args.windows(2).any(|w| w == ["--sandbox", "read-only"]));
    }

    #[test]
    fn a_remote_turn_disables_the_builtin_tools_and_names_the_executor_as_the_only_tool_server() {
        let claude = command_for_remote(Driver::ClaudeAgent, &req(None), "http://127.0.0.1:1/mcp/t").unwrap();
        assert_eq!((claude.program.as_str(), claude.args[1].as_str()), ("claude", "hi there"));
        let at = |flag: &str| claude.args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("{flag}"));
        assert!(claude.args[at("--mcp-config") + 1].contains("http://127.0.0.1:1/mcp/t") && claude.args[at("--disallowedTools") + 1].contains("Bash"));
        assert!(claude.args.contains(&"--strict-mcp-config".to_string()));
        let opencode = command_for_remote(Driver::Opencode, &TurnRequest { instance_id: "opencode".into(), ..req(None) }, "http://x/mcp/t").unwrap();
        assert_eq!((opencode.program.as_str(), opencode.args[1].as_str(), opencode.args.last().map(String::as_str)), ("env", "opencode", Some("hi there")));
        assert!(opencode.args[0].starts_with("OPENCODE_CONFIG_CONTENT=") && opencode.args[0].contains(r#""bash":false"#));
        assert!(command_for_remote(Driver::Codex, &req(None), "u").is_err());
    }

    #[test]
    fn a_driver_is_found_by_the_instance_id_a_request_names() {
        // Given every driver and an instance that is none
        // When each id is parsed, Then it names its driver and the unknown one names none
        assert!(DRIVERS.iter().all(|d| Driver::parse(d.id()) == Some(*d)));
        assert_eq!(Driver::parse("nobody"), None);
    }
}
