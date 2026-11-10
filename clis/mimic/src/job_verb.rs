//! `wf mimic job record|replay <set>`: one capture job of the record-references workflow, for one set that is already provisioned.
//!
//! `record` runs `wf mimic record <set> --resume --max-minutes <chunk>` again and again (each call is a bounded chunk that leaves its
//! checkpoint under `<set>/raw/checkpoint`) until the record is complete or the job's budget is spent; a complete record has its raw captures
//! deleted, is verified offline and is scanned for token-shaped values, because the masked set goes to a bucket anyone can read.
//! `replay` replays the recorded set against each of its candidates and tabulates the diff. Both append a summary to `--summary`
//! (default `$GITHUB_STEP_SUMMARY`) and print one JSON object (schema `mimic-job/1`).
use super::{fail, set_ref, Done, Fail};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use wf_kit::args::Parsed;

const DEFAULT_BUDGET_MINUTES: u64 = 100;
const MISSING: &str = "`wf mimic record` has no --resume and --max-minutes: the checkpointed record that chunked jobs need is not in this build. The interface this job calls is `wf mimic record <set> --resume --max-minutes <n>`: it stops after n minutes with its state under <set>/raw/checkpoint and prints complete:false in its JSON, and a later call resumes it.";
const BINDING: &str = "mimic-job-binding.json";
const MAX_SCANNED_BYTES: u64 = 8 * 1024 * 1024;

/// The binary that answers `mimic ...`: `WF_MIMIC_BIN` (a test double, or a build elsewhere), else this one.
fn bin() -> PathBuf {
    std::env::var_os("WF_MIMIC_BIN").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::current_exe().ok()).unwrap_or_else(|| PathBuf::from("wf-mimic"))
}

struct Call {
    code: i32,
    json: Value,
    stdout: String,
}

fn call(args: &[&str], env: &[(String, String)]) -> Result<Call, Fail> {
    let out = Command::new(bin()).arg("mimic").args(args).envs(env.iter().map(|(k, v)| (k, v))).stdin(Stdio::null()).stderr(Stdio::inherit()).output().map_err(|e| fail(format!("wf mimic {}: {e}", args.first().unwrap_or(&"")), "check wf-mimic is installed beside wf"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let json = stdout.lines().rev().find(|l| l.starts_with('{')).and_then(|l| serde_json::from_str(l).ok()).unwrap_or(Value::Null);
    Ok(Call { code: out.status.code().unwrap_or(-1), json, stdout })
}

fn minutes(p: &Parsed, name: &str, default: u64, max: u64) -> Result<u64, Fail> {
    let Some(v) = p.val(name) else { return Ok(default) };
    v.parse::<u64>().ok().filter(|n| (1..=max).contains(n)).ok_or_else(|| fail(format!("--{name} {v} is not a number of minutes in 1..={max}"), "wf mimic job --help"))
}

/// A token-shaped value in masked output: what the runbook's grep looks for.
pub fn token_like(line: &str) -> Option<&'static str> {
    if let Some(i) = line.find("Bearer ") {
        if line[i + 7..].bytes().take(8).filter(u8::is_ascii_alphanumeric).count() == 8 {
            return Some("a Bearer token");
        }
    }
    let mut rest = line;
    while let Some(i) = rest.find("eyJ") {
        let tail = &rest[i + 3..];
        if tail.bytes().take(10).filter(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-').count() == 10 {
            return Some("a JWT");
        }
        rest = tail;
    }
    None
}

fn append_summary(p: &Parsed, text: &str) {
    let Some(path) = p.val("summary").map(PathBuf::from).or_else(|| std::env::var_os("GITHUB_STEP_SUMMARY").map(PathBuf::from)) else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(text.as_bytes());
    }
}

fn run_id(p: &Parsed) -> String {
    p.val("run").map(String::from).or_else(|| std::env::var("GITHUB_RUN_ID").ok().filter(|v| !v.is_empty())).unwrap_or_else(|| "local".into())
}

pub fn job_verb(p: &Parsed) -> Done {
    let (action, set_arg) = (p.positional[0].as_str(), p.positional[1].as_str());
    match action {
        "record" => record(p, set_arg),
        "replay" => replay(p, set_arg),
        other => Err(fail(format!("job {other}: the actions are record and replay"), "wf mimic job --help")),
    }
}

fn mins(d: Duration) -> String {
    format!("{}m{:02}s", d.as_secs() / 60, d.as_secs() % 60)
}

fn record(p: &Parsed, set_arg: &str) -> Done {
    let set = set_ref(set_arg);
    let cfg = super::load_config(&set.dir.join("mimic.toml"))?;
    super::reference(&cfg)?;
    let chunk = minutes(p, "chunk", u64::from(crate::marker::DEFAULT_CHUNK_MINUTES), u64::from(crate::marker::MAX_CHUNK_MINUTES))?;
    let budget = Duration::from_secs(minutes(p, "budget", DEFAULT_BUDGET_MINUTES, 600)? * 60);
    let run = run_id(p);
    let dir = set.dir.display().to_string();
    let help = call(&["record", "--help"], &[])?;
    if !help.stdout.contains("--resume") || !help.stdout.contains("--max-minutes") {
        return Err(fail(MISSING, "land the checkpointed `wf mimic record`, then push the marker again"));
    }
    let checkpoint = set.dir.join("raw/checkpoint");
    let env = cfg.provision.as_ref().map(|prov| super::provision_verb::block_env(p, &set.name, &set.dir, prov)).unwrap_or_default();
    let key = binding_key(&cfg)?;
    let mut warnings: Vec<String> = Vec::new();
    if checkpoint.exists() && (p.flag("fresh") || !bound_to(&checkpoint, &key)) {
        if !p.flag("fresh") {
            warnings.push("an earlier checkpoint was recorded against another reference pin, provision block or corpus (or carries no binding): it is dropped and the record starts over".to_string());
        }
        std::fs::remove_dir_all(&checkpoint).map_err(|e| fail(format!("{}: {e}", checkpoint.display()), "make the set directory writable"))?;
    }
    let started = Instant::now();
    let (mut chunks, mut last, mut complete) = (Vec::<Value>::new(), Value::Null, false);
    let chunk_arg = chunk.to_string();
    let source = format!("record-references run {run}");
    while chunks.is_empty() || started.elapsed() + Duration::from_secs(chunk * 60) <= budget {
        let t = Instant::now();
        let mut args = vec!["record", dir.as_str(), "--resume", "--max-minutes", chunk_arg.as_str(), "--recorded-on", "ci", "--source", source.as_str()];
        if cfg.record.allow_nondeterministic {
            args.push("--allow-nondeterministic");
        }
        args.extend(cfg.record.args.iter().map(String::as_str));
        let c = call(&args, &env)?;
        if c.code != 0 || c.json["ok"] == json!(false) {
            let mut f = fail(format!("wf mimic record failed (exit {}): {}", c.code, c.json["error"].as_str().unwrap_or("no error in its output")), c.json["next"].as_str().unwrap_or("fix the reference and push the marker again").to_string());
            f.evidence = json!({"chunks": chunks, "record": c.json});
            append_summary(p, &format!("### Record `{}`: failed\n\n{}\n\n", set.name, f.error));
            return Err(f);
        }
        if checkpoint.is_dir() {
            let _ = std::fs::write(checkpoint.join(BINDING), json!({"key": key}).to_string());
        }
        let done = c.json["complete"].as_bool().unwrap_or(true);
        chunks.push(json!({"n": chunks.len() + 1, "seconds": t.elapsed().as_secs(), "complete": done, "interactions": c.json["interactions"]}));
        last = c.json;
        if done {
            complete = true;
            break;
        }
    }
    let wall = started.elapsed();
    let n = last["interactions"].as_u64().unwrap_or(0);
    let unstable = last["nondeterministic"].as_array().map_or(0, Vec::len) as u64;
    let stable_pct = if n == 0 { 0.0 } else { ((n - unstable.min(n)) as f64 * 10000.0 / n as f64).round() / 100.0 };
    let mut out = json!({
        "ok": complete, "action": "record", "set": set.name, "dir": dir, "run": run, "complete": complete, "chunks": chunks, "chunk_minutes": chunk,
        "interactions": n, "nondeterministic": unstable, "stable_pct": stable_pct, "wall_seconds": wall.as_secs(), "warnings": warnings,
        "differences": last["differences"].clone(),
    });
    if !complete {
        let held = scan_tree(&checkpoint, &checkpoint);
        out["incomplete"] = json!(true);
        out["checkpoint_upload"] = json!(held.is_empty());
        out["checkpoint_leaks"] = json!(held);
        out["error"] = json!(format!("the budget of {} minutes ran out after {} chunk(s) of {chunk} minutes: the checkpoint is kept", budget.as_secs() / 60, chunks.len()));
        out["next"] = json!(format!("push an empty commit with [record {}] to resume from the checkpoint", set.name));
        append_summary(p, &format!("### Record `{}`: INCOMPLETE\n\n{} chunk(s) of {chunk} minutes in {}. {} push `[record {}]` again to resume.\n\n", set.name, chunks.len(), mins(wall), if held.is_empty() { "The checkpoint was uploaded as an artifact:" } else { "The checkpoint holds token-shaped values and was NOT uploaded (the next marker needs `fresh`):" }, set.name));
        return Ok((out, 1));
    }
    let raw = set.dir.join("raw");
    if raw.exists() {
        std::fs::remove_dir_all(&raw).map_err(|e| fail(format!("{}: {e}", raw.display()), "make the set directory writable"))?;
    }
    let verify = call(&["verify", &dir], &[])?;
    let leaks = scan(&set.dir);
    out["raw_removed"] = json!(true);
    let failed: Vec<Value> = verify.json["checks"].as_array().into_iter().flatten().chain(verify.json["sets"].as_array().into_iter().flatten().flat_map(|s| s["checks"].as_array().into_iter().flatten())).filter(|c| c["status"] != "ok").cloned().collect();
    let problems: Vec<Value> = verify.json["sets"].as_array().into_iter().flatten().flat_map(|s| s["problems"].as_array().into_iter().flatten()).cloned().collect();
    let accounting_only = cfg.record.allow_nondeterministic && failed.is_empty() && !problems.is_empty() && problems.iter().all(|x| x["field"] == "stability");
    out["verify"] = json!({"ok": verify.json["ok"], "exit": verify.code, "failed": failed, "problems": problems.iter().take(40).collect::<Vec<_>>(), "accounting_deferred": accounting_only, "next": verify.json["next"], "stderr": verify.stdout.lines().filter(|l| !l.starts_with('{')).take(20).collect::<Vec<_>>()});
    out["leaks"] = json!(leaks);
    let verified = verify.code == 0 || accounting_only;
    out["ok"] = json!(verified && leaks.is_empty());
    out["next"] = json!(format!("wf mimic publish {dir} --run {run}"));
    let state = if out["ok"] == json!(true) { "complete" } else { "complete, held back" };
    append_summary(p, &format!(
        "### Record `{}`: {state}\n\n| interactions | nondeterministic | stability | chunks | wall time | verify |\n|---|---|---|---|---|---|\n| {n} | {unstable} | {stable_pct}% | {} x {chunk} min | {} | {} |\n\n{}",
        set.name, chunks.len(), mins(wall), if verify.code == 0 { "ok" } else if accounting_only { "ok, stability allowlist to regenerate" } else { "FAILED" },
        if leaks.is_empty() { String::new() } else { format!("Token-shaped values in the masked set (nothing is published; add a mask rule to rules.json): {}\n\n", leaks.join(", ")) }
    ));
    let code = if out["ok"] == json!(true) { 0 } else { 1 };
    Ok((out, code))
}

/// What a checkpoint is bound to: the provision block (with the pinned source commit), the corpus and the reference's pin. Resuming a checkpoint
/// of anything else would mix two recordings.
fn binding_key(cfg: &crate::config::Config) -> Result<String, Fail> {
    let corpus = cfg.corpus.as_ref().and_then(|c| std::fs::read(c).ok()).unwrap_or_default();
    let pin = super::reference(cfg)?.pin_hash().map_err(|e| fail(e, "fix the reference pin"))?;
    let parts = json!([cfg.provision, mimic_core::json::sha256_label(&corpus), pin]);
    Ok(mimic_core::json::sha256_label(parts.to_string().as_bytes()))
}

fn bound_to(checkpoint: &Path, key: &str) -> bool {
    std::fs::read_to_string(checkpoint.join(BINDING)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["key"] == json!(key))
}

/// Token-shaped values anywhere under `dir` (the checkpoint is unmasked, so it only travels when none are found).
fn scan_tree(dir: &Path, root: &Path) -> Vec<String> {
    let mut hits = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return hits };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            hits.extend(scan_tree(&path, root));
        } else if e.metadata().is_ok_and(|m| m.len() <= MAX_SCANNED_BYTES) {
            let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()).into_owned();
            if let Some((i, what)) = text.lines().enumerate().find_map(|(i, l)| token_like(l).map(|w| (i + 1, w))) {
                hits.push(format!("{}:{i} has {what}", path.strip_prefix(root).unwrap_or(&path).display()));
            }
        }
    }
    hits
}

fn scan(dir: &Path) -> Vec<String> {
    let mut hits = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return hits };
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_file() || !matches!(path.extension().and_then(|x| x.to_str()), Some("json" | "jsonl" | "toml")) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if let Some((i, what)) = text.lines().enumerate().find_map(|(i, l)| token_like(l).map(|w| (i + 1, w))) {
            hits.push(format!("{}:{i} has {what}", path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())));
        }
    }
    hits
}

fn replay(p: &Parsed, set_arg: &str) -> Done {
    let set = set_ref(set_arg);
    let cfg = super::load_config(&set.dir.join("mimic.toml"))?;
    let dir = set.dir.display().to_string();
    let env = cfg.provision.as_ref().map(|prov| super::provision_verb::block_env(p, &set.name, &set.dir, prov)).unwrap_or_default();
    let wanted: Vec<String> = match p.val("candidate") {
        Some(c) => vec![c.to_string()],
        None => cfg.candidates.keys().cloned().collect(),
    };
    if wanted.is_empty() {
        return Err(fail(format!("{dir}/mimic.toml has no [candidate.<name>]"), "add the program to replay against"));
    }
    let started = Instant::now();
    let (mut rows, mut diffs, mut ok) = (Vec::<Value>::new(), Vec::<String>::new(), true);
    for name in &wanted {
        let c = call(&["replay", &dir, "--candidate", name], &env)?;
        if c.code >= 2 || c.json.is_null() {
            let mut f = fail(format!("wf mimic replay {name} could not run: {}", c.json["error"].as_str().unwrap_or("no JSON in its output")), c.json["next"].as_str().unwrap_or("fix the candidate command in mimic.toml").to_string());
            f.evidence = json!({"candidate": name, "replay": c.json});
            append_summary(p, &format!("### Replay `{}` against `{name}`: could not run\n\n{}\n\n", set.name, f.error));
            return Err(f);
        }
        ok &= c.code == 0;
        let s = &c.json["summary"];
        rows.push(json!({"candidate": name, "severity": c.json["severity"], "summary": s}));
        for r in c.json["results"].as_array().into_iter().flatten().take(40) {
            let d = &r["diffs"][0];
            diffs.push(format!("| {name} | {} | {} | {} {} |", r["id"].as_str().unwrap_or(""), r["severity"].as_str().unwrap_or(""), d["kind"].as_str().unwrap_or(""), d["path"].as_str().unwrap_or(r["error"].as_str().unwrap_or(""))));
        }
    }
    let mut md = format!("### Replay `{}`\n\n| candidate | severity | total | clean | allowlisted | drift | breaking |\n|---|---|---|---|---|---|---|\n", set.name);
    for r in &rows {
        let s = &r["summary"];
        md += &format!("| {} | {} | {} | {} | {} | {} | {} |\n", r["candidate"].as_str().unwrap_or(""), r["severity"].as_str().unwrap_or(""), s["total"], s["clean"], s["allowlisted"], s["drift"], s["breaking"]);
    }
    if !diffs.is_empty() {
        md += &format!("\n| candidate | interaction | severity | first difference |\n|---|---|---|---|\n{}\n", diffs.join("\n"));
    }
    md += &format!("\nWall time {}.\n\n", mins(started.elapsed()));
    append_summary(p, &md);
    let out = json!({"ok": ok, "action": "replay", "set": set.name, "dir": dir, "run": run_id(p), "candidates": rows, "differences": diffs.len(), "wall_seconds": started.elapsed().as_secs(), "warnings": [],
        "next": if ok { "nothing to do".to_string() } else { format!("wf mimic replay {dir} --candidate {}", wanted[0]) }});
    if let Some(o) = p.val("out") {
        let path = Path::new(o).join(format!("{}.replay.json", set.name));
        std::fs::create_dir_all(o).and_then(|()| std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap_or_default() + "\n")).map_err(|e| fail(format!("{}: {e}", path.display()), "name a writable --out"))?;
    }
    Ok((out, i32::from(!ok)))
}
