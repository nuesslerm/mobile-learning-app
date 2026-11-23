//! `wf ci plan needs`: the tools one `core-tests` machine installs. The machine runs flow shard i beside typecheck shard `count + 1 - i`; the union of
//! what its units need (`data/test-needs.json`) is printed, and a unit the data does not list needs everything, so a missing entry costs time, never a red shard.
use crate::ci_plan::github_output;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(crate) const TOOLS: [&str; 9] = ["playwright", "ffmpeg", "restic-sqlite", "bun", "java", "rust", "references", "opencode", "browser-harness"];

pub(crate) fn mapping() -> Value {
    serde_json::from_str(include_str!("../data/test-needs.json")).expect("test-needs.json parses")
}

fn unit_name(t: &Value) -> &str {
    t["package"].as_str().or_else(|| t["crate"].as_str()).unwrap_or_default()
}

/// The tools the units of the machine need; every tool when a unit is not listed.
pub(crate) fn needs_in_plan(plan: &Value, index: usize, count: usize, map: &Value) -> BTreeSet<&'static str> {
    let flow = crate::test_plan::shard_in_plan("flow-tests", plan, index, count);
    let unit = crate::test_plan::shard_in_plan("typecheck-lint-unit-tests", plan, count + 1 - index, count);
    let mut tools = BTreeSet::new();
    for t in flow.iter().chain(unit.iter()) {
        match map["units"][unit_name(t)].as_array() {
            Some(listed) => tools.extend(TOOLS.iter().copied().filter(|k| listed.iter().any(|l| l == k))),
            None => return TOOLS.iter().copied().collect(),
        }
    }
    tools
}

pub(crate) fn run(args: &[String]) -> Option<i32> {
    use wf_kit::args::{parse_spec, Positional, Spec};
    let spec = Spec { values: &["stage", "shard", "count", "plan"], flags: &["github-output"], positional: Positional::None, ..Spec::NONE };
    let p = parse_spec(args, &spec)?;
    let refuse = |error: &str| {
        println!("{}", json!({ "schema": "ci-plan/1", "ok": false, "error": error, "next": "wf ci plan needs --stage core --shard 1 --count 6 --plan plan.json", "warnings": [] }));
        Some(2)
    };
    if p.val("stage") != Some("core") {
        return refuse("needs wants --stage core");
    }
    let (Some(plan_path), Some(shard), Some(count)) = (p.val("plan"), p.val("shard").and_then(|s| s.parse::<usize>().ok()), p.val("count").and_then(|s| s.parse::<usize>().ok())) else {
        return refuse("needs wants --plan, --shard and --count");
    };
    if shard < 1 || shard > count {
        return refuse("--shard is from 1 to --count");
    }
    let plan: Value = match std::fs::read_to_string(plan_path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(plan) => plan,
        Err(e) => return refuse(&format!("cannot read the plan {plan_path}: {e}")),
    };
    let tools = needs_in_plan(&plan, shard, count, &mapping());
    let pairs: Vec<(&str, String)> = TOOLS.iter().map(|k| (*k, tools.contains(k).to_string())).collect();
    println!("{}", Value::Object(pairs.iter().map(|(k, v)| (k.to_string(), json!(v == "true"))).collect()));
    if p.flag("github-output") {
        if let Err(e) = github_output(&pairs) {
            return refuse(&e);
        }
    }
    Some(0)
}
