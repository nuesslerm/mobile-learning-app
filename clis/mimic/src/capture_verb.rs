//! `wf mimic capture <record|replay> <set>`: the capture of one set on a runner, in four phases.
//!
//! 1. provision (`--provision auto|only|skip`, default auto): the `[provision]` block installs the reference (`provision_phase.rs`); a work
//!    directory whose marker key matches is reused. Provisioning builds third-party code, so every child of it runs without a credential.
//! 2. capture, every child in a `ChildEnv` (`child_env.rs`: an allowlist of the caller's environment, the `[env]` of the block, the variables the set
//!    names; `BWS_*` and `GITHUB_*` never) and the keys a record's reference declares (`[reference] env_from_vault`) in the reference's alone:
//!    `record` runs `wf mimic record <set> --resume --max-minutes <chunk>` again and again (each call is a bounded chunk that leaves its
//!    checkpoint under `<set>/raw/checkpoint`) until the record is complete or the budget is spent; a complete record has its raw captures
//!    deleted, is verified offline and is scanned for token-shaped values and for the exact value of every declared key, because the masked set goes to a bucket anyone can read.
//!    `replay` replays the recorded set against each of its candidates and tabulates the diff.
//! 3. candidate stop: a `chain` candidate the replay started is stopped when its replay ends (`chain_launch.rs`).
//! 4. publish (`--publish`, a record that is complete and clean): the masked set committed to the state ref, after the reference is gone.
//!
//! It appends a summary to `--summary` (default `$GITHUB_STEP_SUMMARY`) and prints one JSON object (schema `mimic-capture/1`).
use super::provision_phase::{self, Mode};
use crate::child_env::{self, ChildEnv};
use crate::leak_scan::{scan_set, scan_tree};
use super::{fail, set_ref, Done, Fail};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use wf_kit::args::Parsed;

const DEFAULT_BUDGET_MINUTES: u64 = 100;
const MISSING: &str = "`wf mimic record` has no --resume and --max-minutes: the checkpointed record that chunked jobs need is not in this build. The interface this job calls is `wf mimic record <set> --resume --max-minutes <n>`: it stops after n minutes with its state under <set>/raw/checkpoint and prints complete:false in its JSON, and a later call resumes it.";
const BINDING: &str = "mimic-capture-binding.json";

/// The binary that answers `mimic ...`: `WF_MIMIC_BIN` (a test double, or a build elsewhere), else this one.
fn bin() -> PathBuf {
    std::env::var_os("WF_MIMIC_BIN").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::current_exe().ok()).unwrap_or_else(|| PathBuf::from("wf-mimic"))
}

struct Call {
    code: i32,
    json: Value,
    stdout: String,
}

/// One `wf mimic <args>` child in `env`. The values of `secrets` are replaced by their names in everything it prints, so a log line of the reference
/// never carries one to the runner's log or to the JSON this verb prints.
fn call(args: &[&str], env: &ChildEnv, secrets: &[(String, String)]) -> Result<Call, Fail> {
    let started = |e: std::io::Error| fail(format!("wf mimic {}: {e}", args.first().unwrap_or(&"")), "check wf-mimic is installed beside wf");
    let mut child = env.command(bin()).arg("mimic").args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(started)?;
    let stderr = child.stderr.take().map(BufReader::new);
    let values = secrets.to_vec();
    let relay = std::thread::spawn(move || {
        for line in stderr.into_iter().flat_map(|r| r.split(b'\n').map_while(Result::ok)) {
            eprintln!("{}", child_env::redact(&String::from_utf8_lossy(&line), &values));
        }
    });
    let out = child.wait_with_output().map_err(started)?;
    let _ = relay.join();
    let stdout = child_env::redact(&String::from_utf8_lossy(&out.stdout), secrets);
    let json = stdout.lines().rev().find(|l| l.starts_with('{')).and_then(|l| serde_json::from_str(l).ok()).unwrap_or(Value::Null);
    Ok(Call { code: out.status.code().unwrap_or(-1), json, stdout })
}

fn minutes(p: &Parsed, name: &str, default: u64, max: u64) -> Result<u64, Fail> {
    let Some(v) = p.val(name) else { return Ok(default) };
    v.parse::<u64>().ok().filter(|n| (1..=max).contains(n)).ok_or_else(|| fail(format!("--{name} {v} is not a number of minutes in 1..={max}"), "wf mimic capture --help"))
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

pub fn capture_verb(p: &Parsed) -> Done {
    let (action, set_arg) = (p.positional[0].as_str(), p.positional[1].as_str());
    if !matches!(action, "record" | "replay") {
        return Err(fail(format!("capture {action}: the actions are record and replay"), "wf mimic capture --help"));
    }
    let mode = Mode::parse(p)?;
    if p.flag("publish") && (action != "record" || p.flag("check") || mode == Mode::Only) {
        return Err(fail("--publish publishes the set a record just completed: it goes with `record` and a capture (not --check, not --provision only)", "wf mimic capture record <set> --publish"));
    }
    let set = set_ref(set_arg);
    let cfg = super::load_config(&set.dir.join("mimic.toml"))?;
    let provisioned = provision_phase::phase(p, &set, &cfg, action, mode)?;
    let dir = set.dir.display().to_string();
    if p.flag("check") || mode == Mode::Only {
        let next = if p.flag("check") { format!("wf mimic capture {action} {dir}") } else { format!("wf mimic capture {action} {dir} --provision skip") };
        let out = json!({"ok": true, "action": action, "set": set.name, "dir": dir, "captured": false, "provision": provisioned, "warnings": [], "next": next});
        return Ok((out, 0));
    }
    let (mut out, code) = if action == "record" { record(p, &set, &cfg)? } else { replay(p, &set, &cfg)? };
    out["provision"] = provisioned;
    Ok((out, code))
}

fn mins(d: Duration) -> String {
    format!("{}m{:02}s", d.as_secs() / 60, d.as_secs() % 60)
}

fn record(p: &Parsed, set: &super::SetRef, cfg: &crate::config::Config) -> Done {
    super::reference(cfg)?;
    let chunk = minutes(p, "chunk", u64::from(mimic_plan_core::DEFAULT_CHUNK_MINUTES), u64::from(mimic_plan_core::MAX_CHUNK_MINUTES))?;
    let budget = Duration::from_secs(minutes(p, "budget", DEFAULT_BUDGET_MINUTES, 600)? * 60);
    let run = run_id(p);
    let dir = set.dir.display().to_string();
    let plain = ChildEnv::base();
    let help = call(&["record", "--help"], &plain, &[])?;
    if !help.stdout.contains("--resume") || !help.stdout.contains("--max-minutes") {
        return Err(fail(MISSING, "land the checkpointed `wf mimic record`, then push the marker again"));
    }
    let checkpoint = set.dir.join("raw/checkpoint");
    let declared = cfg.reference.as_ref().map(|r| r.env_from_vault.clone()).unwrap_or_default();
    let held = child_env::resolve_declared(&declared).map_err(|e| fail(e, "put the key in the vault (`wf secret`) or the environment of the capture step, and run again"))?;
    let secrets: Vec<(String, String)> = held.iter().map(|d| (d.key.clone(), d.value.clone())).collect();
    let block = cfg.provision.as_ref().map(|prov| provision_phase::block_env(p, &set.name, &set.dir, prov)).unwrap_or_default();
    let env = ChildEnv::base().pass(&cfg.declared_pass()).with(block).with(held.iter().map(|d| (d.env.clone(), d.value.clone())));
    let key = binding_key(cfg)?;
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
        let c = call(&args, &env, &secrets)?;
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
        "captured": true, "ok": complete, "action": "record", "set": set.name, "dir": dir, "run": run, "complete": complete, "chunks": chunks, "chunk_minutes": chunk,
        "interactions": n, "nondeterministic": unstable, "stable_pct": stable_pct, "wall_seconds": wall.as_secs(), "warnings": warnings,
        "differences": last["differences"].clone(),
    });
    if !complete {
        let held = scan_tree(&checkpoint, &checkpoint, &secrets);
        out["incomplete"] = json!(true);
        out["checkpoint_upload"] = json!(held.is_empty());
        out["checkpoint_leaks"] = json!(held);
        out["error"] = json!(format!("the budget of {} minutes ran out after {} chunk(s) of {chunk} minutes: the checkpoint is kept", budget.as_secs() / 60, chunks.len()));
        out["next"] = json!(format!("push an empty commit with [record {}] to resume from the checkpoint", set.name));
        append_summary(p, &format!("### Record `{}`: INCOMPLETE\n\n{} chunk(s) of {chunk} minutes in {}. {} push `[record {}]` again to resume.\n\n", set.name, chunks.len(), mins(wall), if held.is_empty() { "The checkpoint was uploaded as an artifact:" } else { "The checkpoint holds token-shaped values and was NOT uploaded (the next marker needs `fresh`):" }, set.name));
        return Ok((out, 1));
    }
    let raw = set.dir.join("raw");
    let raw_leaks = scan_tree(&raw, &raw, &secrets).into_iter().filter(|h| h.contains(" holds the value of ")).map(|h| format!("raw/{h}")).collect::<Vec<_>>();
    if raw.exists() {
        std::fs::remove_dir_all(&raw).map_err(|e| fail(format!("{}: {e}", raw.display()), "make the set directory writable"))?;
    }
    let verify = call(&["verify", &dir], &plain, &[])?;
    let mut leaks = raw_leaks;
    leaks.extend(scan_set(&set.dir, &secrets));
    out["raw_removed"] = json!(true);
    let failed: Vec<Value> = verify.json["checks"].as_array().into_iter().flatten().chain(verify.json["sets"].as_array().into_iter().flatten().flat_map(|s| s["checks"].as_array().into_iter().flatten())).filter(|c| c["status"] != "ok").cloned().collect();
    let problems: Vec<Value> = verify.json["sets"].as_array().into_iter().flatten().flat_map(|s| s["problems"].as_array().into_iter().flatten()).cloned().collect();
    let accounting_only = cfg.record.allow_nondeterministic && failed.is_empty() && !problems.is_empty() && problems.iter().all(|x| x["field"] == "stability");
    out["verify"] = json!({"ok": verify.json["ok"], "exit": verify.code, "failed": failed, "problems": problems.iter().take(40).collect::<Vec<_>>(), "accounting_deferred": accounting_only, "next": verify.json["next"], "stderr": verify.stdout.lines().filter(|l| !l.starts_with('{')).take(20).collect::<Vec<_>>()});
    out["leaks"] = json!(leaks);
    let verified = verify.code == 0 || accounting_only;
    out["ok"] = json!(verified && leaks.is_empty());
    out["next"] = json!(format!("wf mimic capture record {dir} --provision skip --publish --run {run}"));
    let state = if out["ok"] == json!(true) { "complete" } else { "complete, held back" };
    append_summary(p, &format!(
        "### Record `{}`: {state}\n\n| interactions | nondeterministic | stability | chunks | wall time | verify |\n|---|---|---|---|---|---|\n| {n} | {unstable} | {stable_pct}% | {} x {chunk} min | {} | {} |\n\n{}",
        set.name, chunks.len(), mins(wall), if verify.code == 0 { "ok" } else if accounting_only { "ok, stability allowlist to regenerate" } else { "FAILED" },
        if leaks.is_empty() { String::new() } else { format!("Token-shaped values or a declared key in the recording or the masked set (nothing is published; add a mask rule to rules.json): {}\n\n", leaks.join(", ")) }
    ));
    if out["ok"] == json!(true) && p.flag("publish") {
        publish_phase(p, set, &run, &mut out);
    }
    let code = if out["ok"] == json!(true) { 0 } else { 1 };
    Ok((out, code))
}

/// Phase four: the reference is gone and its keys are not held by anything that is still running. The masked set goes to the state ref with git in a
/// `ChildEnv` of its own (no vault token, no `GITHUB_*`). A refused push is a warning, not a red record: the set is also a workflow artifact.
fn publish_phase(p: &Parsed, set: &super::SetRef, run: &str, out: &mut Value) {
    let root = p.val("root").map_or_else(wf_kit::root::repo_root, PathBuf::from);
    let summary = p.val("summary").map(PathBuf::from).or_else(|| std::env::var_os("GITHUB_STEP_SUMMARY").map(PathBuf::from));
    match super::bundle_verb::publish(std::slice::from_ref(set), run, &root, summary, &ChildEnv::base()) {
        Ok(published) => {
            out["next"] = published["fetch"].clone();
            out["publish"] = published;
        }
        Err(f) => {
            out["publish"] = json!({"ok": false, "error": f.error, "next": f.next});
            if let Some(w) = out["warnings"].as_array_mut() {
                w.push(json!(format!("the set was not published: {}", f.error)));
            }
            append_summary(p, &format!("### Publish `{}`: refused\n\n{}\n\n", set.name, f.error));
        }
    }
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

fn replay(p: &Parsed, set: &super::SetRef, cfg: &crate::config::Config) -> Done {
    let dir = set.dir.display().to_string();
    let block = cfg.provision.as_ref().map(|prov| provision_phase::block_env(p, &set.name, &set.dir, prov)).unwrap_or_default();
    let env = ChildEnv::base().pass(&cfg.declared_pass()).with(block);
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
        let mut argv = vec!["replay", dir.as_str(), "--candidate", name.as_str()];
        if let Some(rows) = p.val("rows").filter(|r| !r.is_empty()) {
            argv.extend(["--rows", rows, "--gaps", "check"]);
        }
        for flag in ["work-root", "root"] {
            if let Some(v) = p.val(flag) {
                argv.extend([if flag == "root" { "--root" } else { "--work-root" }, v]);
            }
        }
        let c = call(&argv, &env, &[])?;
        if c.code >= 2 || c.json.is_null() {
            let mut f = fail(format!("wf mimic replay {name} could not run: {}", c.json["error"].as_str().unwrap_or("no JSON in its output")), c.json["next"].as_str().unwrap_or("fix the candidate command in mimic.toml").to_string());
            f.evidence = json!({"candidate": name, "replay": c.json});
            append_summary(p, &format!("### Replay `{}` against `{name}`: could not run\n\n{}\n\n", set.name, f.error));
            return Err(f);
        }
        ok &= c.code == 0;
        let s = &c.json["summary"];
        let mut listed = Vec::<Value>::new();
        for r in c.json["results"].as_array().into_iter().flatten().take(40) {
            let d = &r["diffs"][0];
            listed.push(json!({"id": r["id"], "label": r["label"], "severity": r["severity"], "first": {"kind": d["kind"], "path": d["path"]}, "error": r["error"]}));
            diffs.push(format!("| {name} | {} | {} | {} {} |", r["id"].as_str().unwrap_or(""), r["severity"].as_str().unwrap_or(""), d["kind"].as_str().unwrap_or(""), d["path"].as_str().unwrap_or(r["error"].as_str().unwrap_or(""))));
        }
        rows.push(json!({"candidate": name, "severity": c.json["severity"], "summary": s, "differences": listed}));
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
    let out = json!({"captured": true, "ok": ok, "action": "replay", "set": set.name, "dir": dir, "run": run_id(p), "candidates": rows, "differences": diffs.len(), "wall_seconds": started.elapsed().as_secs(), "warnings": [],
        "next": if ok { "nothing to do".to_string() } else { format!("wf mimic replay {dir} --candidate {}", wanted[0]) }});
    if let Some(o) = p.val("out") {
        let path = Path::new(o).join(format!("{}.replay.json", set.name));
        std::fs::create_dir_all(o).and_then(|()| std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap_or_default() + "\n")).map_err(|e| fail(format!("{}: {e}", path.display()), "name a writable --out"))?;
    }
    Ok((out, i32::from(!ok)))
}
