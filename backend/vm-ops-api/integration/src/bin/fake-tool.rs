//! The `docker` and `wf` double, linked into each case's `bin/` under both names. It appends `{tool, argv}` as one JSON
//! line to `$FAKE_TOOL_LOG` and never runs what it is given, a sibling's script included. The rules in `$FAKE_TOOL_RULES`
//! (`[{tool, prefix, rest, delayMs, answer}]`; the first whose tool and argv prefix match, and whose `rest` regex, when it
//! has one, matches the further arguments joined with one space, wins) delay the call and may script its answer: stdout
//! and stderr, each written whole or in two writes cut at a byte offset (`stdoutCut`, `stderrCut`), so a character can be
//! split across two reads of the service, then the exit code. Without a scripted answer, `wf log emit` answers
//! `{"sent":true}` as the real one does, a `docker run` with `-d` before its command prints a container id, a
//! `wf secret sync-env --base B --out O` writes O as B plus two vault secrets, and any other call exits 97, so a case
//! never mistakes an unscripted call for a success. A rule with `nth` applies only to the nth call it would match, so a
//! case can fail a first `docker compose up` and pass the second; an answer's `writes` (file to text) and `removes`
//! (files) change files relative to the call's directory before it answers. When a script run sets `FAKE_TOOL_SEE`
//! (colon-separated files) or `FAKE_TOOL_ENV` (comma-separated variable names), each record also carries the call's
//! `cwd`, those files' text at that moment (`seen`) and those variables (`env`). Run under its own name as
//! `fake-tool env-names`, it prints the names, never the values, of the variables it was handed. A rule with `stdin: true`
//! makes the matching call read its standard input to the end and record it as `stdin` (no other call reads it: one handed a
//! terminal would wait for it forever).
use regex::Regex;
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// What a detached `docker run` prints: the new container's id.
const CONTAINER_ID: &str = "c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00";
/// The pause between the two writes of a cut answer, so the service reads each as a chunk of its own.
const CUT_GAP: Duration = Duration::from_millis(150);
/// The secrets a double `wf secret sync-env` resolves from the vault, after the base file's lines.
const VAULT_SECRETS: &str = "VM_OPS_API_AUTH_TOKEN=vault-token\nTS_VM_AUTH_KEY=vault-key\n";

fn stop(code: i32, why: String) -> ! {
    eprintln!("fake-tool: {why}");
    std::process::exit(code);
}

/// Whether the rule's tool, argv prefix and `rest` regex match a call.
fn applies(r: &Value, tool: &str, args: &[String]) -> bool {
    let prefix: Vec<&str> = r["prefix"].as_array().map(|p| p.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let head = r["tool"] == tool && args.len() >= prefix.len() && args.iter().zip(&prefix).all(|(a, p)| a == p);
    head && r["rest"].as_str().is_none_or(|rest| {
        let rest = Regex::new(rest).unwrap_or_else(|e| stop(96, format!("the rule's rest regex {rest:?}: {e}")));
        rest.is_match(&args[prefix.len()..].join(" "))
    })
}

/// How many calls the log holds that the rule applies to, this one included: it is appended before the rules are read.
fn call_number(r: &Value) -> u64 {
    let text = std::env::var_os("FAKE_TOOL_LOG").and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    let calls = text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok());
    let matching = calls.filter(|c| {
        let argv: Vec<String> = c["argv"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default();
        applies(r, c["tool"].as_str().unwrap_or(""), &argv)
    });
    matching.count() as u64
}

/// The rules file, or `None` when the call has none; a file that cannot be read stops the call when `strict`.
fn rules(strict: bool) -> Option<Vec<Value>> {
    let file = std::env::var_os("FAKE_TOOL_RULES").filter(|f| Path::new(f).exists())?;
    let rules = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok());
    if rules.is_none() && strict {
        stop(96, format!("cannot read the rules in {}", Path::new(&file).display()));
    }
    rules
}

/// The first rule that matches this call; a rules file or regex that cannot be read stops the call.
fn rule(tool: &str, args: &[String]) -> Option<Value> {
    rules(true)?.into_iter().find(|r| applies(r, tool, args) && r["nth"].as_u64().is_none_or(|n| n == call_number(r)))
}

/// Whether a rule that applies to this call asks for its standard input.
fn wants_stdin(tool: &str, args: &[String]) -> bool {
    rules(false).is_some_and(|all| all.iter().any(|r| r["stdin"] == true && applies(r, tool, args)))
}

/// The extra fields a script run asks each record to carry.
fn detail() -> Map<String, Value> {
    let mut extra = Map::new();
    if let Ok(see) = std::env::var("FAKE_TOOL_SEE") {
        let seen: Map<String, Value> = see
            .split(':')
            .filter(|p| !p.is_empty())
            .map(|p| (p.to_string(), std::fs::read_to_string(p).map_or(Value::Null, Value::from)))
            .collect();
        let cwd = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default();
        extra.insert("cwd".into(), json!(cwd));
        extra.insert("seen".into(), Value::Object(seen));
    }
    if let Ok(names) = std::env::var("FAKE_TOOL_ENV") {
        let env: Map<String, Value> = names.split(',').filter_map(|n| std::env::var(n).ok().map(|v| (n.to_string(), json!(v)))).collect();
        extra.insert("env".into(), Value::Object(env));
    }
    extra
}

/// `wf secret sync-env --base B --out O`: O is B's text and the vault's secrets, as the real one resolves them.
fn sync_env(args: &[String]) {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
    let (Some(base), Some(out)) = (value("--base"), value("--out")) else { stop(2, "sync-env needs --base and --out".into()) };
    let mut text = std::fs::read_to_string(base).unwrap_or_else(|e| stop(1, format!("cannot read {base}: {e}")));
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(VAULT_SECRETS);
    std::fs::write(out, text).unwrap_or_else(|e| stop(1, format!("cannot write {out}: {e}")));
}

/// Writes `text` whole, or cut at byte `cut` into two writes with a pause between them.
fn write(out: &mut impl Write, text: &str, cut: Option<usize>) {
    let bytes = text.as_bytes();
    let at = cut.unwrap_or(bytes.len()).min(bytes.len());
    let _ = out.write_all(&bytes[..at]).and_then(|()| out.flush());
    if at < bytes.len() {
        std::thread::sleep(CUT_GAP);
        let _ = out.write_all(&bytes[at..]).and_then(|()| out.flush());
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let tool = argv.first().and_then(|a| Path::new(a).file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let args = &argv[1.min(argv.len())..];
    if tool == "fake-tool" && args == ["env-names"] {
        for (name, _) in std::env::vars_os() {
            println!("{}", name.to_string_lossy());
        }
        return;
    }
    if let Some(log) = std::env::var_os("FAKE_TOOL_LOG") {
        // One O_APPEND write per record: Node spawns `wf log emit` and a sibling in the same tick, and two writes could interleave
        let mut record = json!({ "tool": tool, "argv": args });
        record.as_object_mut().expect("a record is an object").extend(detail());
        if wants_stdin(&tool, args) {
            let mut text = String::new();
            let _ = std::io::stdin().read_to_string(&mut text);
            record["stdin"] = json!(text);
        }
        let record = record.to_string() + "\n";
        let appended = std::fs::OpenOptions::new().create(true).append(true).open(&log).and_then(|mut f| f.write_all(record.as_bytes()));
        if let Err(e) = appended {
            stop(98, format!("cannot append to {}: {e}", Path::new(&log).display()));
        }
    }
    let rule = rule(&tool, args);
    let delay = rule.as_ref().and_then(|r| r["delayMs"].as_u64()).map_or(Duration::ZERO, Duration::from_millis);
    std::thread::sleep(delay);
    if let Some(answer) = rule.as_ref().map(|r| &r["answer"]).filter(|a| a.is_object()) {
        for (file, text) in answer["writes"].as_object().into_iter().flatten() {
            std::fs::write(file, text.as_str().unwrap_or("")).unwrap_or_else(|e| stop(98, format!("cannot write {file}: {e}")));
        }
        for file in answer["removes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let _ = std::fs::remove_file(file);
        }
        let cut = |key: &str| answer[key].as_u64().and_then(|n| usize::try_from(n).ok());
        write(&mut std::io::stdout().lock(), answer["stdout"].as_str().unwrap_or(""), cut("stdoutCut"));
        write(&mut std::io::stderr().lock(), answer["stderr"].as_str().unwrap_or(""), cut("stderrCut"));
        std::process::exit(answer["exitCode"].as_i64().and_then(|c| i32::try_from(c).ok()).unwrap_or(0));
    }
    if tool == "wf" && args.len() >= 2 && args[0] == "log" && args[1] == "emit" {
        println!("{}", json!({ "sent": true }));
        return;
    }
    if tool == "wf" && args.len() >= 2 && args[0] == "secret" && args[1] == "sync-env" {
        return sync_env(args);
    }
    let detached = args.iter().take_while(|a| *a != "sh").any(|a| a == "-d" || a == "--detach");
    if tool == "docker" && args.first().is_some_and(|a| a == "run") && detached {
        println!("{CONTAINER_ID}");
        return;
    }
    stop(97, format!("no answer for {tool} {args:?}"));
}
