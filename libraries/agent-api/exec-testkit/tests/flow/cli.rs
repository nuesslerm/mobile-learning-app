//! The scripted CLI as the service starts it.
use exec_testkit::{recordings, Script, ScriptedCli, Turn};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// An MCP server that answers every JSON-RPC request with `reply` (chunked when `chunked`) and keeps the bodies it was sent.
fn mcp_server(reply: Value, chunked: bool) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mcp/t1", listener.local_addr().unwrap());
    let bodies: Arc<Mutex<Vec<Value>>> = Arc::default();
    let seen = bodies.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream);
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            seen.lock().unwrap().push(serde_json::from_slice(&body).unwrap());
            let payload = reply.to_string();
            let head = if chunked { "Transfer-Encoding: chunked".to_string() } else { format!("Content-Length: {}", payload.len()) };
            let body = if chunked { format!("{:x}\r\n{payload}\r\n0\r\n\r\n", payload.len()) } else { payload };
            let _ = reader.get_mut().write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{head}\r\nConnection: close\r\n\r\n{body}").as_bytes());
        }
    });
    (url, bodies)
}

fn mcp_result(text: &str, is_error: bool) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "result": {"content": [{"type": "text", "text": text}], "isError": is_error}})
}

struct Run {
    code: Option<i32>,
    stdout: Vec<String>,
    stderr: String,
}

/// Starts `program` from the kit's directory with exactly `env`, in `cwd`, and waits for it.
fn run(cli: &ScriptedCli, program: &str, args: &[&str], cwd: &std::path::Path, env: &[(&str, &str)]) -> Run {
    let out = Command::new(cli.dir().join(program)).args(args).current_dir(cwd).env_clear().envs(env.iter().copied()).output().unwrap();
    Run { code: out.status.code(), stdout: String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() }
}

#[test]
fn a_started_program_prints_the_recording_it_is_scripted_with_ends_with_the_code_it_is_told_and_logs_how_it_was_started() {
    // Given the program scripted to replay a Claude Code recording, complain, exit 3 and keep the value of CLAUDE_CONFIG_DIR and CODEX_HOME
    let (cli, recording, cwd) = (ScriptedCli::install(), recordings::named("claude-write-file"), tempfile::tempdir().unwrap());
    cli.script("claude", &Script::of(vec![Turn::replaying(&recording.lines).complaining("rate limited").exiting(3).logging_env(&["CLAUDE_CONFIG_DIR", "CODEX_HOME"])]));
    // When it is started as claude with arguments, in a directory, with two variables set (one of them named by the script)
    let ran = run(&cli, "claude", &["-p", "hi", "--output-format", "stream-json"], cwd.path(), &[("CLAUDE_CONFIG_DIR", "/data/providers/claude"), ("SECRET_TOKEN", "s3cret")]);
    // Then it printed the recording's lines in order, said what the script says on stderr, and exited with the code
    assert_eq!((ran.stdout, ran.code), (recording.lines, Some(3)));
    assert!(ran.stderr.contains("rate limited"), "{}", ran.stderr);
    // And the log has its arguments, its directory and the named variables: the one that was set, the one that was not, and not the secret
    let log = cli.invocations("claude");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].argv, ["-p", "hi", "--output-format", "stream-json"]);
    assert_eq!(std::fs::canonicalize(&log[0].cwd).unwrap(), std::fs::canonicalize(cwd.path()).unwrap());
    assert_eq!(serde_json::to_value(&log[0].env).unwrap(), json!({"CLAUDE_CONFIG_DIR": "/data/providers/claude", "CODEX_HOME": null}));
    assert_eq!(log[0].program, "claude");
}

#[test]
fn each_name_plays_the_script_of_that_name_and_a_program_with_no_script_prints_nothing_and_exits_cleanly_but_is_logged() {
    // Given the three programs installed, claude and codex scripted with different recordings and opencode not at all
    let (cli, cwd) = (ScriptedCli::install(), tempfile::tempdir().unwrap());
    let (claude, codex) = (recordings::named("claude-text"), recordings::named("codex-text"));
    cli.script("claude", &Script::of(vec![Turn::replaying(&claude.lines)]));
    cli.script("codex", &Script::of(vec![Turn::replaying(&codex.lines)]));
    // When each is started
    let (a, b, c) = (run(&cli, "claude", &[], cwd.path(), &[]), run(&cli, "codex", &["exec"], cwd.path(), &[]), run(&cli, "opencode", &["run"], cwd.path(), &[]));
    // Then each printed its own script, and the unscripted one printed nothing and exited 0
    assert_eq!((a.stdout, b.stdout, c.stdout, c.code), (claude.lines, codex.lines, vec![], Some(0)));
    // And all three are in the logs, each under its own name
    assert_eq!(cli.invocations("opencode")[0].argv, ["run"]);
    assert_eq!((cli.invocations("claude").len(), cli.invocations("codex").len()), (1, 1));
}

#[test]
fn a_script_of_several_turns_plays_one_per_start_and_the_last_again_after_them() {
    // Given a script of two turns: the first prints the text recording, the second fails with code 4
    let (cli, cwd) = (ScriptedCli::install(), tempfile::tempdir().unwrap());
    let text = recordings::named("opencode-text");
    cli.script("opencode", &Script::of(vec![Turn::replaying(&text.lines), Turn::default().then_line("second").exiting(4)]));
    // When the program is started three times
    let starts: Vec<Run> = (0..3).map(|_| run(&cli, "opencode", &[], cwd.path(), &[])).collect();
    // Then the first prints the recording, and the second and the third play the second turn
    assert_eq!((starts[0].stdout.clone(), starts[0].code), (text.lines, Some(0)));
    assert_eq!([(starts[1].stdout.clone(), starts[1].code), (starts[2].stdout.clone(), starts[2].code)], [(vec!["second".to_string()], Some(4)), (vec!["second".to_string()], Some(4))]);
    assert_eq!(cli.invocations("opencode").len(), 3);
    // And a script set again forgets the starts so far
    cli.script("opencode", &Script::of(vec![Turn::default().then_line("again")]));
    assert!(cli.invocations("opencode").is_empty());
    assert_eq!(run(&cli, "opencode", &[], cwd.path(), &[]).stdout, ["again"]);
}

#[test]
fn a_program_told_to_hang_prints_its_lines_and_stays_until_it_is_killed_and_was_logged_before_it_hung() {
    // Given a program that prints one line and hangs
    let (cli, cwd) = (ScriptedCli::install(), tempfile::tempdir().unwrap());
    cli.script("claude", &Script::of(vec![Turn::default().then_line(r#"{"type":"system"}"#).hanging()]));
    let mut child = Command::new(cli.dir().join("claude")).current_dir(cwd.path()).stdout(Stdio::piped()).spawn().unwrap();
    // When its first line has come and a while has passed
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut first).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    // Then it is still running, and the start is in the log
    assert_eq!((first.trim(), child.try_wait().unwrap().is_none()), (r#"{"type":"system"}"#, true));
    assert_eq!(cli.invocations("claude").len(), 1);
    // And killing it ends it
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
}

#[test]
fn a_tool_call_goes_through_the_mcp_server_the_cli_was_started_with_and_is_printed_as_that_cli_prints_it() {
    let cwd = tempfile::tempdir().unwrap();
    let args = json!({"path": "notes.txt", "content": "hello\n"});
    for (is_error, chunked) in [(false, false), (true, true)] {
        // Given an MCP server that answers a call with text (a failure when is_error), and both programs scripted to call write_file
        let cli = ScriptedCli::install();
        let (url, bodies) = mcp_server(mcp_result("wrote 6 bytes", is_error), chunked);
        for program in ["claude", "opencode"] {
            cli.script(program, &Script::of(vec![Turn::default().then_calling("call_1", "write_file", args.clone())]));
        }
        // When claude is started with the server in --mcp-config, and opencode with it in OPENCODE_CONFIG_CONTENT, as the driver starts them
        let config = json!({"mcpServers": {"executor": {"type": "http", "url": url}}}).to_string();
        let claude = run(&cli, "claude", &["-p", "write", "--mcp-config", &config], cwd.path(), &[]);
        let opencode = run(&cli, "opencode", &["run"], cwd.path(), &[("OPENCODE_CONFIG_CONTENT", &json!({"mcp": {"executor": {"type": "remote", "url": url}}}).to_string())]);
        // Then the server was sent a tools/call for the tool with the arguments, once by each
        let calls: Vec<Value> = bodies.lock().unwrap().iter().map(|b| json!([b["method"], b["params"]["name"], b["params"]["arguments"]])).collect();
        assert_eq!(calls, [json!(["tools/call", "write_file", args]), json!(["tools/call", "write_file", args])]);
        // And claude printed the call and then its result, in the lines the driver reads
        assert_eq!(claude.code, Some(0), "{}", claude.stderr);
        let lines: Vec<Value> = claude.stdout.iter().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines[0]["message"]["content"][0], json!({"type": "tool_use", "id": "call_1", "name": "mcp__executor__write_file", "input": args}));
        assert_eq!(lines[1]["message"]["content"][0], json!({"type": "tool_result", "tool_use_id": "call_1", "content": "wrote 6 bytes", "is_error": is_error}));
        // And opencode printed one line with the tool's state
        let line: Value = serde_json::from_str(&opencode.stdout[0]).unwrap();
        let state = if is_error { json!({"status": "error", "input": args, "error": "wrote 6 bytes"}) } else { json!({"status": "completed", "input": args, "output": "wrote 6 bytes"}) };
        assert_eq!((opencode.stdout.len(), line["type"].clone(), line["part"]["callID"].clone(), line["part"]["state"].clone()), (1, json!("tool_use"), json!("call_1"), state));
    }
}

#[test]
fn a_tool_call_the_program_cannot_make_ends_it_with_a_code_and_the_reason_on_stderr() {
    // Given programs scripted to call a tool
    let (cli, cwd) = (ScriptedCli::install(), tempfile::tempdir().unwrap());
    for program in ["claude", "codex"] {
        cli.script(program, &Script::of(vec![Turn::default().then_calling("c", "read_file", json!({"path": "a"}))]));
    }
    // When claude is started with no MCP config, and codex with one
    let config = json!({"mcpServers": {"executor": {"url": "http://127.0.0.1:1/mcp/t"}}}).to_string();
    let no_config = run(&cli, "claude", &["-p", "x"], cwd.path(), &[]);
    let codex = run(&cli, "codex", &["exec", "--mcp-config", &config], cwd.path(), &[]);
    // Then claude ends with 3, saying it has no MCP server, and codex with 2, saying it cannot use the channel
    assert_eq!(no_config.code, Some(3));
    assert!(no_config.stderr.contains("no MCP server"), "{}", no_config.stderr);
    assert_eq!(codex.code, Some(2));
    assert!(codex.stderr.contains("codex cannot use the executor channel"), "{}", codex.stderr);
}

#[test]
fn a_scripted_login_prints_its_prompt_reads_the_pasted_code_keeps_a_credential_in_the_directory_it_was_told_and_logs_what_it_could_see() {
    // Given a login scripted to print a device prompt, read one pasted line and keep it in a file of CLAUDE_CONFIG_DIR, and to list two directories
    let (cli, home) = (ScriptedCli::install(), tempfile::tempdir().unwrap());
    cli.script("claude", &Script::of(vec![Turn::default().then_line("Open https://login.example.test/device and enter ABCD-1234").then_reading_a_line().then_writing_credential("CLAUDE_CONFIG_DIR", ".credentials.json", "{\"token\":\"{paste}\"}").listing_env(&["CLAUDE_CONFIG_DIR", "CODEX_HOME"])]));
    let dir = home.path().join("claude");
    let mut child = Command::new(cli.dir().join("claude")).args(["auth", "login"]).current_dir(home.path()).env_clear().env("CLAUDE_CONFIG_DIR", &dir).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    // When the person pastes a code after the prompt
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut prompt = String::new();
    out.read_line(&mut prompt).unwrap();
    child.stdin.take().unwrap().write_all(b"pasted-code-1\n").unwrap();
    // Then the process ends cleanly having written the file below its own directory with the pasted line
    assert!(child.wait().unwrap().success());
    assert!(prompt.contains("ABCD-1234"));
    assert_eq!(std::fs::read_to_string(dir.join(".credentials.json")).unwrap(), "{\"token\":\"pasted-code-1\"}");
    // And the log lists the files the directory held when it started (none yet), and none for a variable that was not set
    let log = cli.invocations("claude");
    assert_eq!(log[0].listing.get("CLAUDE_CONFIG_DIR"), Some(&Vec::new()));
    assert_eq!(log[0].listing.get("CODEX_HOME"), Some(&Vec::new()));
}

#[test]
fn a_scripted_login_that_is_not_told_where_to_keep_its_login_fails_instead_of_writing_somewhere() {
    // Given a login scripted to keep a credential in CODEX_HOME, started with no such variable
    let (cli, cwd) = (ScriptedCli::install(), tempfile::tempdir().unwrap());
    cli.script("codex", &Script::of(vec![Turn::default().then_writing_credential("CODEX_HOME", "auth.json", "{}")]));
    // When it runs
    let ran = run(&cli, "codex", &["login", "--device-auth"], cwd.path(), &[]);
    // Then it exits with code 5 saying which variable is missing, and wrote nothing in its working directory
    assert_eq!(ran.code, Some(5));
    assert!(ran.stderr.contains("CODEX_HOME is not set"), "{}", ran.stderr);
    assert_eq!(std::fs::read_dir(cwd.path()).unwrap().count(), 0);
}

#[test]
fn a_program_started_with_version_alone_prints_the_scripted_version_without_playing_a_turn_or_being_logged() {
    // Given claude scripted with a version and one turn that prints a line, and codex scripted with a turn only
    let (cli, cwd) = (ScriptedCli::install(), tempfile::tempdir().unwrap());
    cli.script("claude", &Script::of(vec![Turn::default().then_line("turn one")]).printing_version("2.1.7 (Claude Code)"));
    cli.script("codex", &Script::of(vec![Turn::default().then_line("codex turn")]));
    // When claude is asked for its version twice and then started for real, and codex is asked for its version
    let (first, second) = (run(&cli, "claude", &["--version"], cwd.path(), &[]), run(&cli, "claude", &["--version"], cwd.path(), &[]));
    let started = run(&cli, "claude", &["-p", "hi"], cwd.path(), &[]);
    let unscripted = run(&cli, "codex", &["--version"], cwd.path(), &[]);
    // Then the version is printed with code 0, the real start still plays the first turn, and only it is in the log
    assert_eq!((first.stdout, first.code, second.stdout), (vec!["2.1.7 (Claude Code)".to_string()], Some(0), vec!["2.1.7 (Claude Code)".to_string()]));
    assert_eq!(started.stdout, ["turn one"]);
    assert_eq!(cli.invocations("claude").iter().map(|i| i.argv.clone()).collect::<Vec<_>>(), [["-p", "hi"]]);
    // And a program with no scripted version treats `--version` as an ordinary start
    assert_eq!((unscripted.stdout, cli.invocations("codex").len()), (vec!["codex turn".to_string()], 1));
}
