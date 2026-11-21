//! `wf ci proven --stage <s> --count <n> --plan <file>`: which shards of a stage already passed on the same inputs. A passing shard's job saves a GitHub Actions
//! cache entry `proven-<stage>-<fingerprint>` (a job cannot push the state ref); the plan job asks the cache API for each shard's entry on the run's own ref and the
//! workflow's matrix keeps only the shards without one. Anything unprovable or unreadable runs.
use crate::ci_comment::{json_of, send, var};
use crate::ci_timings::api_env;
use serde_json::{json, Value};
use std::path::PathBuf;
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { values: &["stage", "count", "plan", "repo"], flags: &["fresh", "no-lookup", "github-output"], positional: Positional::None, ..Spec::NONE };

fn refused(error: &str, next: &str) -> Option<i32> {
    println!("{}", json!({ "schema": "ci-proven/1", "ok": false, "error": error, "next": next, "warnings": [] }));
    Some(2)
}

/// True when the cache `key` exists on `branch_ref` (an entry of another ref, such as a pull request's, proves nothing for this one).
fn marked(env: &crate::ci_comment::Env, key: &str, branch_ref: &str) -> Result<bool, String> {
    let url = format!("{}/repos/{}/actions/caches?key={key}&per_page=100", env.api, env.repo);
    let body = json_of(send(env, "GET", &url, None)?, "listing the proven markers")?;
    let entries = body["actions_caches"].as_array().cloned().unwrap_or_default();
    Ok(entries.iter().any(|e| e["key"] == key && (branch_ref.is_empty() || e["ref"] == branch_ref)))
}

fn proven(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    let (Some(stage), Some(plan_path)) = (p.val("stage"), p.val("plan")) else { return refused("proven wants --stage and --plan", "wf ci proven --stage integration-tests --count 4 --plan plan.json") };
    let count = p.val("count").and_then(|c| c.parse::<usize>().ok()).filter(|c| *c >= 1).unwrap_or(1);
    let plan: Value = match std::fs::read_to_string(plan_path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(plan) => plan,
        Err(e) => return refused(&format!("cannot read the plan {plan_path}: {e}"), "wf ci plan tests --json > plan.json"),
    };
    let repo = p.val("repo").map(PathBuf::from).unwrap_or_else(wf_kit::root::repo_root);
    let sha = plan["sha"].as_str().filter(|s| !s.is_empty()).unwrap_or("HEAD").to_string();
    let mut warnings: Vec<String> = Vec::new();
    let lookup = !p.flag("fresh") && !p.flag("no-lookup");
    let env = if lookup {
        api_env(Some("0".to_string())).map_err(|e| warnings.push(format!("nothing is proven: {e}"))).ok()
    } else {
        None
    };
    let branch_ref = var("GITHUB_REF").unwrap_or_default();
    let mut rows: Vec<Value> = Vec::new();
    let (mut run, mut skip, mut keys) = (Vec::new(), Vec::new(), vec![String::new()]);
    for shard in 1..=count {
        let (key, why, is_proven) = match crate::test_plan::shard_proof_in_plan(stage, &plan, shard, count, &repo, &sha) {
            Err(why) => (None, why, false),
            Ok(fingerprint) => {
                let key = format!("proven-{stage}-{fingerprint}");
                match env.as_ref().filter(|_| lookup).map(|e| marked(e, &key, &branch_ref)) {
                    Some(Ok(true)) => (Some(key), "a job marked this fingerprint after it passed".to_string(), true),
                    Some(Ok(false)) => (Some(key), "no marker for this fingerprint".to_string(), false),
                    Some(Err(e)) => {
                        warnings.push(e);
                        (Some(key), "the marker could not be read".to_string(), false)
                    }
                    None => (Some(key), if p.flag("fresh") { "a fresh run proves nothing" } else { "no lookup" }.to_string(), false),
                }
            }
        };
        keys.push(if p.flag("fresh") { String::new() } else { key.clone().unwrap_or_default() });
        (if is_proven { &mut skip } else { &mut run }).push(shard);
        rows.push(json!({ "shard": shard, "key": key, "proven": is_proven, "why": why }));
    }
    if p.flag("github-output") {
        let name = stage.replace('-', "_");
        if let Err(e) = crate::ci_plan::github_output(&[(format!("{name}_run_shards"), json!(run).to_string()), (format!("{name}_proof_keys"), json!(keys).to_string())]) {
            return refused(&e, "set GITHUB_OUTPUT");
        }
    }
    println!("{}", json!({ "schema": "ci-proven/1", "ok": true, "stage": stage, "count": count, "run": run, "proven": skip, "shards": rows, "warnings": warnings }));
    Some(0)
}

/// `wf ci proven ...`.
pub fn run(verb: &str, rest: &[String]) -> Option<i32> {
    (verb == "proven").then(|| proven(rest)).flatten()
}
