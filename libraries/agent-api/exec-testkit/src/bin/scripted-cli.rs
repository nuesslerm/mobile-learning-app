//! The program that stands in for `claude`, `codex` and `opencode`. It is started under one of those names from a directory that also holds
//! `<name>.script.json` (what to do, `exec_testkit::script`) and `<name>.invocations.jsonl` (the log it appends to). It never reads its
//! arguments for anything but the MCP server's address and the log: the script decides what it prints and how it ends.
use exec_testkit::app_server::{Output, Player};
use exec_testkit::script::{app_server_log_file, log_file, script_file, Invocation, Received, Script, Step, Turn};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fail(code: i32, message: &str) -> ! {
    eprintln!("scripted-cli: {message}");
    std::process::exit(code)
}

/// The executor's MCP server as the CLI was told of it: Claude Code's `--mcp-config` argument, opencode's `OPENCODE_CONFIG_CONTENT`.
fn mcp_url(args: &[String]) -> Option<String> {
    let from_arg = args.iter().position(|a| a == "--mcp-config").and_then(|at| args.get(at + 1)).and_then(|text| serde_json::from_str::<Value>(text).ok()).and_then(|config| config["mcpServers"]["executor"]["url"].as_str().map(String::from));
    from_arg.or_else(|| {
        let config: Value = serde_json::from_str(&std::env::var("OPENCODE_CONFIG_CONTENT").ok()?).ok()?;
        config["mcp"]["executor"]["url"].as_str().map(String::from)
    })
}

/// One JSON-RPC request to `url` over HTTP/1.1, and the JSON the server answers.
fn post(url: &str, body: &Value) -> Result<Value, String> {
    let rest = url.strip_prefix("http://").ok_or("the MCP server is not at an http:// address")?;
    let (authority, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(a, p)| (a, format!("/{p}")));
    let mut stream = TcpStream::connect(authority).map_err(|e| format!("{authority}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(60))).map_err(|e| e.to_string())?;
    let body = body.to_string();
    let request = format!("POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status).map_err(|e| e.to_string())?;
    let (mut length, mut chunked) = (None, false);
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).map_err(|e| e.to_string())?;
        let header = header.trim_end().to_ascii_lowercase();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().ok();
        }
        chunked |= header.starts_with("transfer-encoding:") && header.contains("chunked");
    }
    let mut payload = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).map_err(|e| e.to_string())?;
            let size = usize::from_str_radix(size.trim().split(';').next().unwrap_or("0"), 16).map_err(|e| e.to_string())?;
            if size == 0 {
                break;
            }
            let mut chunk = vec![0; size];
            reader.read_exact(&mut chunk).map_err(|e| e.to_string())?;
            payload.extend(chunk);
            let mut crlf = String::new();
            reader.read_line(&mut crlf).map_err(|e| e.to_string())?;
        }
    } else if let Some(length) = length {
        payload.resize(length, 0);
        reader.read_exact(&mut payload).map_err(|e| e.to_string())?;
    } else {
        reader.read_to_end(&mut payload).map_err(|e| e.to_string())?;
    }
    if !status.contains(" 200 ") {
        return Err(format!("the MCP server answered {}", status.trim_end()));
    }
    serde_json::from_slice(&payload).map_err(|e| format!("the MCP server's answer is no JSON: {e}"))
}

fn print(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// What the CLI's own file and shell tools do, in the directory it was started in (the workspace host's), as `(text, is_error)`; none for a tool
/// only the service answers (`approval_prompt`, `ask_user`), which goes through the MCP server.
fn run_own_tool(tool: &str, input: &Value) -> Option<(String, bool)> {
    let path = || input["path"].as_str().unwrap_or_default().to_string();
    match tool {
        "write_file" => {
            let (target, content) = (PathBuf::from(path()), input["content"].as_str().unwrap_or_default());
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            Some(match std::fs::write(&target, content) {
                Ok(()) => (format!("wrote {} bytes", content.len()), false),
                Err(e) => (format!("{}: {e}", target.display()), true),
            })
        }
        "read_file" => Some(match std::fs::read_to_string(path()) {
            Ok(content) => (content, false),
            Err(e) => (format!("{}: {e}", path()), true),
        }),
        "run_command" => {
            let out = std::process::Command::new("sh").arg("-c").arg(input["command"].as_str().unwrap_or_default()).output().unwrap_or_else(|e| fail(3, &e.to_string()));
            let (mut text, code) = (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code().unwrap_or(-1));
            if code != 0 {
                text.push_str(&format!("\n[exit code {code}]"));
            }
            Some((text, code != 0))
        }
        _ => None,
    }
}

/// A tool call as the named CLI prints it, the call first and then its result.
fn call_tool(program: &str, args: &[String], id: &str, tool: &str, input: &Value) {
    if program == "claude" {
        print(&json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": id, "name": format!("mcp__executor__{tool}"), "input": input}]}}).to_string());
    }
    let (text, is_error) = match run_own_tool(tool, input) {
        Some(done) => done,
        None => {
            let Some(url) = mcp_url(args) else { fail(3, "no MCP server in --mcp-config or OPENCODE_CONFIG_CONTENT") };
            let answer = post(&url, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": input}})).unwrap_or_else(|e| fail(3, &e));
            let result = &answer["result"];
            (result["content"][0]["text"].as_str().unwrap_or_default().to_string(), result["isError"] == true)
        }
    };
    if program == "claude" {
        print(&json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": text, "is_error": is_error}]}}).to_string());
    } else {
        let state = if is_error { json!({"status": "error", "input": input, "error": text}) } else { json!({"status": "completed", "input": input, "output": text}) };
        print(&json!({"type": "tool_use", "part": {"callID": id, "tool": format!("executor_{tool}"), "state": state}}).to_string());
    }
}

/// The files below `dir`, as paths relative to it, sorted.
fn files_below(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, at: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(at) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else if let Ok(relative) = path.strip_prefix(base) {
                out.push(relative.display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// What a login CLI does at the end: its tokens go to a file of the directory it was told to keep them in.
fn write_credential(env: &str, file: &str, content: &str, pasted: &str) {
    let Some(dir) = std::env::var_os(env) else { fail(5, &format!("{env} is not set: no directory to keep a login in")) };
    let path = Path::new(&dir).join(file);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, content.replace("{paste}", pasted)).unwrap_or_else(|e| fail(5, &format!("{}: {e}", path.display())));
}

/// The version an unscripted program prints (`claude 0.0.0-mimic`); the hermetic host of `mimic-script-host` prints the same.
const DEFAULT_VERSION: &str = "0.0.0-mimic";

/// Plays one turn's steps, then ends the process as the turn says.
fn play(turn: &Turn, program: &str, args: &[String]) -> ! {
    let mut pasted = String::new();
    for step in &turn.steps {
        match step {
            Step::Line { text } => print(text),
            Step::Sleep { ms } => std::thread::sleep(Duration::from_millis(*ms)),
            Step::McpCall { id, tool, args: input } => call_tool(program, args, id, tool, input),
            Step::ReadLine => {
                let mut line = String::new();
                if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
                    fail(4, "stdin closed before a line was pasted");
                }
                pasted = line.trim_end().to_string();
            }
            Step::Credential { env, file, content } => write_credential(env, file, content, &pasted),
        }
    }
    if !turn.stderr.is_empty() {
        eprintln!("{}", turn.stderr);
    }
    if turn.hang {
        exec_testkit::hang_until_orphaned();
    }
    std::process::exit(turn.exit_code);
}

/// Codex's app-server on stdio: one JSON request a line, answered from the script by method; it ends when its input does.
fn serve_app_server(script: &Script) -> ! {
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = request.get("id").cloned() else { continue };
        let method = request["method"].as_str().unwrap_or_default();
        let mut answer = if method == "initialize" {
            json!({"result": {"userAgent": "scripted-codex"}})
        } else {
            script.app_server.get(method).cloned().unwrap_or_else(|| json!({"error": {"code": -32601, "message": format!("no such method: {method}")}}))
        };
        answer["id"] = id;
        print(&answer.to_string());
    }
    std::process::exit(0)
}

/// Codex's app-server playing a conversation: each message of the client is logged and answered from the run this start plays, and the process ends
/// with its input (or as the run says). A message the run does not wait for ends it with code 7.
fn play_app_server(script: &Script, dir: &Path, program: &str) -> ! {
    let log = dir.join(app_server_log_file(program));
    let start = std::fs::read_dir(dir).ok().and_then(|_| std::fs::read_to_string(dir.join(format!("{program}.app-server.starts"))).ok()).map_or(0, |t| t.lines().count()) + 1;
    {
        let mut starts = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{program}.app-server.starts"))).unwrap_or_else(|e| fail(1, &e.to_string()));
        let _ = writeln!(starts, "{}", std::process::id());
    }
    let run = script.app_server_runs.get((start - 1).min(script.app_server_runs.len().saturating_sub(1))).cloned().unwrap_or_default();
    let mut player = Player::new(run);
    let act = |outputs: Vec<Output>| {
        for output in outputs {
            match output {
                Output::Line(line) => print(&line),
                Output::Sleep(ms) => std::thread::sleep(Duration::from_millis(ms)),
                Output::Exit(code) => std::process::exit(code),
            }
        }
    };
    act(player.begin());
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let received = Received { start, pid: std::process::id(), message: message.clone() };
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(file, "{}", serde_json::to_string(&received).unwrap_or_default());
        }
        match player.on_client(&message) {
            Ok(outputs) => act(outputs),
            Err(why) => fail(7, &why),
        }
    }
    std::process::exit(0)
}

fn main() {
    let exe = std::env::current_exe().unwrap_or_else(|e| fail(1, &format!("no path of its own: {e}")));
    let (dir, program) = (exe.parent().unwrap_or(Path::new(".")).to_path_buf(), exe.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let written = std::fs::read_to_string(dir.join(script_file(&program))).ok();
    // A program nobody scripted answers a version query like the recording host's stub does, so a recorded server and a replayed chain see the same CLIs.
    let unscripted = Script { version: Some(format!("{program} {DEFAULT_VERSION}")), ..Script::default() };
    let script: Script = match &written {
        None => unscripted,
        Some(text) => serde_json::from_str(text).unwrap_or_default(),
    };
    if let (Some(text), [only]) = (&script.version, args.as_slice()) {
        if only == "--version" {
            println!("{text}");
            std::process::exit(0);
        }
    }
    if args.first().map(String::as_str) == Some("app-server") && !script.app_server_runs.is_empty() {
        play_app_server(&script, &dir, &program);
    }
    if args.first().map(String::as_str) == Some("app-server") && !script.app_server.is_empty() {
        serve_app_server(&script);
    }
    if let Some(turn) = args.first().and_then(|first| script.commands.get(first)) {
        play(turn, &program, &args);
    }
    let log = dir.join(log_file(&program));
    let starts = std::fs::read_to_string(&log).map_or(0, |text| text.lines().count());
    let turn: Turn = script.turn_for(starts);
    let invocation = Invocation {
        program: program.clone(),
        argv: args.clone(),
        cwd: std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
        env: turn.log_env.iter().map(|name| (name.clone(), std::env::var(name).ok())).collect::<BTreeMap<_, _>>(),
        listing: turn.list_env.iter().map(|name| (name.clone(), std::env::var_os(name).map(|d| files_below(Path::new(&d))).unwrap_or_default())).collect::<BTreeMap<_, _>>(),
    };
    // Logged before anything runs, so a start that hangs or is killed is in the log; a command start plays no turn, so it is not one
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&log).unwrap_or_else(|e| fail(1, &format!("{}: {e}", log.display())));
    let _ = writeln!(file, "{}", serde_json::to_string(&invocation).unwrap_or_default());
    drop(file);
    play(&turn, &program, &args)
}
