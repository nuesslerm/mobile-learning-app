//! `wf bench agent-proof`: an agent reads `wf browser test --api` and writes one Lua script for a fixed four-step task, then
//! the script runs recorded; one JSON object per arm says what the agent took and cost and whether its script passed.
use wf_kit::args::{parse_spec, Positional, Spec};
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TASK: &str = "Write one Lua script for `wf browser test` that does exactly these four steps, using only the API below:
1. open env.base .. \"/leetcode/two-sum\"
2. wait for {id='ProblemEditorRunLocalButton'}
3. tap it
4. wait for {id='ProblemEditorRunOutcomeTotals'} and fail the script unless its text contains \"passed\"
The code editor autofocuses on open and its on-screen keyboard covers the button, so before step 3 lower it as a user does:
when {id='EditorHelperRails'} (the keyboard toolbar, two rows) shows within 3 s, its lower row {id='EditorActionToolbar'} ends in
Done {id='EditorAction-Done'}, which is off screen until that row scrolls: scroll that row until Done shows (it lies to the
RIGHT), tap Done, then wait until the rails are gone. Never hide the keyboard any other way.
The run passes when the script exits 0. Answer with only the Lua script: no prose, no tools.";

const REFERENCE: &str = "e2e/agent-proof.lua";
const ARMS: &[&str] = &["sonnet", "opencode", "scripted"];

pub fn prompt() -> String {
    format!("{TASK}\n\nAPI (`wf browser test --api`):\n{}", wf_harness::device::api::text("browser"))
}

/// The Lua in an agent's answer: the first fenced block when there is one, else the whole answer.
pub fn extract_script(answer: &str) -> String {
    let mut lines = answer.lines();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("```") {
            let body: Vec<&str> = lines.by_ref().take_while(|l| !l.trim_start().starts_with("```")).collect();
            return format!("{}\n", body.join("\n").trim_end());
        }
    }
    format!("{}\n", answer.trim())
}

pub struct Agent {
    pub model: String,
    pub answer: String,
    pub cost_usd: Option<f64>,
    pub tokens: Option<f64>,
}

/// `claude -p --output-format json`: the answer, the model that served it, its cost and its tokens.
pub fn parse_claude(stdout: &str) -> Result<Agent, String> {
    let v: Value = serde_json::from_str(stdout.trim()).map_err(|e| format!("claude printed no JSON: {e}"))?;
    if v["is_error"].as_bool() == Some(true) {
        return Err(format!("claude failed: {}", v["result"].as_str().unwrap_or("")));
    }
    let answer = v["result"].as_str().ok_or("claude JSON has no result")?.to_string();
    // claude -p also bills a small helper model: the answer came from the one that wrote the most
    let model = v["modelUsage"].as_object().and_then(|m| m.iter().max_by_key(|(_, u)| u["outputTokens"].as_u64().unwrap_or(0)).map(|(k, _)| k.clone())).unwrap_or_else(|| "sonnet".into());
    let u = &v["usage"];
    let parts = ["input_tokens", "output_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"].map(|k| u[k].as_f64());
    let tokens = parts.iter().any(Option::is_some).then(|| parts.iter().flatten().sum());
    Ok(Agent { model, answer, cost_usd: v["total_cost_usd"].as_f64(), tokens })
}

pub struct Run {
    pub passed: bool,
    pub run_s: f64,
    pub recordings: Vec<String>,
    pub error: Option<String>,
}

/// The JSON object one arm prints; `costUsd`, `tokens` and `error` only when known.
pub fn report(arm: &str, model: &str, agent_s: f64, script: &str, run: &Run, agent: Option<&Agent>, error: Option<String>) -> Value {
    let round = |s: f64| (s * 10.0).round() / 10.0;
    let mut o = Map::new();
    o.insert("arm".into(), json!(arm));
    o.insert("model".into(), json!(model));
    o.insert("agentS".into(), json!(round(agent_s)));
    o.insert("runS".into(), json!(round(run.run_s)));
    o.insert("wallS".into(), json!(round(agent_s + run.run_s)));
    o.insert("passed".into(), json!(run.passed));
    if let Some(c) = agent.and_then(|a| a.cost_usd) {
        o.insert("costUsd".into(), json!(c));
    }
    if let Some(t) = agent.and_then(|a| a.tokens) {
        o.insert("tokens".into(), json!(t));
    }
    o.insert("script".into(), json!(script));
    o.insert("recording".into(), json!(run.recordings.first()));
    if let Some(e) = error.or_else(|| run.error.clone()) {
        o.insert("error".into(), json!(e));
    }
    Value::Object(o)
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs `cmd` with `input` on stdin; past `timeout` its whole process group is killed and the code is 124.
fn capture(mut cmd: Command, input: &str, timeout: Duration) -> Result<Output, String> {
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {:?}: {e}", cmd.get_program()))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let reader = std::thread::spawn(move || {
        let (mut o, mut e) = (String::new(), String::new());
        if let Some(s) = out.as_mut() {
            let _ = s.read_to_string(&mut o);
        }
        if let Some(s) = err.as_mut() {
            let _ = s.read_to_string(&mut e);
        }
        (o, e)
    });
    let started = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(1),
            Ok(None) if started.elapsed() > timeout => {
                let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(child.id() as i32), nix::sys::signal::Signal::SIGKILL);
                let _ = child.wait();
                break 124;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let (stdout, stderr) = reader.join().unwrap_or_default();
    Ok(Output { code, stdout, stderr })
}

fn ask_claude(prompt: &str, timeout: Duration) -> Result<Agent, String> {
    // Outside the repo, with no tools and no MCP servers: the prompt is the task and the API text, nothing else.
    let cwd = std::env::temp_dir().join(format!("wf-agent-proof-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&cwd);
    let mut cmd = Command::new("claude");
    cmd.args(["-p", "--model", "sonnet", "--output-format", "json", "--no-session-persistence", "--strict-mcp-config", "--tools", ""]).current_dir(&cwd);
    let out = capture(cmd, prompt, timeout)?;
    let _ = std::fs::remove_dir_all(&cwd);
    if out.code == 124 {
        return Err(format!("claude timed out after {}s", timeout.as_secs()));
    }
    parse_claude(&out.stdout).map_err(|e| format!("{e} (exit {}; stderr: {})", out.code, out.stderr.trim()))
}

fn ask_opencode(prompt: String, timeout: Duration) -> Result<Agent, String> {
    let d = offload::dispatch_run::dispatch_read_only(prompt, timeout.as_millis() as u64, "agent-proof").ok_or("the opencode ladder has no rung this binary drives")?;
    if !d.ok {
        let trail: Vec<String> = d.attempts.iter().map(|(m, _, r)| format!("{m} ({r})")).collect();
        return Err(format!("opencode dispatch failed after {}", trail.join(", ")));
    }
    Ok(Agent { model: d.model.unwrap_or_default(), answer: d.output, cost_usd: d.cost, tokens: d.tokens })
}

/// `wf <platform> test <script> --env base=<url>`, recorded into `<out>/<arm>-rec`, waits logged to `<out>/<arm>-waits.jsonl`.
fn run_script(script: &Path, platform: &str, base: &str, out: &Path, arm: &str) -> Result<Run, String> {
    let exe = wf_harness::device::self_exe()?;
    let mut cmd = Command::new(exe);
    let rec = out.join(format!("{arm}-rec"));
    cmd.args([platform, "test"]).arg(script).args(["--env", &format!("base={base}"), "--test-output-dir"]).arg(&rec);
    cmd.env("WF_WAIT_LOG", out.join(format!("{arm}-waits.jsonl")));
    let started = Instant::now();
    let o = capture(cmd, "", Duration::from_secs(600))?;
    let run_s = started.elapsed().as_secs_f64();
    let _ = std::fs::write(out.join(format!("{arm}-run.log")), format!("{}{}", o.stdout, o.stderr));
    let recordings = o.stdout.lines().filter_map(|l| l.strip_prefix("recording: ")).map(str::to_string).collect();
    let error = (o.code != 0).then(|| o.stderr.lines().find(|l| l.starts_with("wf: ")).unwrap_or(o.stderr.trim()).to_string());
    Ok(Run { passed: o.code == 0, run_s, recordings, error })
}

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &Spec { values: &["arm", "base", "out", "platform", "script", "timeout"], required: &["arm", "base"], positional: Positional::None, ..Spec::NONE })?;
    let arm = p.val("arm").unwrap_or_default();
    if !ARMS.contains(&arm) {
        wf_kit::wf_err!("wf: unknown arm '{arm}' ({})", ARMS.join(", "));
        return Some(1);
    }
    let base = p.val("base").unwrap_or_default().trim_end_matches('/').to_string();
    let platform = p.val("platform").unwrap_or("browser");
    let timeout = Duration::from_secs(p.val("timeout").and_then(|t| t.parse().ok()).unwrap_or(600));
    let out = p.val("out").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!(".agents/tmp/agent-proof/{}", wf_kit::time::now_ms())));
    if let Err(e) = std::fs::create_dir_all(&out) {
        wf_kit::wf_err!("wf: cannot create {}: {e}", out.display());
        return Some(1);
    }
    let started = Instant::now();
    let asked = match arm {
        "sonnet" => ask_claude(&prompt(), timeout),
        "opencode" => ask_opencode(prompt(), timeout),
        _ => {
            let path = p.val("script").map(PathBuf::from).unwrap_or_else(|| wf_kit::root::repo_root().join(REFERENCE));
            std::fs::read_to_string(&path).map(|answer| Agent { model: "none".into(), answer, cost_usd: None, tokens: None }).map_err(|e| format!("cannot read {}: {e}", path.display()))
        }
    };
    let agent_s = if arm == "scripted" { 0.0 } else { started.elapsed().as_secs_f64() };
    let (agent, script, run, error) = match asked {
        Ok(agent) => {
            let _ = std::fs::write(out.join(format!("{arm}-answer.txt")), &agent.answer);
            let script = extract_script(&agent.answer);
            let path = out.join(format!("{arm}.lua"));
            let run = std::fs::write(&path, &script).map_err(|e| e.to_string()).and_then(|()| run_script(&path, platform, &base, &out, arm));
            match run {
                Ok(run) => (Some(agent), script, run, None),
                Err(e) => (Some(agent), script, Run { passed: false, run_s: 0.0, recordings: vec![], error: None }, Some(e)),
            }
        }
        Err(e) => (None, String::new(), Run { passed: false, run_s: 0.0, recordings: vec![], error: None }, Some(e)),
    };
    let model = agent.as_ref().map_or_else(|| if arm == "sonnet" { "sonnet".into() } else { String::new() }, |a| a.model.clone());
    let v = report(arm, &model, agent_s, &script, &run, agent.as_ref(), error);
    println!("{v}");
    Some(if run.passed { 0 } else { 1 })
}
