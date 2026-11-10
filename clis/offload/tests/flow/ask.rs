//! `wf offload ask`: one prompt or a batch of files down the cheap-model ladder through `opencode run`, a runbook judged by its step
//! evidence, the run log's stats and savings, and the ladder's pool cooldowns and prices. `opencode` is a Node-script stub that
//! answers per model from a table and logs its argv; the opencode-go preflight, OpenRouter and vm-ops-api are fakes.
use crate::support::{node_path, run_wf, MockReply, MockServer, RunOpts, RunResult};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const M1: &str = "minimax/MiniMax-M3";
const M2: &str = "opencode-go/deepseek-v4.1-flash";
const M3: &str = "kimi-code-plan-global/kimi-for-coding";
const M4: &str = "kimi-code-plan-global/k3";

/// `opencode run ... --model <m> <prompt>`: logs its argv, cwd and the keys it was given, answers from `opencode.json` by model (or
/// `*`) after `sleepMs`, and logs when it started and finished.
const OPENCODE: &str = "const fs = require('fs');
const dir = process.env.STUB_DIR;
const args = process.argv.slice(2);
const model = args[args.indexOf('--model') + 1];
const start = Date.now();
fs.appendFileSync(dir + '/calls.log', JSON.stringify({ args, cwd: process.cwd(), key: process.env.OPENCODE_API_KEY || null, anthropic: [process.env.ANTHROPIC_API_KEY, process.env.ANTHROPIC_AUTH_TOKEN].filter(Boolean) }) + '\\n');
const table = JSON.parse(fs.readFileSync(dir + '/opencode.json', 'utf8'));
const e = table[model] || table['*'] || { stdout: '' };
const emit = () => {
  fs.appendFileSync(dir + '/spans.log', JSON.stringify({ start, end: Date.now() }) + '\\n');
  if (e.stderr) process.stderr.write(e.stderr);
  if (e.stdout) process.stdout.write(e.stdout);
  process.exit(e.exit || 0);
};
if (e.sleepMs) setTimeout(emit, e.sleepMs); else emit();
";

struct Work {
    dir: tempfile::TempDir,
}

struct Call {
    args: Vec<String>,
}

impl Work {
    fn new(prefix: &str) -> Self {
        let dir = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        let opencode = dir.path().join("bin/opencode");
        std::fs::write(&opencode, format!("#!{}\n{OPENCODE}", node_path().expect("node on PATH").display())).unwrap();
        std::fs::set_permissions(&opencode, std::fs::Permissions::from_mode(0o755)).unwrap();
        let w = Work { dir };
        w.table(&json!({}));
        w
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn put(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn table(&self, t: &Value) {
        std::fs::write(self.path("opencode.json"), t.to_string()).unwrap();
    }

    /// `wf offload ask <args>` with the run log and pool ledger in the work dir and the tailnet unreachable; earlier calls, spans and runs are
    /// forgotten.
    fn ask(&self, args: &[&str], env: &[(&str, &str)], input: Option<&str>) -> RunResult {
        for log in ["calls.log", "spans.log", "runs.jsonl"] {
            let _ = std::fs::remove_file(self.path(log));
        }
        let mut full = vec!["offload", "ask"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default()
            .bin_dir(&self.path("bin"))
            .cwd(self.dir.path())
            .clean_env()
            .timeout(Duration::from_secs(60))
            .env("STUB_DIR", &self.dir.path().display().to_string())
            .env("OPENCODE_DISPATCH_RUN_LOG", &self.path("runs.jsonl").display().to_string())
            .env("OPENCODE_POOL_LEDGER", &self.path("pool.json").display().to_string())
            .env("WF_TAILNET_SOCKS", "127.0.0.1:1");
        if let Some(text) = input {
            opts = opts.input(text);
        }
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    fn calls(&self) -> Vec<Call> {
        std::fs::read_to_string(self.path("calls.log"))
            .map(|log| {
                log.trim()
                    .lines()
                    .map(|l| {
                        let c: Value = serde_json::from_str(l).unwrap();
                        Call {
                            args: c["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn prompts(&self) -> Vec<String> {
        self.calls().iter().map(|c| c.args.last().unwrap().clone()).collect()
    }
}

fn ev(o: Value) -> String {
    format!("{o}\n")
}

const RENAMED: &str = "Renamed the helper.";

/// The turn of a run that only answers, as `opencode run --format json` prints it.
fn turn(session: &str, text: &str) -> Value {
    json!([
        { "type": "step_start", "timestamp": 1, "sessionID": session, "part": { "id": "s1", "type": "step-start", "sessionID": session } },
        { "type": "text", "timestamp": 2, "sessionID": session, "part": { "id": "t1", "type": "text", "text": text, "sessionID": session } },
        { "type": "step_finish", "timestamp": 3, "sessionID": session, "part": { "id": "s2", "type": "step-finish", "reason": "stop", "sessionID": session } },
    ])
}

/// What the rca service answers to an ok run: the text, the serving model, and the files the run changed.
fn served(text: &str, model: &str, changes: Value) -> MockReply {
    let attempt = json!({ "index": 0, "choice": { "model": model }, "verdict": { "ok": true } });
    MockReply::json(200, json!({ "ok": true, "output": { "text": text, "toolEvents": [], "structured": true }, "attempt": attempt, "attempts": [attempt], "changes": changes, "events": turn("ses_x", text) }))
}

/// The rca service as a local listener, and the environment that points `wf` at it.
struct Rca {
    server: MockServer,
    port: String,
}

impl Rca {
    fn start() -> Self {
        let server = MockServer::start(None);
        let port = server.port.to_string();
        Rca { server, port }
    }

    fn env(&self) -> Vec<(&str, &str)> {
        vec![("CHAT_AGENT_API_SECRET", "s3cret"), ("RCA_HOST", "127.0.0.1"), ("RCA_PORT", self.port.as_str()), ("WF_TAILNET_SOCKS", "")]
    }

    fn answer(&self, reply: MockReply) {
        self.server.clear();
        self.server.routes(&[("POST /runs", reply)]);
    }

    /// The run requests, without the harness's own `opencode.json` (the table its stub opencode answers from).
    fn bodies(&self) -> Vec<Value> {
        self.requests_with_config()
            .into_iter()
            .map(|mut body| {
                if let Some(files) = body["files"].as_array_mut() {
                    files.retain(|f| f["path"] != "opencode.json");
                }
                body
            })
            .collect()
    }

    fn requests_with_config(&self) -> Vec<Value> {
        self.server.requests().iter().filter(|q| q.method == "POST" && q.url == "/runs").map(|q| serde_json::from_str(&q.body).unwrap()).collect()
    }
}

#[test]
fn an_ask_is_served_by_the_rca_service_with_its_files_and_what_the_run_changed_comes_back() {
    // Given a repository, and the rca service answering a read-only run
    let w = Work::new("ask-rca-");
    w.put("CLAUDE.md", "rules");
    w.put("docs/a.md", "alpha");
    std::fs::create_dir(w.path(".git")).unwrap();
    let shot = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    std::fs::write(shot.path(), [0x89u8, b'P', b'N', b'G']).unwrap();
    let rca = Rca::start();
    rca.answer(served("the answer", M2, json!([])));

    // When a read-only ask names the files its prompt reads, one of them a screenshot staged under another name
    let files = format!("CLAUDE.md,docs/a.md,{}=shots/s.png", shot.path().display());
    let r = w.ask(&["summarise", "--files", &files, "--shape", "judge:text", "--caller", "visual"], &rca.env(), None);

    // Then the service got the prompt, shape, caller and files under the secret, the answer is printed, and no local opencode ran
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "the answer\n"), "{}", r.stderr);
    assert!(r.stderr.contains(&format!("served by {M2}")), "{}", r.stderr);
    assert_eq!(rca.server.requests()[0].headers.get("x-bot-secret").map(String::as_str), Some("s3cret"));
    assert_eq!(
        rca.bodies(),
        [json!({ "prompt": "summarise", "shape": "judge:text", "caller": "visual", "auto": false, "files": [{ "path": "CLAUDE.md", "content": "rules" }, { "path": "docs/a.md", "content": "alpha" }, { "path": "shots/s.png", "contentBase64": "iVBORw==" }] })]
    );
    assert!(w.calls().is_empty());

    // Given a writing ask (the default) whose run rewrote, created and removed files, Then the shape is the implementer's and the files are applied
    w.put("a.ts", "old");
    w.put("gone.ts", "bye");
    rca.answer(served(RENAMED, M1, json!([{ "path": "a.ts", "content": "new" }, { "path": "src/made.ts", "content": "made" }, { "path": "bin.dat", "contentBase64": "iVBORw==" }, { "path": "gone.ts", "deleted": true }])));
    let r = w.ask(&["rename it", "--auto", "--shape", "implementer:unit", "--files", "a.ts,gone.ts", "--model", M3, "--model", M4], &rca.env(), None);
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "Renamed the helper.\n"), "{}", r.stderr);
    let body = &rca.bodies()[0];
    assert_eq!((body["shape"].clone(), body["models"].clone(), body["caller"].clone(), body["auto"].clone()), (json!("implementer:unit"), json!([M3, M4]), json!("dispatch"), json!(true)));
    assert_eq!(std::fs::read_to_string(w.path("a.ts")).unwrap(), "new");
    assert_eq!(std::fs::read_to_string(w.path("src/made.ts")).unwrap(), "made");
    assert_eq!(std::fs::read(w.path("bin.dat")).unwrap(), [0x89u8, b'P', b'N', b'G']);
    assert!(!w.path("gone.ts").exists());

    // Given --summary, Then the changes are listed beside the answer, and the transcript is written
    w.put("gone.ts", "bye");
    let r = w.ask(&["rename it", "--auto", "--files", "a.ts,gone.ts", "--summary=brief"], &rca.env(), None);
    assert!(r.stdout.starts_with("Renamed the helper.\n") && r.stdout.contains("[served by minimax/MiniMax-M3"), "{}", r.stdout);
    let transcript = Regex::new(r"transcript (\S+\.md)").unwrap().captures(&r.stdout).unwrap()[1].to_string();
    let text = std::fs::read_to_string(&transcript).unwrap();
    assert!(text.contains("[write] a.ts") && text.contains("[delete] gone.ts") && text.contains("Renamed the helper."), "{text}");

    // Given a run that answers a path leaving the repository, Then nothing is written and the ask fails
    rca.answer(served("x", M1, json!([{ "path": "../escape.txt", "content": "no" }])));
    let r = w.ask(&["x"], &rca.env(), None);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("leaves the repository") && !w.path("../escape.txt").exists(), "{}", r.stderr);

    // Given every rung failed, Then it exits 1 naming each attempt
    let failed = json!({ "ok": false, "reason": "ladder-exhausted", "attempts": [
        { "index": 0, "choice": { "model": M1 }, "verdict": { "ok": false, "failureClass": "cap" } },
        { "index": 1, "choice": { "model": M2 }, "verdict": { "ok": false, "failureClass": "bad-output" } },
    ] });
    rca.answer(MockReply::json(200, failed));
    let r = w.ask(&["x"], &rca.env(), None);
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, ""));
    assert!(r.stderr.contains(&format!("Error: \x1b[0mAll 2 models were exhausted: {M1} (cap), {M2} (bad-output)")), "{}", r.stderr);
    assert!(r.stderr.contains(&format!("[failed after: {M1} (capped), {M2}]")), "{}", r.stderr);

    // Given the service refuses because its cooldown ledger is down, Then the ask fails with the reason
    rca.answer(MockReply::json(503, json!({ "error": "the pool cooldowns could not be read: ORDS 503", "code": "LEDGER_UNAVAILABLE" })));
    let r = w.ask(&["x"], &rca.env(), None);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("503") && r.stderr.contains("the pool cooldowns could not be read"), "{}", r.stderr);

    // Given no service is listening, Then it fails naming the next step; and a file outside the repository is refused before any request
    let env = [("CHAT_AGENT_API_SECRET", "s3cret"), ("RCA_HOST", "127.0.0.1"), ("RCA_PORT", "1"), ("WF_TAILNET_SOCKS", "")];
    let r = w.ask(&["x"], &env, None);
    assert!(r.exit_code == 1 && r.stderr.contains("unreachable") && r.stderr.contains("wf vm ops stats"), "{}", r.stderr);
    let r = w.ask(&["x", "--files", "../outside.md"], &rca.env(), None);
    assert!(r.exit_code == 1 && r.stderr.contains("stays inside"), "{}", r.stderr);

    // And through all of it no local opencode ran
    assert!(w.calls().is_empty());
}

#[test]
fn a_batch_asks_the_rca_service_once_per_matched_file_with_that_file_and_applies_what_comes_back() {
    // Given two files, a template, and the service rewriting whatever it is sent
    let w = Work::new("wf-flow-ask-batch-");
    std::fs::create_dir(w.path(".git")).unwrap();
    w.put("a.ts", "AAA");
    w.put("sub/b.ts", "BBB");
    w.put("t.md", "fix {{file}}\n{{content}}");
    let rca = Rca::start();
    rca.answer(served("Done.", M1, json!([{ "path": "a.ts", "content": "fixed" }])));

    // When batched one at a time
    let r = w.ask(&["--batch", "a.ts", "sub/*.ts", "-t", "t.md", "--concurrency", "1"], &rca.env(), None);

    // Then each file was asked in order, with its name and content as the prompt and as the attached file
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let bodies = rca.bodies();
    assert_eq!(bodies.iter().map(|b| b["prompt"].as_str().unwrap()).collect::<Vec<_>>(), ["fix a.ts\nAAA", "fix sub/b.ts\nBBB"]);
    assert_eq!(bodies[1]["files"], json!([{ "path": "sub/b.ts", "content": "BBB" }]));
    assert_eq!(bodies[0]["caller"], json!("batch"));
    // And what came back is on disk, and the table lists both
    assert_eq!(std::fs::read_to_string(w.path("a.ts")).unwrap(), "fixed");
    assert!(r.stdout.contains("a.ts") && r.stdout.contains("sub/b.ts") && r.stdout.contains("2/2 changed"), "{}", r.stdout);
    assert!(w.calls().is_empty());
}

const RUNBOOK: &str = "---\ntitle: Demo runbook\nverifies: the demo\ntier: runbook\n---\n# Demo runbook\n\n1. Open the app.\n2. Tap the button.\n";

fn step(description: &str, ui: bool, passed: bool) -> Value {
    json!({ "description": description, "uiAffecting": ui, "passed": passed })
}

fn reply(text: &str) -> String {
    ev(json!({ "type": "step_start", "sessionID": "ses_1" }))
        + &ev(json!({ "type": "tool_use", "part": { "tool": "edit", "state": { "status": "completed", "input": { "filePath": "/repo/a.ts" } } } }))
        + &ev(json!({ "type": "text", "part": { "text": text } }))
        + &ev(json!({ "type": "step_finish", "part": { "cost": 0.001, "tokens": { "input": 1, "output": 1, "total": 2 } } }))
}

#[test]
fn a_runbook_is_driven_through_the_ladder_and_judged_by_the_steps_the_agent_reports() {
    let w = Work::new("wf-flow-ask-runbook-");
    assert!(Command::new("git").args(["init", "-q"]).current_dir(w.dir.path()).status().unwrap().success());
    w.put("docs/runbooks/demo.md", RUNBOOK);
    let answer = |text: &str, exit: i32| w.table(&json!({ "*": { "stdout": reply(text), "exit": exit } }));
    let runbook = |args: &[&str]| {
        let mut full = vec!["--runbook", "demo"];
        full.extend_from_slice(args);
        w.ask(&full, &[], None)
    };
    let steps = |list: &[Value]| Value::Array(list.to_vec()).to_string();

    // Given an agent that reports every step passed, Then the runbook passes and the prompt carries it and where to write the steps
    answer(&steps(&[step("open", false, true), step("tap", false, true)]), 0);
    let r = runbook(&[]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "demo.md: pass\n"));
    let prompt = w.prompts()[0].clone();
    assert!(prompt.contains("Execute the following runbook step by step: \"Demo runbook\".") && prompt.contains("2. Tap the button."), "{prompt}");
    assert!(Regex::new(r"Additionally write the same JSON array to \.agents/tmp/runbook-steps-demo\.md-[\w-]+\.json ").unwrap().is_match(&prompt), "{prompt}");
    // and --json prints the result object
    answer(&steps(&[step("open", false, true)]), 0);
    let out: Value = serde_json::from_str(&runbook(&["--json"]).stdout).unwrap();
    assert_eq!((out["runbook"].as_str(), out["verdict"].as_str(), out["steps"].clone()), (Some("demo.md"), Some("pass"), json!([{ "description": "open", "uiAffecting": false, "passed": true }])));

    // Given a UI-affecting step without a screenshot, or a failing step, Then it is rejected naming the step, or fails
    answer(&steps(&[step("open", false, true), step("tap the button", true, true)]), 0);
    let r = runbook(&[]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, "demo.md: rejected -- 1 UI-affecting step(s) returned no screenshot evidence: tap the button\n"));
    assert!(r.stderr.contains("runbook \"demo.md\" did not pass (rejected)"));
    answer(&steps(&[step("open", false, true), step("tap", false, false)]), 0);
    assert_eq!(runbook(&[]).stdout, "demo.md: fail\n");

    // Given prose around a pretty-printed fenced array, Then the array is still found
    answer("All done.\n```json\n[\n  {\"description\": \"open\", \"uiAffecting\": false, \"passed\": true}\n]\n```\nBye.", 0);
    assert_eq!(runbook(&[]).stdout, "demo.md: pass\n");

    // Given --steps-file holding the steps and an agent that answers in prose, Then the file wins
    answer("Everything went fine, no JSON here.", 0);
    let file = w.path("steps.json");
    std::fs::write(&file, steps(&[step("open", false, true)])).unwrap();
    let r = runbook(&["--steps-file", &file.display().to_string()]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "demo.md: pass\n"));
    assert!(w.prompts()[0].contains(&format!("write the same JSON array to {} ", file.display())));

    // Given no parseable array, Then it is rejected and the full transcript is written
    answer("I could not do it.", 0);
    let r = runbook(&[]);
    assert_eq!(r.exit_code, 1);
    assert!(Regex::new(r"^demo\.md: rejected -- the agent did not report a parseable JSON step-results array \(finalText or \.agents/tmp/runbook-steps-demo\.md-[\w-]+\.json\); full transcript written to \.agents/tmp/runbook-demo\.md-[\w-]+\.md\n$").unwrap().is_match(&r.stdout), "{}", r.stdout);
    let written: Vec<String> = std::fs::read_dir(w.path(".agents/tmp")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|f| f.starts_with("runbook-demo.md-")).collect();
    assert_eq!(written.len(), 1);
    assert!(std::fs::read_to_string(w.path(".agents/tmp").join(&written[0])).unwrap().contains("I could not do it."));

    // Given an unknown driver, a runbook that does not exist, or every rung failing, Then it fails saying which
    let r = runbook(&["--runbook-cli", "codex"]);
    assert!(r.exit_code == 1 && r.stderr.contains("--runbook-cli must be claude or opencode, got \"codex\""), "{}", r.stderr);
    let r = w.ask(&["--runbook", "nope"], &[], None);
    assert!(r.exit_code != 0 && format!("{}{}", r.stdout, r.stderr).contains("nope.md"));
    answer("", 1);
    let r = runbook(&[]);
    assert!(r.exit_code != 0 && format!("{}{}", r.stdout, r.stderr).contains("opencode dispatch failed after"));
}

fn stats_row(over: Value) -> Value {
    let mut out = json!({ "cwd": "/r", "attempts": [{ "model": "x", "capped": false, "reason": "ok" }], "promptSha256": "x", "promptChars": 10, "promptHead": "h", "toolCalls": 1, "reason": "ok", "escalated": false });
    for (key, value) in over.as_object().unwrap() {
        out[key] = value.clone();
    }
    out
}

fn stats_rows() -> Vec<Value> {
    vec![
        stats_row(json!({
            "ts": "2026-10-01T10:00:00.000Z", "runId": "a", "caller": "dispatch", "claudeSessionId": "s1", "model": "minimax/MiniMax-M3", "ok": true, "durationMs": 8000,
            "tokens": { "input": 100, "output": 50, "reasoning": 0, "cacheRead": 0, "cacheWrite": 0, "total": 150 },
            "cost": 0.0125, "outputChars": 2000, "summaryChars": 200, "wroteNothing": false, "shape": "implementer:unit", "tier": "unit", "pool": "minimax",
        })),
        stats_row(json!({
            "ts": "2026-10-02T10:00:00.000Z", "runId": "b", "caller": "work", "claudeSessionId": "s2", "ok": false,
            "attempts": [{ "model": "a", "capped": true, "reason": "cap" }, { "model": "b", "capped": false, "reason": "x" }],
            "durationMs": 31000, "outputChars": 0, "summaryChars": 0, "wroteNothing": true,
        })),
        stats_row(json!({
            "ts": "2026-10-03T10:00:00.000Z", "runId": "c", "caller": "dispatch", "model": "kimi-code-plan-global/k3", "ok": true, "attempts": [], "durationMs": 15000,
            "cost": 0.0625, "outputChars": 1000, "summaryChars": 1500, "wroteNothing": false, "shape": "judge:text", "tier": "unit", "pool": "kimi",
        })),
    ]
}

const HEADER: &str = "group                                     runs     ok wrote0     p50    tokens      cost   rot  ctx-cut";
const FOOTER: &str = "ctx-cut = share of dispatched output a --summary caller never had to read (outputChars vs summaryChars).";

fn in_dir(cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> RunResult {
    let mut opts = RunOpts::default().cwd(cwd).clean_env();
    for (key, value) in env {
        opts = opts.env(key, value);
    }
    run_wf(args, &opts)
}

#[test]
fn ask_stats_summarize_the_rca_services_run_log_or_a_file() {
    let dir = tempfile::Builder::new().prefix("wf-flow-ask-stats-").tempdir().unwrap();
    let log = dir.path().join("runs.jsonl");
    std::fs::write(&log, format!("{}\n{{torn\n\n", stats_rows().iter().map(Value::to_string).collect::<Vec<_>>().join("\n"))).unwrap();
    let log = log.display().to_string();
    let stats = |args: &[&str]| {
        let mut full = vec!["offload", "ask", "--stats"];
        full.extend_from_slice(args);
        in_dir(dir.path(), &full, &[])
    };
    let runs = |args: &[&str]| {
        let mut full = vec!["--log", log.as_str()];
        full.extend_from_slice(args);
        stats(&full).stdout.lines().next().unwrap_or_default().to_string()
    };

    // Given a run log with a torn line, Then the table groups the runs by caller, model, Claude session and shape
    let r = stats(&["--log", &log]);
    assert_eq!(r.exit_code, 0);
    assert_eq!(
        r.stdout,
        [
            format!("{log} (3 run(s))"),
            "".into(),
            HEADER.into(),
            "all                                          3    67%      1     15s       150    $0.075     1      43%".into(),
            "".into(),
            "# by caller".into(),
            "dispatch                                     2   100%      0     15s       150    $0.075     0      43%".into(),
            "work                                         1     0%      1     31s         0    $0.000     1      n/a".into(),
            "".into(),
            "# by model".into(),
            "minimax/MiniMax-M3                           1   100%      0      8s       150    $0.013     0      90%".into(),
            "(failed)                                     1     0%      1     31s         0    $0.000     1      n/a".into(),
            "kimi-code-plan-global/k3                     1   100%      0     15s         0    $0.063     0     -50%".into(),
            "".into(),
            "# by Claude session".into(),
            "s1                                           1   100%      0      8s       150    $0.013     0      90%".into(),
            "s2                                           1     0%      1     31s         0    $0.000     1      n/a".into(),
            "(no session id)                              1   100%      0     15s         0    $0.063     0     -50%".into(),
            "".into(),
            "# by shape|tier|model|pool".into(),
            "implementer:unit|unit|minimax/MiniMax-M3|minimax     1   100%      0      8s       150    $0.013     0      90%".into(),
            "unknown|unknown|(failed)|unknown             1     0%      1     31s         0    $0.000     1      n/a".into(),
            "judge:text|unit|kimi-code-plan-global/k3|kimi     1   100%      0     15s         0    $0.063     0     -50%".into(),
            "".into(),
            FOOTER.into(),
            "".into(),
        ]
        .join("\n")
    );

    // Given --json, Then the log path and the full stats print with JavaScript number formatting
    let r = stats(&["--log", &log, "--json"]);
    let parsed: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(parsed.as_object().unwrap().keys().collect::<Vec<_>>(), ["logPath", "total", "byCaller", "byModel", "bySession", "byShapeModel"]);
    assert_eq!(parsed["logPath"], json!(log));
    assert_eq!(
        parsed["total"],
        json!({ "key": "all", "runs": 3, "ok": 2, "wroteNothing": 1, "p50DurationMs": 15000, "totalTokens": 150, "totalCost": 0.075, "totalNotionalCost": null, "unpricedRuns": 3, "outputChars": 3000, "summaryChars": 1700, "rotations": 1 })
    );
    assert!(r.stdout.contains("\n  \"total\": {\n    \"key\": \"all\",\n    \"runs\": 3,") && r.stdout.contains("\"p50DurationMs\": 15000,") && !r.stdout.contains("15000.0") && r.stdout.ends_with("\n  ]\n}\n"));

    // Given --since, --session and --caller, Then each narrows the runs and they combine
    assert!(runs(&["--since", "2026-10-02"]).starts_with(&format!("{log} (2 run(s))")));
    assert!(runs(&["--since", "2026-10-02T10:00:00Z"]).starts_with(&format!("{log} (2 run(s))")));
    assert!(runs(&["--since", "2026-10-02T10:00:00.001Z"]).starts_with(&format!("{log} (1 run(s))")));
    assert!(runs(&["--since", "1d"]).starts_with(&format!("{log} (0 run(s))")));
    assert!(runs(&["--since", "100000d"]).starts_with(&format!("{log} (3 run(s))")));
    assert!(runs(&["--session", "s2"]).starts_with(&format!("{log} (1 run(s))")));
    assert!(runs(&["--caller", "dispatch"]).starts_with(&format!("{log} (2 run(s))")));
    assert!(runs(&["--caller", "dispatch", "--session", "s2"]).starts_with(&format!("{log} (0 run(s))")));
    let r = stats(&["--log", &log, "--since", "nonsense"]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, ""));
    assert!(r.stderr.contains("--since must be like 7d, 24h, 90m or an ISO date, got \"nonsense\""));

    // Given a missing or an empty log, Then zero runs and empty groups print
    let missing = dir.path().join("missing.jsonl").display().to_string();
    assert_eq!(stats(&["--log", &missing]).stdout, format!("{missing} (0 run(s))\n\n{HEADER}\nall                                          0     0%      0      0s         0    $0.000     0      n/a\n\n{FOOTER}\n"));
    let empty = dir.path().join("empty.jsonl");
    std::fs::write(&empty, "").unwrap();
    let r = stats(&["--log", &empty.display().to_string(), "--json"]);
    assert!(r.stdout.contains("\"byCaller\": [],") && r.stdout.contains("\"byShapeModel\": []\n}"), "{}", r.stdout);

    // Given no --log, Then the run log is the rca service's, fetched with the secret; with no service it fails naming the next step
    let rca = Rca::start();
    rca.server.routes(&[("GET /runs/log", MockReply::json(200, json!({ "log": std::fs::read_to_string(&log).unwrap() })))]);
    let r = in_dir(dir.path(), &["offload", "ask", "--stats"], &rca.env());
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stdout.starts_with("the rca service's run log (3 run(s))\n"), "{}", r.stdout);
    assert_eq!(rca.server.requests().iter().map(|q| (q.method.as_str(), q.url.as_str(), q.headers.get("x-bot-secret").map(String::as_str))).collect::<Vec<_>>(), [("GET", "/runs/log", Some("s3cret"))]);
    let down = [("CHAT_AGENT_API_SECRET", "s3cret"), ("RCA_HOST", "127.0.0.1"), ("RCA_PORT", "1"), ("WF_TAILNET_SOCKS", "")];
    let r = in_dir(dir.path(), &["offload", "ask", "--stats"], &down);
    assert!(r.exit_code == 1 && r.stderr.contains("unreachable"), "{}{}", r.stdout, r.stderr);
}

fn savings_row(over: Value) -> Value {
    let mut base = json!({ "ts": "2026-10-01T10:00:00.000Z", "runId": "r", "caller": "dispatch", "ok": true, "cost": 0.01, "promptChars": 100, "durationMs": 5000 });
    for (key, value) in over.as_object().unwrap() {
        if value.is_null() {
            base.as_object_mut().unwrap().remove(key);
        } else {
            base[key] = value.clone();
        }
    }
    base
}

const RATIOS_LINE: &str = "ratios: docs/audits/2026-09-02-cheap-model-delegation-report.md, benchmark B1 (arm A Sonnet, gate-passing runs, 7 tasks x n=3), measured 2026-09-03. Estimate: size from prompt length x arm-A mean per size.";

#[test]
fn ask_savings_prices_what_delegating_avoided_from_the_run_log_and_the_delegation_report() {
    let dir = tempfile::Builder::new().prefix("wf-flow-ask-savings-").tempdir().unwrap();
    let (log, report) = (dir.path().join("runs.jsonl"), dir.path().join("report.jsonl"));
    let rows = [
        savings_row(json!({})),
        savings_row(json!({ "promptChars": 500, "cost": 0.02, "durationMs": 7000 })),
        savings_row(json!({ "promptChars": 5000, "cost": 0.05, "durationMs": 20_000, "ts": "2026-10-03T10:00:00.000Z" })),
        savings_row(json!({ "ok": false, "cost": 0.5 })),
        savings_row(json!({ "cost": null, "tokens": null })),
        savings_row(json!({ "caller": "mcp", "cost": 0.03, "promptChars": 50 })),
        savings_row(json!({ "caller": "work", "cost": 1 })),
    ];
    std::fs::write(&log, format!("{}\n{{torn\n", rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"))).unwrap();
    let sessions = [
        json!({ "ts": "2026-10-01T10:00:00.000Z", "sessionId": "a", "delegations": 2, "directWork": 5 }),
        json!({ "ts": "2026-10-04T10:00:00.000Z", "sessionId": "a", "delegations": 3, "directWork": 6 }),
        json!({ "ts": "2026-10-02T10:00:00.000Z", "sessionId": "b", "delegations": 1, "directWork": 1 }),
        json!({ "ts": "2026-09-01T10:00:00.000Z", "delegations": 9, "directWork": 9 }),
    ];
    std::fs::write(&report, format!("{}\n", sessions.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"))).unwrap();
    let (log, report) = (log.display().to_string(), report.display().to_string());
    let savings = |args: &[&str]| {
        let mut full = vec!["offload", "ask", "--savings", "--log", log.as_str()];
        full.extend_from_slice(args);
        in_dir(dir.path(), &full, &[])
    };

    // Given a log and a report, Then only dispatch rows are summed by prompt size (the option's default reaches the window filter), and sessions are reported
    let r = savings(&["--report", &report]);
    assert_eq!(r.exit_code, 0);
    assert_eq!(
        r.stdout,
        [
            format!("{log} (5 run(s) in window)").as_str(),
            "",
            "delegated runs: 3 ok, 1 failed, 1 unpriced (no tokens: mock/test rows) | opencode $0.080 in 32s | Claude would have been ~$1.026 in ~307s",
            "saved ~$0.946 (92%), wall-clock -275s",
            "",
            "size     runs  opencode$  claude$(est)  opencode-s  claude-s(est)",
            "small       1     $0.010        $0.160          5s            42s",
            "medium      1     $0.020        $0.249          7s            72s",
            "large       1     $0.050        $0.617         20s           193s",
            "",
            "sessions: 3 | delegations 13 vs direct read/edit/write/grep 16 | delegation share 45%",
            "",
            RATIOS_LINE,
            "",
        ]
        .join("\n")
    );

    // Given --caller mcp, or --since, Then those runs count instead
    let r = savings(&["--report", &report, "--caller", "mcp"]);
    assert!(r.stdout.starts_with(&format!("{log} (1 run(s) in window)\n")) && r.stdout.contains("delegated runs: 1 ok, 0 failed, 0 unpriced") && r.stdout.contains("small       1     $0.030"), "{}", r.stdout);
    let r = savings(&["--report", &report, "--since", "2026-10-02"]);
    assert!(r.stdout.starts_with(&format!("{log} (1 run(s) in window)\n")) && r.stdout.contains("sessions: 2 | delegations 4 vs direct read/edit/write/grep 7"), "{}", r.stdout);

    // Given no report file, Then the sessions line is left out
    let r = savings(&["--report", &dir.path().join("nope.jsonl").display().to_string()]);
    assert!(r.exit_code == 0 && !r.stdout.contains("sessions:"));

    // Given --json, Then the log and report paths lead the savings object, seconds as whole numbers
    let r = savings(&["--report", &report, "--json"]);
    let parsed: Map<String, Value> = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(parsed.keys().take(5).collect::<Vec<_>>(), ["logPath", "reportPath", "ratios", "runs", "failedRuns"]);
    assert_eq!((parsed["runs"].clone(), parsed["failedRuns"].clone(), parsed["unpricedRuns"].clone()), (3.into(), 1.into(), 1.into()));
    assert_eq!((parsed["sessions"]["count"].clone(), parsed["sessions"]["delegations"].clone(), parsed["sessions"]["directWork"].clone()), (3.into(), 13.into(), 16.into()));
    assert_eq!(parsed["bySize"].as_array().unwrap().iter().map(|s| format!("{}:{}", s["size"].as_str().unwrap(), s["runs"])).collect::<Vec<_>>(), ["small:1", "medium:1", "large:1"]);
    assert!(r.stdout.contains("\"opencodeSeconds\": 32,") && !r.stdout.contains("32.0"));

    // Given a ratios file, Then its table and its borrowed marker are used; one where Claude is cheaper makes the saving negative
    let ratios = dir.path().join("ratios.json");
    std::fs::write(
        &ratios,
        json!({ "source": "custom table", "measuredAt": "2026-01-01", "promptCharsBuckets": { "small": 200, "medium": 1000 }, "sizes": { "small": { "claudeUsd": 0.5, "claudeSeconds": 100, "n": 4 }, "medium": { "claudeUsd": 0.0005, "claudeSeconds": 0.4, "n": 1 }, "large": { "claudeUsd": 1, "claudeSeconds": 10, "n": 0 } } }).to_string(),
    )
    .unwrap();
    let r = savings(&["--report", &report, "--ratios", &ratios.display().to_string()]);
    assert!(r.exit_code == 0 && r.stdout.contains("ratio borrowed, size unmeasured") && r.stdout.contains("ratios: custom table, measured 2026-01-01."), "{}", r.stdout);
    let cheap = dir.path().join("cheap.json");
    let size = json!({ "claudeUsd": 0.0001, "claudeSeconds": 0.2, "n": 1 });
    std::fs::write(&cheap, json!({ "source": "s", "measuredAt": "m", "promptCharsBuckets": { "small": 200, "medium": 1000 }, "sizes": { "small": size, "medium": size, "large": size } }).to_string()).unwrap();
    let r = savings(&["--report", &report, "--ratios", &cheap.display().to_string()]);
    assert!(r.stdout.contains("saved ~$-0.") && Regex::new(r" \(-\d+%\)").unwrap().is_match(&r.stdout), "{}", r.stdout);

    // Given an empty window, Then zeros print and the share is 0%
    let missing = dir.path().join("missing.jsonl").display().to_string();
    let r = in_dir(dir.path(), &["offload", "ask", "--savings", "--log", &missing, "--report", &report], &[]);
    assert!(r.stdout.contains("(0 run(s) in window)") && r.stdout.contains("delegated runs: 0 ok, 0 failed, 0 unpriced") && r.stdout.contains("saved ~$0.000 (0%), wall-clock +0s"), "{}", r.stdout);

    // Given an unreadable ratios file or an unparseable --since, Then it fails, naming the allowed forms
    assert_ne!(savings(&["--ratios", &dir.path().join("absent.json").display().to_string()]).exit_code, 0);
    let r = savings(&["--since", "nonsense"]);
    assert!(r.exit_code == 1 && r.stderr.contains("--since must be like 7d, 24h, 90m or an ISO date, got \"nonsense\""), "{}", r.stderr);
}

const LADDER: &str = "libraries/rca-core/plugins/ladder/data/model-ladders.json";
const PRICING: &str = "libraries/rca-core/data/model-pricing.json";
const OLD_PRICING: &str = "{\n  \"fetchedAt\": \"2020-01-01T00:00:00.000Z\"\n}\n";

fn price(input: Value, output: Value, read: Value, write: Value) -> Value {
    json!({ "inputPerMTok": input, "outputPerMTok": output, "cacheReadPerMTok": read, "cacheWritePerMTok": write })
}

#[test]
fn clearing_a_cooldown_goes_through_the_rca_service_and_reports_whether_the_pool_was_cold() {
    // Given the rca service, answering that kimi was cold and minimax was not
    let dir = tempfile::Builder::new().prefix("wf-flow-ladder-").tempdir().unwrap();
    let rca = Rca::start();
    rca.server.routes(&[("POST /runs/cooldowns/clear", MockReply::json(200, json!({ "pool": "kimi", "wasCold": true })))]);

    // When the cooldown is cleared through the ladder tools, Then it says so and sent the pool under the secret
    let r = in_dir(dir.path(), &["offload", "ladder", "clear-cooldown", "kimi"], &rca.env());
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "kimi: cold mark cleared\n"), "{}", r.stderr);
    let seen = rca.server.requests();
    assert_eq!(seen.len(), 1);
    assert!(seen.iter().all(|q| q.method == "POST" && q.url == "/runs/cooldowns/clear" && q.headers.get("x-bot-secret").map(String::as_str) == Some("s3cret") && serde_json::from_str::<Value>(&q.body).unwrap() == json!({ "pool": "kimi" })));

    // Given a warm pool, Then it says so
    rca.server.clear();
    rca.server.routes(&[("POST /runs/cooldowns/clear", MockReply::json(200, json!({ "pool": "minimax", "wasCold": false })))]);
    assert_eq!(in_dir(dir.path(), &["offload", "ladder", "clear-cooldown", "minimax"], &rca.env()).stdout, "minimax: already warm, counters cleared\n");

    // Given the service refuses, Then it exits 1 with the reason
    rca.server.clear();
    rca.server.routes(&[("POST /runs/cooldowns/clear", MockReply::json(503, json!({ "error": "the pool cooldowns could not be read: ORDS 503", "code": "LEDGER_UNAVAILABLE" })))]);
    let r = in_dir(dir.path(), &["offload", "ladder", "clear-cooldown", "kimi"], &rca.env());
    assert!(r.exit_code == 1 && r.stderr.contains("the pool cooldowns could not be read"), "{}{}", r.stdout, r.stderr);
}

#[test]
fn the_ladder_refreshes_model_prices_from_openrouter() {
    let dir = tempfile::Builder::new().prefix("wf-flow-ladder-").tempdir().unwrap();
    let root = dir.path();
    // Given OpenRouter's per-token prices, When the prices are synced, Then each ladder model is written per MTok in ladder order
    let openrouter = MockServer::start(None);
    let url = format!("http://127.0.0.1:{}/api/v1/models", openrouter.port);
    for file in [LADDER, PRICING] {
        std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
    }
    let sync = |models: &[&str], status: u16, listing: Value| {
        let map: Map<String, Value> = models.iter().map(|m| (m.to_string(), json!({}))).collect();
        std::fs::write(root.join(LADDER), json!({ "models": map }).to_string()).unwrap();
        std::fs::write(root.join(PRICING), OLD_PRICING).unwrap();
        openrouter.clear();
        openrouter.routes(&[("GET /api/v1/models", MockReply::json(status, listing))]);
        in_dir(root, &["offload", "ladder", "sync-pricing"], &[("WF_OPENROUTER_MODELS_URL", &url)])
    };
    let written = || serde_json::from_str::<Value>(&std::fs::read_to_string(root.join(PRICING)).unwrap()).unwrap();
    let r = sync(
        &["minimax/MiniMax-M3", "opencode-go/deepseek-v4.1-flash", "kimi-code-plan-global/k3"],
        200,
        json!({ "data": [
            { "id": "moonshotai/kimi-k3", "pricing": { "prompt": "0.000002648138063", "completion": "0.00001328272425", "input_cache_read": "0.00000030264435" } },
            { "id": "minimax/minimax-m3", "pricing": { "prompt": "0.0000003", "completion": "0.0000012", "input_cache_read": "0.00000006" } },
            { "id": "deepseek/deepseek-v4.1-flash", "pricing": { "prompt": "0.00000006566", "completion": "0.00000013132", "input_cache_write": "0.0000001" } },
        ] }),
    );
    assert_eq!(r.exit_code, 0);
    assert_eq!(openrouter.requests().iter().map(|q| format!("{} {}", q.method, q.url)).collect::<Vec<_>>(), ["GET /api/v1/models"]);
    let file = written();
    assert_eq!(file.as_object().unwrap().keys().collect::<Vec<_>>(), ["fetchedAt", "minimax/MiniMax-M3", "opencode-go/deepseek-v4.1-flash", "kimi-code-plan-global/k3"]);
    assert!(Regex::new(r"^\d{4}-\d\d-\d\dT[\d:.]+Z$").unwrap().is_match(file["fetchedAt"].as_str().unwrap()));
    assert_eq!(file["minimax/MiniMax-M3"], price(json!(0.3), json!(1.2), json!(0.06), Value::Null));
    assert_eq!(file["opencode-go/deepseek-v4.1-flash"], price(json!(0.06566), json!(0.13132), Value::Null, json!(0.1)));
    assert_eq!(file["kimi-code-plan-global/k3"], price(json!(2.648138063), json!(13.28272425), json!(0.30264435), Value::Null));
    assert_eq!(r.stdout, format!("wrote model-pricing.json: 3 ladder model(s), 0 unresolved (unknown), fetchedAt {}\n", file["fetchedAt"].as_str().unwrap()));
    assert!(!root.join(format!("{PRICING}.tmp")).exists());

    // and the file is two-space JSON with whole numbers bare and a final newline
    sync(&["claude-haiku-4-5"], 200, json!({ "data": [{ "id": "anthropic/claude-haiku-4.5", "pricing": { "prompt": "0.000001", "completion": "0.000005", "input_cache_read": "0.0000001", "input_cache_write": "0.00000125" } }] }));
    let text = std::fs::read_to_string(root.join(PRICING)).unwrap();
    assert_eq!(
        Regex::new("\"fetchedAt\": \"[^\"]+\"").unwrap().replace(&text, "\"fetchedAt\": \"T\""),
        ["{", "  \"fetchedAt\": \"T\",", "  \"claude-haiku-4-5\": {", "    \"inputPerMTok\": 1,", "    \"outputPerMTok\": 5,", "    \"cacheReadPerMTok\": 0.1,", "    \"cacheWritePerMTok\": 1.25", "  }", "}", ""].join("\n")
    );

    // Given a model with no mapping or no listing, or a rate that is not a number, Then it is written unknown and counted, or unpublished
    let r = sync(&["no/mapping", "minimax/MiniMax-M3", "claude-opus-5"], 200, json!({ "data": [{ "id": "minimax/minimax-m3", "pricing": { "prompt": "0.0000003" } }] }));
    let unknown = json!({ "inputPerMTok": 0, "outputPerMTok": 0, "cacheReadPerMTok": null, "cacheWritePerMTok": null, "unknown": true });
    let file = written();
    assert_eq!((file["no/mapping"].clone(), file["claude-opus-5"].clone(), file["minimax/MiniMax-M3"].clone()), (unknown.clone(), unknown, price(json!(0.3), json!(0), Value::Null, Value::Null)));
    assert!(Regex::new(r"^wrote model-pricing\.json: 3 ladder model\(s\), 2 unresolved \(unknown\), fetchedAt ").unwrap().is_match(&r.stdout), "{}", r.stdout);
    sync(&["minimax/MiniMax-M3"], 200, json!({ "data": [{ "id": "minimax/minimax-m3", "pricing": { "prompt": "free", "completion": "0.0000012", "input_cache_read": "" } }] }));
    assert_eq!(written()["minimax/MiniMax-M3"], price(json!(0), json!(1.2), json!(0), Value::Null));

    // Given a listing without data, Then every model is unknown; Given an HTTP error, Then it fails and leaves the file alone
    assert!(sync(&["minimax/MiniMax-M3"], 200, json!({})).stdout.contains("1 ladder model(s), 1 unresolved (unknown)"));
    let r = sync(&["minimax/MiniMax-M3"], 500, json!({ "error": "boom" }));
    assert!(r.exit_code != 0 && format!("{}{}", r.stdout, r.stderr).contains("HTTP 500"));
    assert_eq!((std::fs::read_to_string(root.join(PRICING)).unwrap().as_str(), openrouter.requests().len()), (OLD_PRICING, 1));
}

#[test]
fn a_session_agent_variant_and_title_go_to_the_rca_service_and_the_session_is_remembered_for_the_next_turn() {
    // Given a repository with an opencode agent definition, and a service that answers with a session and its token usage
    let w = Work::new("ask-session-");
    w.put(".opencode/agents/researcher.md", "---\ndescription: reads\n---\nResearch.");
    w.put("notes.md", "n");
    std::fs::create_dir(w.path(".git")).unwrap();
    let rca = Rca::start();
    let attempt = json!({ "index": 0, "choice": { "model": M2 }, "verdict": { "ok": true } });
    rca.answer(MockReply::json(200, json!({ "ok": true, "output": { "text": "first", "toolEvents": [], "structured": true }, "attempt": attempt, "attempts": [attempt], "session": "ses_abc", "usage": { "input": 100.0, "output": 50.0, "reasoning": 0.0, "cacheRead": 0.0, "cacheWrite": 0.0 } })));

    // When a first turn names an agent, a variant and a bare title, and a summary is asked for
    let r = w.ask(&["look at it", "--files", "notes.md", "--agent", "researcher", "--variant", "high", "--title", "--summary=brief"], &rca.env(), None);

    // Then the request carries them (the agent's definition attached from the repository), the answer prints the session and the tokens,
    // and the session is saved
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let body = &rca.bodies()[0];
    assert_eq!((body["agent"].clone(), body["variant"].clone(), body["title"].clone(), body["auto"].clone()), (json!("researcher"), json!("high"), json!("\"look at it\""), json!(false)));
    assert_eq!(body["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect::<Vec<_>>(), ["notes.md", ".opencode/agents/researcher.md"]);
    assert!(r.stderr.contains("session ses_abc") && r.stderr.contains("tokens=150"), "{}", r.stderr);
    assert_eq!(std::fs::read_to_string(w.path(".agents/tmp/rca-last-session")).unwrap(), "ses_abc");

    // When the next turn continues the last session, and another names one
    let r = w.ask(&["and more", "--continue"], &rca.env(), None);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let r2 = w.ask(&["again", "-s", "ses_other", "-f", "notes.md"], &rca.env(), None);
    assert_eq!(r2.exit_code, 0, "{}", r2.stderr);

    // Then the first sent the saved session, the second the named one with its file
    let bodies = rca.bodies();
    assert_eq!(bodies[1]["session"], json!("ses_abc"));
    assert_eq!(bodies[2]["session"], json!("ses_other"));
    assert_eq!(bodies[2]["files"][0], json!({ "path": "notes.md", "content": "n" }));

    // And continuing where nothing was saved starts a new session, as `opencode run --continue` does
    std::fs::remove_file(w.path(".agents/tmp/rca-last-session")).unwrap();
    let before = rca.bodies().len();
    let r = w.ask(&["x", "-c"], &rca.env(), None);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(rca.bodies().len(), before + 1);
    assert!(rca.bodies().last().unwrap().get("session").is_none());
}

/// `fixtures/opencode-run-options.json`: every option of `opencode run --help` (1.19.0) and what `wf offload ask` does with it.
fn opencode_options() -> Vec<Value> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/flow/fixtures/opencode-run-options.json")).unwrap();
    serde_json::from_str::<Value>(&text).unwrap()["options"].as_array().unwrap().clone()
}

#[test]
fn every_option_of_opencode_run_is_either_the_same_in_ask_refused_with_a_reason_or_not_applicable() {
    // Given opencode run's options as recorded, a repository with a saved session, and the rca service answering with a session
    let options = opencode_options();
    let w = Work::new("ask-options-");
    std::fs::create_dir(w.path(".git")).unwrap();
    w.put("a.md", "A");
    w.put("b.md", "B");
    w.put(".agents/tmp/rca-last-session", "ses_saved");
    let rca = Rca::start();
    let attempt = json!({ "index": 0, "choice": { "model": "p/m" }, "verdict": { "ok": true } });
    rca.answer(MockReply::json(200, json!({ "ok": true, "output": { "text": "answer", "toolEvents": [], "structured": true }, "attempt": attempt, "attempts": [attempt], "session": "ses_new", "events": turn("ses_new", "answer") })));
    let ask = |args: &[&str]| w.ask(args, &rca.env(), None);
    let last_body = || rca.bodies().last().cloned().unwrap();

    // Then the options marked the same behave as opencode's: the long form and the short form where opencode has one
    type Journey<'a> = (&'a str, Vec<&'a str>, Box<dyn Fn(&Value) -> bool>);
    let same: Vec<Journey> = vec![
        ("continue", vec!["-c"], Box::new(|b| b["session"] == json!("ses_saved"))),
        ("session", vec!["-s", "ses_x"], Box::new(|b| b["session"] == json!("ses_x"))),
        ("model", vec!["-m", "p/m1", "-m", "p/m2"], Box::new(|b| b["models"] == json!(["p/m1", "p/m2"]))),
        ("agent", vec!["--agent", "reviewer"], Box::new(|b| b["agent"] == json!("reviewer"))),
        ("file", vec!["-f", "a.md", "--file", "b.md"], Box::new(|b| b["files"].as_array().is_some_and(|f| f.len() == 2 && f[0]["path"] == json!("a.md") && f[1]["path"] == json!("b.md")) && b["attach"] == json!(["a.md", "b.md"]))),
        ("title", vec!["--title", "named"], Box::new(|b| b["title"] == json!("named"))),
        ("variant", vec!["--variant", "max"], Box::new(|b| b["variant"] == json!("max"))),
        ("auto", vec!["--auto"], Box::new(|b| b["auto"] == json!(true))),
        ("command", vec!["--command", "greet"], Box::new(|b| b["command"] == json!("greet"))),
        ("dir", vec!["--dir", "sub", "-f", "c.md"], Box::new(|b| b["files"] == json!([{ "path": "c.md", "content": "C" }]))),
    ];
    w.put("sub/c.md", "C");
    for (long, args, expected) in &same {
        let mut full = vec!["x"];
        full.extend(args.iter());
        let r = ask(&full);
        assert_eq!(r.exit_code, 0, "--{long}: {}{}", r.stdout, r.stderr);
        assert!(expected(&last_body()), "--{long} did not reach the request: {}", last_body());
    }
    // and --format json prints the turn's events one per line and nothing else, the default prints each text part on stdout, and --help works
    let r = ask(&["x", "--format", "json"]);
    let lines: Vec<Value> = r.stdout.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {}", r.stdout))).collect();
    assert_eq!(lines.iter().map(|l| l["type"].as_str().unwrap()).collect::<Vec<_>>(), ["step_start", "text", "step_finish"]);
    assert_eq!((lines[1]["part"]["text"].clone(), lines[1]["sessionID"].clone()), (json!("answer"), json!("ses_new")));
    assert_eq!(ask(&["x"]).stdout, "answer\n");
    let help = in_dir(&w.path("."), &["offload", "ask", "--help"], &[]);
    assert!(help.exit_code == 0 && help.stdout.contains("Usage: wf offload ask"), "{}", help.stdout);

    // And the one opencode option the service cannot do yet is refused with why, before any request
    let before = rca.bodies().len();
    let r = ask(&["x", "--fork", "-c"]);
    assert!(r.exit_code == 2 && r.stderr.contains("--fork is not supported by the rca service"), "{}{}", r.stdout, r.stderr);
    assert_eq!(rca.bodies().len(), before);

    // And every option that is not applicable is rejected, not silently ignored, and sends nothing
    for option in options.iter().filter(|o| o["status"] == json!("not-applicable")) {
        let long = format!("--{}", option["long"].as_str().unwrap());
        let r = ask(&["x", long.as_str()]);
        assert_ne!(r.exit_code, 0, "{long} is marked not applicable but was accepted: {}", r.stdout);
        let short = option["short"].as_str().map(|s| format!("-{s}"));
        if let Some(short) = short.filter(|s| s != "-h") {
            assert_ne!(ask(&["x", short.as_str()]).exit_code, 0, "{short} is marked not applicable but was accepted");
        }
    }
    assert_eq!(rca.bodies().len(), before, "a refused option sent a request");

    // And the table is complete: each status is one of the three, and each option marked the same is exercised above
    let covered: Vec<&str> = same.iter().map(|(long, _, _)| *long).chain(["format", "help"]).collect();
    for option in &options {
        let (long, status) = (option["long"].as_str().unwrap(), option["status"].as_str().unwrap());
        assert!(["same", "unsupported", "not-applicable"].contains(&status), "{long}: unknown status {status}");
        if status == "same" {
            assert!(covered.contains(&long), "--{long} is marked the same but no journey exercises it");
        }
    }
}

#[test]
fn a_turn_prints_as_opencode_run_prints_it_and_the_option_it_does_not_have_is_refused() {
    // Given a service answering a turn with a read, a write, then two text parts
    let w = Work::new("ask-print-");
    std::fs::create_dir(w.path(".git")).unwrap();
    let rca = Rca::start();
    let attempt = json!({ "index": 0, "choice": { "model": "p/m" }, "verdict": { "ok": true } });
    let events = json!([
        { "type": "step_start", "sessionID": "ses_p", "part": { "type": "step-start" } },
        { "type": "text", "sessionID": "ses_p", "part": { "type": "text", "text": "First I read it." } },
        { "type": "tool_use", "sessionID": "ses_p", "part": { "type": "tool", "tool": "read", "state": { "status": "completed", "title": "a.txt", "input": {}, "output": "x" } } },
        { "type": "tool_use", "sessionID": "ses_p", "part": { "type": "tool", "tool": "write", "state": { "status": "completed", "title": "b.txt", "input": {}, "output": "Wrote file successfully." } } },
        { "type": "text", "sessionID": "ses_p", "part": { "type": "text", "text": "DONE" } },
    ]);
    rca.answer(MockReply::json(200, json!({ "ok": true, "output": { "text": "First I read it.DONE", "toolEvents": [], "structured": true }, "attempt": attempt, "attempts": [attempt], "session": "ses_p", "events": events })));

    // When it is asked with the default format
    let r = w.ask(&["go"], &rca.env(), None);

    // Then stdout is each text part and a newline, and the header and the tool trace are on stderr as opencode run prints them
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "First I read it.\nDONE\n"), "{}", r.stderr);
    assert!(r.stderr.contains("\x1b[0m\n> build · m\n\x1b[0m\n\x1b[0m→ \x1b[0mRead a.txt\n\x1b[0m\n\x1b[0m← \x1b[0mWrite b.txt\nWrote file successfully.\n\x1b[0m\n"), "{:?}", r.stderr);

    // And an option opencode run does not have is refused with the way to what it was for, before any request
    let before = rca.bodies().len();
    let refused = w.ask(&["go", "--no-auto"], &rca.env(), None);
    assert!(refused.exit_code == 2 && refused.stderr.contains("--agent read-only") && rca.bodies().len() == before, "{}", refused.stderr);
}

#[test]
fn the_projects_opencode_config_instructions_agents_skills_and_commands_go_with_the_run_without_its_local_mcp_servers() {
    // Given a project with a config (a local and a remote MCP server, instructions by path and by glob), AGENTS.md, an agent, a skill, a
    // command, a plugin, a tool, and things that stay behind: node_modules, a file over 512 KB, the opencode binary directory
    let w = Work::new("ask-project-");
    std::fs::create_dir(w.path(".git")).unwrap();
    w.put("opencode.json", &json!({ "mcp": { "installs": { "type": "local", "command": ["npx", "-y", "some-mcp"] }, "script": { "type": "local", "command": ["node", "./tools/mcp.js", "--flag"] }, "remote-one": { "type": "remote", "url": "https://m.example/mcp" } }, "agent": { "writer": { "prompt": "{file:./prompts/writer.md}" } }, "instructions": ["docs/rules.md", "rules/*.md", "missing.md", "/abs.md", "../up.md"], "permission": { "bash": { "rm *": "deny" } } }).to_string());
    w.put("AGENTS.md", "agents rules");
    w.put("docs/rules.md", "by path");
    w.put("rules/one.md", "by glob");
    w.put("rules/deep/two.md", "not matched by a single star");
    w.put(".opencode/agent/a.md", "agent");
    w.put(".opencode/skill/s/SKILL.md", "skill");
    w.put(".opencode/command/c.md", "command");
    w.put(".opencode/plugin/p.ts", "plugin");
    w.put(".opencode/tool/t.ts", "tool");
    w.put(".opencode/skill/s/node_modules/dep/index.js", "dependency");
    w.put(".opencode/bin/opencode", "binary");
    w.put(".claude/skills/c/SKILL.md", "claude skill");
    w.put(".agents/skills/d/SKILL.md", "agents skill");
    w.put("prompts/writer.md", "a prompt named by the config");
    w.put("tools/mcp.js", "a local mcp server");
    w.put(".opencode/command/ref.md", "Review @docs/ref.md and `@docs/other.md`, not @missing.md or an email a@b.c");
    w.put("docs/ref.md", "referenced");
    w.put("docs/other.md", "referenced too");
    w.put("CLAUDE.md", "claude rules, left behind because AGENTS.md exists");
    w.put(".opencode/agent/huge.md", &"x".repeat(600 * 1024));
    let rca = Rca::start();
    let attempt = json!({ "index": 0, "choice": { "model": "p/m" }, "verdict": { "ok": true } });
    rca.answer(MockReply::json(200, json!({ "ok": true, "output": { "text": "a", "toolEvents": [], "structured": true }, "attempt": attempt, "attempts": [attempt], "events": turn("s", "a") })));

    // When it is asked, with one of those files also named by the caller
    let r = w.ask(&["go", "--files", "AGENTS.md"], &rca.env(), None);

    // Then the service got each project file once, under its own path, and the config without the local server
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let body = rca.requests_with_config().remove(0);
    let files = body["files"].as_array().unwrap();
    let mut paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    paths.sort();
    assert_eq!(paths, [".agents/skills/d/SKILL.md", ".claude/skills/c/SKILL.md", ".opencode/agent/a.md", ".opencode/command/c.md", ".opencode/command/ref.md", ".opencode/plugin/p.ts", ".opencode/skill/s/SKILL.md", ".opencode/tool/t.ts", "AGENTS.md", "docs/other.md", "docs/ref.md", "docs/rules.md", "opencode.json", "prompts/writer.md", "rules/one.md", "tools/mcp.js"].iter().copied().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>());
    let config: Value = serde_json::from_str(files.iter().find(|f| f["path"] == "opencode.json").unwrap()["content"].as_str().unwrap()).unwrap();
    assert_eq!(config["mcp"], json!({ "script": { "type": "local", "command": ["node", "./tools/mcp.js", "--flag"] }, "remote-one": { "type": "remote", "url": "https://m.example/mcp" } }));
    assert_eq!(config["permission"], json!({ "bash": { "rm *": "deny" } }));
}

#[test]
fn the_message_is_the_words_given_or_the_pipe_and_a_failed_run_prints_one_error_event_in_json() {
    // Given the rca service answering ok, then failing
    let w = Work::new("ask-message-");
    std::fs::create_dir(w.path(".git")).unwrap();
    let rca = Rca::start();
    let attempt = json!({ "index": 0, "choice": { "model": "p/m" }, "verdict": { "ok": true } });
    rca.answer(MockReply::json(200, json!({ "ok": true, "output": { "text": "a", "toolEvents": [], "structured": true }, "attempt": attempt, "attempts": [attempt], "events": turn("s", "a") })));

    // When the message is several words, or comes from a pipe, as with `opencode run [message..]`
    assert_eq!(w.ask(&["say", "hello", "to", "the", "world"], &rca.env(), None).exit_code, 0);
    assert_eq!(w.ask(&[], &rca.env(), Some("from the pipe\n")).exit_code, 0);
    assert_eq!(w.ask(&["-"], &rca.env(), Some("from the dash")).exit_code, 0);

    // Then the service got each as the prompt
    let prompts: Vec<String> = rca.bodies().iter().map(|b| b["prompt"].as_str().unwrap().to_string()).collect();
    assert_eq!(prompts, ["say hello to the world", "from the pipe", "from the dash"]);

    // And a run that fails with --format json prints an error event on stdout and exits 1, as `opencode run --format json` does
    let failed = json!({ "ok": false, "reason": "ladder-exhausted", "attempts": [{ "index": 0, "choice": { "model": "p/m" }, "verdict": { "ok": false, "failureClass": "crash" } }] });
    rca.answer(MockReply::json(200, failed));
    let r = w.ask(&["x", "--format", "json"], &rca.env(), None);
    let event: Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", r.stdout));
    assert_eq!((r.exit_code, event["type"].clone(), event["error"]["data"]["message"].as_str().is_some_and(|m| m.contains("exhausted"))), (1, json!("error"), true), "{}", r.stdout);
}

#[test]
fn a_caller_drops_the_sessions_it_made_by_name_and_a_missing_name_or_a_refusal_is_said() {
    // Given a service that deletes a caller's sessions and says how many
    let rca = Rca::start();
    rca.server.routes(&[("DELETE /runs/sessions?caller=e2e-one", MockReply::json(200, json!({ "dropped": 2 })))]);
    let run = |args: &[&str], env: &[(&str, &str)]| run_wf(&[&["offload", "sessions"], args].concat(), &env.iter().fold(RunOpts::default(), |opts, (k, v)| opts.env(k, v)));

    // When the caller asks for them to go
    let r = run(&["rm", "--caller", "e2e-one"], &rca.env());
    // Then the service is asked to delete exactly that caller's sessions, and the count is printed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(serde_json::from_str::<Value>(r.stdout.trim()).unwrap(), json!({ "dropped": 2 }));
    assert_eq!(rca.server.requests().iter().filter(|q| q.method == "DELETE").count(), 1);

    // And no caller is refused before any request, and a service that refuses is reported with its answer
    let r = run(&["rm"], &rca.env());
    assert_eq!(r.exit_code, 2, "{}{}", r.stdout, r.stderr);
    rca.server.routes(&[("DELETE /runs/sessions?caller=e2e-two", MockReply::json(401, json!({ "error": "unauthorized" })))]);
    let r = run(&["rm", "--caller", "e2e-two"], &rca.env());
    assert!(r.exit_code == 1 && r.stderr.contains("401") && r.stderr.contains("unauthorized"), "{}{}", r.stdout, r.stderr);
}

#[test]
fn an_operator_mints_lists_and_revokes_device_bearers_with_the_service_secret_and_a_refusal_or_a_missing_label_is_said_as_json() {
    // Given a service that mints a bearer, lists it, and revokes it
    let rca = Rca::start();
    let minted = json!({ "id": "bearer-1a2b", "label": "t3", "createdAt": "2026-11-04T10:00:00.000Z", "token": "rcab_0123456789abcdef" });
    rca.server.routes(&[
        ("POST /agent/v1/bearers", MockReply::json(201, minted.clone())),
        ("GET /agent/v1/bearers", MockReply::json(200, json!({ "bearers": [{ "id": "bearer-1a2b", "label": "t3", "createdAt": "2026-11-04T10:00:00.000Z" }] }))),
        ("DELETE /agent/v1/bearers/bearer-1a2b", MockReply::json(200, json!({ "revoked": "bearer-1a2b" }))),
        ("DELETE /agent/v1/bearers/nope", MockReply::json(404, json!({ "error": "no bearer has the id nope", "code": "NOT_FOUND" }))),
    ]);
    let run = |args: &[&str], env: &[(&str, &str)]| run_wf(&[&["offload", "bearers"], args].concat(), &env.iter().fold(RunOpts::default(), |opts, (k, v)| opts.env(k, v)));
    let line = |r: &RunResult| serde_json::from_str::<Value>(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}{}", r.stdout, r.stderr));

    // When the operator mints one for a label
    let r = run(&["mint", "--label", "t3"], &rca.env());
    // Then the token is printed once with the id and the next step, and the service was asked with the service secret and that label
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let printed = line(&r);
    assert_eq!((printed["ok"].clone(), printed["id"].clone(), printed["token"].clone()), (json!(true), json!("bearer-1a2b"), json!("rcab_0123456789abcdef")));
    assert!(printed["next"].as_str().is_some_and(|next| next.contains("T3_RCA_BEARER")), "{printed}");
    let sent = rca.server.requests().into_iter().find(|q| q.method == "POST").expect("the mint reached the service");
    assert_eq!((sent.headers.get("x-bot-secret").map(String::as_str), serde_json::from_str::<Value>(&sent.body).unwrap()), (Some("s3cret"), json!({ "label": "t3" })));
    assert!(!r.stderr.contains("rcab_"), "the token is only in the answer: {}", r.stderr);

    // When the operator lists them and revokes one
    let listed = line(&run(&["list"], &rca.env()));
    let revoked = run(&["revoke", "bearer-1a2b"], &rca.env());
    // Then the list names the bearer and no token, and the revoke names the id it ended
    assert_eq!(listed, json!({ "ok": true, "bearers": [{ "id": "bearer-1a2b", "label": "t3", "createdAt": "2026-11-04T10:00:00.000Z" }] }));
    assert_eq!((revoked.exit_code, line(&revoked)), (0, json!({ "ok": true, "revoked": "bearer-1a2b" })));

    // And no label, or a blank one, is refused before any request, and a bearer the service has not is a failure that says so and what to do
    let before = rca.server.requests().len();
    assert_eq!((run(&["mint"], &rca.env()).exit_code, run(&["mint", "--label", "  "], &rca.env()).exit_code), (2, 2));
    assert_eq!(rca.server.requests().len(), before);
    let gone = run(&["revoke", "nope"], &rca.env());
    let failure = line(&gone);
    assert_eq!((gone.exit_code, failure["ok"].clone(), failure["status"].clone(), failure["code"].clone()), (1, json!(false), json!(404), json!("NOT_FOUND")), "{failure}");
    assert!(failure["next"].as_str().is_some_and(|next| next.contains("wf offload bearers list")));

    // And a service that refuses the secret is a failure naming the secret, and one that cannot be reached says how to bring it up
    rca.server.routes(&[("GET /agent/v1/bearers", MockReply::json(401, json!({ "error": "Unauthorized", "code": "UNAUTHORIZED" })))]);
    let refused = run(&["list"], &rca.env());
    let down = run(&["list"], &[("CHAT_AGENT_API_SECRET", "s3cret"), ("RCA_HOST", "127.0.0.1"), ("RCA_PORT", "1"), ("WF_TAILNET_SOCKS", "")]);
    assert_eq!((refused.exit_code, line(&refused)["status"].clone(), down.exit_code), (1, json!(401), 1));
    assert!(line(&refused)["next"].as_str().is_some_and(|next| next.contains("CHAT_AGENT_API_SECRET")));
    assert!(line(&down)["next"].as_str().is_some_and(|next| next.contains("wf vm net up")), "{}", down.stdout);
}
