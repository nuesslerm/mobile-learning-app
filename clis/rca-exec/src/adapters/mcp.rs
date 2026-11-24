//! One request to a stdio MCP server the workspace declares in its `.mcp.json`: the server is started in the workspace root with only its own
//! declared environment, initialized, asked once and stopped. Nothing is kept between calls, so a server that needs state across calls is not
//! served by this action. The declared environment (which may hold a key) is never returned, and a server's own error text is replaced by a fixed message, so a server that echoes its environment cannot leak it.
use crate::adapters::provider_account::{answer_with, kill_group, lines};
use exec_proc::{McpAnswer, McpRequest, MCP_ANSWER_MAX, MCP_CONFIG, MCP_PROTOCOL};
use exec_proto::{Code, ExecError, ExecResult};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn io(e: std::io::Error) -> ExecError {
    ExecError::new(Code::Io, e.to_string())
}

/// What `.mcp.json` says about one server: the program, its arguments and the environment it declares.
#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The server `name` of the config text, or why it cannot be started: no such server, not a stdio server, or a malformed entry.
pub fn server_of(config: &str, name: &str) -> ExecResult<Server> {
    let parsed: Value = serde_json::from_str(config).map_err(|_| ExecError::invalid_args(format!("{MCP_CONFIG} is not JSON")))?;
    let entry = parsed["mcpServers"].get(name).ok_or_else(|| ExecError::new(Code::NotFound, format!("{MCP_CONFIG} declares no server {name:?}")))?;
    if entry.get("url").is_some() || entry["type"].as_str().is_some_and(|t| t != "stdio") {
        return Err(ExecError::invalid_args(format!("server {name:?} is not a stdio server; only stdio servers are started here")));
    }
    let command = entry["command"].as_str().filter(|c| !c.is_empty()).ok_or_else(|| ExecError::invalid_args(format!("server {name:?} has no `command`")))?.to_string();
    let args = match &entry["args"] {
        Value::Null => Vec::new(),
        Value::Array(all) => all.iter().map(|a| a.as_str().map(str::to_string).ok_or_else(|| ExecError::invalid_args(format!("server {name:?} has an `args` entry that is no string")))).collect::<ExecResult<_>>()?,
        _ => return Err(ExecError::invalid_args(format!("server {name:?} has `args` that is no array"))),
    };
    let env = match &entry["env"] {
        Value::Null => Vec::new(),
        Value::Object(all) => all.iter().map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())).ok_or_else(|| ExecError::invalid_args(format!("server {name:?} has an `env` value that is no string")))).collect::<ExecResult<_>>()?,
        _ => return Err(ExecError::invalid_args(format!("server {name:?} has `env` that is no object"))),
    };
    Ok(Server { command, args, env })
}

/// Starts `server` in `root`, does the handshake, sends the one request, stops the server.
pub fn request(args: &McpRequest, server: &Server, root: &Path, search: &str, home: &Path) -> ExecResult<McpAnswer> {
    let mut command = Command::new(&server.command);
    command.args(&server.args).env_clear().env("PATH", search).env("HOME", home).envs(server.env.iter().cloned()).current_dir(root);
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).process_group(0).spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ExecError::new(Code::NotFound, format!("the program of server {:?} is not installed here", args.server)),
        _ => io(e),
    })?;
    let rx = lines(&mut child);
    let who = format!("MCP server {:?}", args.server);
    let outcome = (|| {
        let mut input = child.stdin.take().ok_or_else(|| ExecError::new(Code::Io, format!("{who} takes no input")))?;
        let deadline = Instant::now() + Duration::from_millis(args.timeout_ms);
        let mut send = |value: Value| writeln!(input, "{value}").and_then(|()| input.flush()).map_err(io);
        let initialize = tracing::info_span!("mcp.initialize", api.outcome = "error").entered();
        send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": MCP_PROTOCOL, "capabilities": {}, "clientInfo": {"name": "rca-exec", "version": "1.0.0"}}}))?;
        answer_with(&rx, 1, deadline, &who, false)?;
        initialize.record("api.outcome", "ok");
        drop(initialize);
        let notice = tracing::info_span!("mcp.notifications.initialized", api.outcome = "error").entered();
        send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
        notice.record("api.outcome", "sent");
        drop(notice);
        let request = tracing::info_span!("mcp.request", otel.name = %format!("mcp.{}", args.method), api.outcome = "error").entered();
        send(json!({"jsonrpc": "2.0", "id": 2, "method": args.method, "params": args.params}))?;
        let result = answer_with(&rx, 2, deadline, &who, false)?;
        if result.to_string().len() > MCP_ANSWER_MAX {
            return Err(ExecError::new(Code::Io, format!("{who} answered more than {MCP_ANSWER_MAX} bytes")));
        }
        request.record("api.outcome", "ok");
        Ok(McpAnswer { result })
    })();
    kill_group(&mut child);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_config_when_a_server_is_read_then_a_stdio_entry_is_returned_and_every_other_shape_is_refused_by_name() {
        // Given a config with a stdio server, a url server and one without a command
        let config = r#"{"mcpServers": {"ok": {"command": "node", "args": ["s.js"], "env": {"K": "v"}}, "remote": {"url": "https://x"}, "bare": {}, "typed": {"type": "sse", "command": "x"}}}"#;
        // When each is read, Then the stdio entry has its program, arguments and environment, and the others are refused
        assert_eq!(server_of(config, "ok").unwrap(), Server { command: "node".into(), args: vec!["s.js".into()], env: vec![("K".into(), "v".into())] });
        assert_eq!(server_of(config, "remote").unwrap_err().code, Code::InvalidArgs);
        assert_eq!(server_of(config, "typed").unwrap_err().code, Code::InvalidArgs);
        assert!(server_of(config, "bare").unwrap_err().message.contains("no `command`"));
        assert_eq!(server_of(config, "gone").unwrap_err().code, Code::NotFound);
        assert_eq!(server_of("{", "ok").unwrap_err().code, Code::InvalidArgs);
    }
}
