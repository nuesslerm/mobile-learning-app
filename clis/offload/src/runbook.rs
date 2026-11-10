//! `wf offload ask --runbook <name> [--runbook-cli opencode|claude] [--steps-file <path>]`: a runbook (`docs/runbooks/<name>.md`)
//! is driven by an agent, which reports one JSON array of step results; the run is judged: every UI-affecting step needs
//! screenshot evidence (else the run is rejected outright), every step must pass.
use wf_kit::args::{parse_spec, Positional, Spec};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::process::{Command, Stdio};

struct Driven {
    final_text: String,
    transcript: String,
    subtype: Option<String>,
    is_error: Option<bool>,
}

fn timestamp() -> String {
    rca_core::time::iso(wf_kit::time::now_ms()).replace([':', '.'], "-")
}

fn build_prompt(title: &str, body: &str, steps_path: &str) -> String {
    [
        format!("Execute the following runbook step by step: \"{title}\"."),
        String::new(),
        body.to_string(),
        String::new(),
        "For every step, note whether it has an on-screen/visual effect (\"uiAffecting\"). If it does, you MUST capture a screenshot as evidence and record its file path -- a claim with no screenshot for a UI-affecting step will be rejected outright, not scored.".to_string(),
        String::new(),
        "When done, print ONE line containing ONLY a JSON array (no other text on that line) matching this shape, one entry per step:".to_string(),
        "[{\"description\": string, \"uiAffecting\": boolean, \"passed\": boolean, \"screenshotPath\"?: string, \"notes\"?: string}]".to_string(),
        String::new(),
        format!("Additionally write the same JSON array to {steps_path} so the driver can recover your step results even when your final text is prose or a tool transcript instead of one parseable JSON line."),
    ]
    .join("\n")
}

/// The last bracketed span of `raw` that parses as a JSON array, however many lines it spans or what wraps it.
fn parse_step_results(raw: &str) -> Option<Vec<Value>> {
    let text = raw.trim();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut starts: Vec<usize> = chars.iter().filter(|(_, c)| *c == '[').map(|(i, _)| *i).collect();
    starts.reverse();
    for start in starts {
        let mut depth = 0i32;
        for (i, c) in text[start..].char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(&text[start..start + i + 1]) {
                            return Some(a);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn read_steps_file(path: &str) -> Option<Vec<Value>> {
    match serde_json::from_str::<Value>(&std::fs::read_to_string(path).ok()?).ok()? {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

fn write_transcript(path: &str, d: &Driven) -> Result<(), String> {
    let mut lines = vec![format!("# Runbook transcript ({})", rca_core::time::iso(wf_kit::time::now_ms())), String::new()];
    lines.push(d.subtype.as_ref().map_or(String::new(), |s| format!("result_subtype: {s}")));
    lines.push(d.is_error.map_or(String::new(), |e| format!("is_error: {e}")));
    lines.push(String::new());
    let header: Vec<&String> = lines.iter().enumerate().filter(|(i, l)| !(l.is_empty() && *i > 0 && lines[i - 1].is_empty())).map(|(_, l)| l).collect();
    let header = header.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
    if let Some(dir) = Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, format!("{header}\n{}\n", d.transcript)).map_err(|e| e.to_string())
}

fn drive_opencode(prompt: String) -> Result<Driven, String> {
    match crate::dispatch_run::dispatch_runbook(prompt) {
        Some(Ok(text)) => Ok(Driven { final_text: text.clone(), transcript: text, subtype: None, is_error: None }),
        Some(Err(e)) => Err(e),
        None => Err("opencode dispatch could not start".into()),
    }
}

/// The `claude` CLI one-shot, with the Anthropic credentials stripped from its environment.
fn drive_claude(prompt: String) -> Result<Driven, String> {
    let model = "claude-sonnet-5";
    eprintln!("{}", wf_kit::logfmt::line("info", "runbook.agent.claude", "driving claude agent sdk", &[("model", model.to_string())]));
    let mut cmd = Command::new("claude");
    cmd.args(["-p", &prompt, "--model", model, "--output-format", "json"]).env_remove("ANTHROPIC_API_KEY").env_remove("ANTHROPIC_AUTH_TOKEN").stdin(Stdio::null());
    let out = cmd.output().map_err(|e| format!("the claude CLI could not be started, so \"wf offload ask --runbook --runbook-cli claude\" cannot drive a turn: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let parsed: Value = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
    let final_text = parsed["result"].as_str().map(str::to_string).unwrap_or_else(|| stdout.clone());
    if !out.status.success() && parsed.is_null() {
        return Err(format!("claude exited {}: {}", out.status.code().unwrap_or(1), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(Driven { transcript: final_text.clone(), final_text, subtype: parsed["subtype"].as_str().map(str::to_string), is_error: parsed["is_error"].as_bool() })
}

fn judge(runbook: &str, steps: Vec<Value>) -> Value {
    let missing: Vec<String> = steps
        .iter()
        .filter(|s| s["uiAffecting"].as_bool().unwrap_or(false) && s["screenshotPath"].as_str().is_none_or(str::is_empty))
        .map(|s| s["description"].as_str().unwrap_or("").to_string())
        .collect();
    if !missing.is_empty() {
        return json!({
            "runbook": runbook,
            "verdict": "rejected",
            "steps": steps,
            "rejectionReason": format!("{} UI-affecting step(s) returned no screenshot evidence: {}", missing.len(), missing.join("; ")),
        });
    }
    let pass = steps.iter().all(|s| s["passed"].as_bool().unwrap_or(false));
    json!({ "runbook": runbook, "verdict": if pass { "pass" } else { "fail" }, "steps": steps })
}

fn title_of(contents: &str, fallback: &str) -> String {
    contents
        .lines()
        .find_map(|l| l.strip_prefix('#').and_then(|r| r.strip_prefix(|c: char| c.is_whitespace())).map(|r| r.trim_start().to_string()).filter(|r| !r.is_empty()))
        .unwrap_or_else(|| fallback.to_string())
}

/// What `dispatch --runbook` takes.
pub const SPEC: Spec = Spec { values: &["runbook", "runbook-cli", "steps-file"], positional: Positional::None, ..Spec::NONE };

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    let wants_json = p.flag("json");
    let name = p.opts.get("runbook")?.clone();
    let cli = p.opts.get("runbook-cli").cloned().unwrap_or_else(|| "opencode".into());
    let fail = |message: String| {
        wf_kit::wf_err!("wf: {message}");
        Some(1)
    };
    if cli != "opencode" && cli != "claude" {
        return fail(format!("--runbook-cli must be claude or opencode, got \"{cli}\""));
    }
    let basename = if name.ends_with(".md") { name.clone() } else { format!("{name}.md") };
    let root = wf_kit::root::repo_root();
    let path = root.join("docs/runbooks").join(&basename);
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return fail(format!("ENOENT: no such file or directory, open '{}'", path.display())),
    };
    let stamp = timestamp();
    let steps_path = p.opts.get("steps-file").cloned().unwrap_or_else(|| format!(".agents/tmp/runbook-steps-{basename}-{stamp}.json"));
    let transcript_path = format!(".agents/tmp/runbook-{basename}-{stamp}.md");
    let prompt = build_prompt(&title_of(&contents, &basename), &contents, &steps_path);
    let driven = match if cli == "claude" { drive_claude(prompt) } else { drive_opencode(prompt) } {
        Ok(d) => d,
        Err(e) => return fail(e),
    };
    let steps = read_steps_file(&steps_path).or_else(|| parse_step_results(&driven.final_text));
    let result = match steps {
        Some(s) => judge(&basename, s),
        None => {
            if let Err(e) = write_transcript(&transcript_path, &driven) {
                return fail(e);
            }
            let mut m = Map::new();
            m.insert("runbook".into(), json!(basename));
            m.insert("verdict".into(), json!("rejected"));
            m.insert("steps".into(), json!([]));
            m.insert("rejectionReason".into(), json!(format!("the agent did not report a parseable JSON step-results array (finalText or {steps_path}); full transcript written to {transcript_path}")));
            Value::Object(m)
        }
    };
    if wants_json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
    } else {
        let reason = result["rejectionReason"].as_str().map_or(String::new(), |r| format!(" -- {r}"));
        println!("{}: {}{reason}", basename, result["verdict"].as_str().unwrap_or(""));
    }
    let verdict = result["verdict"].as_str().unwrap_or("");
    if verdict != "pass" {
        return fail(format!("runbook \"{basename}\" did not pass ({verdict})"));
    }
    Some(0)
}
