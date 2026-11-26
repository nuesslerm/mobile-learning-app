//! `wf mimic stabilize <recording>... [--allowlist f] [--min-pct n] [--write]`: infer the mask rules that make several recordings of the
//! same corpus equal, say what still differs and why, and with `--write` apply them to the first recording (rules file, masked
//! interactions, `nondeterministic` tags, a `stability.json` report). The recordings are sets written by `wf mimic record
//! --allow-nondeterministic`; nothing here runs a reference.
use super::{fail, set_ref, Done, Fail};
use mimic_core::cassette::Interaction;
use mimic_core::stabilize::{account, stabilize, Cause, StabilityAllowlist, Stabilization};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use wf_kit::args::Parsed;

const NONDETERMINISTIC: &str = "nondeterministic";
const DEFAULT_MIN_PCT: f64 = 99.0;
pub const REPORT: &str = "stability.json";
pub const ALLOWLIST: &str = "stability-allowlist.json";

fn allowlist(p: &Parsed, base: &Path) -> Result<StabilityAllowlist, Fail> {
    let path = p.val("allowlist").map(std::path::PathBuf::from).unwrap_or_else(|| base.join(ALLOWLIST));
    match std::fs::read_to_string(&path) {
        Ok(text) => StabilityAllowlist::parse(&text).map_err(|e| fail(e, format!("fix {}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && p.val("allowlist").is_none() => Ok(StabilityAllowlist { schema: mimic_core::stabilize::ALLOWLIST_SCHEMA.into(), entries: Vec::new() }),
        Err(e) => Err(fail(format!("allowlist {}: {e}", path.display()), "fix the path or drop --allowlist")),
    }
}

fn pct(s: &Stabilization) -> f64 {
    if s.cases.is_empty() { 0.0 } else { (s.stable_after() as f64 * 10000.0 / s.cases.len() as f64).round() / 100.0 }
}

fn causes(s: &Stabilization) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for c in s.unstable() {
        *out.entry(serde_json::to_value(c.cause).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()).or_insert(0) += 1;
    }
    out
}

fn report(runs: &[String], s: &Stabilization, accounting: &Value) -> Value {
    let unstable: Vec<Value> = s.unstable().iter().map(|c| {
        let cause = c.cause.unwrap_or(Cause::TextChange);
        json!({"id": c.id, "row": c.row, "cause": cause, "reason": cause.reason(), "samples": c.samples})
    }).collect();
    json!({
        "runs": runs, "compared": s.cases.len(), "unobserved": s.unobserved, "stable_before": s.stable_before(), "stable_after": s.stable_after(),
        "stable_pct": pct(s), "causes": causes(s), "proposals": s.proposals, "unstable": unstable, "accounting": accounting,
    })
}

fn rewrite(base: &super::SetRef, mut set: mimic_core::ports::CassetteSet, s: &Stabilization) -> Result<(), Fail> {
    let compiled = s.rules.compile().map_err(|e| fail(e.to_string(), "fix the proposed rules"))?;
    let unstable: Vec<&str> = s.unstable().iter().map(|c| c.id.as_str()).collect();
    let observed = |id: &str| s.cases.iter().any(|c| c.id == id);
    let fix = |i: &Interaction| {
        let mut out = compiled.interaction(i);
        out.tags.retain(|t| t != NONDETERMINISTIC);
        if observed(&i.id) && unstable.contains(&i.id.as_str()) || !observed(&i.id) && i.tags.iter().any(|t| t == NONDETERMINISTIC) {
            out.tags.push(NONDETERMINISTIC.into());
        }
        out
    };
    set.cassette.interactions = set.cassette.interactions.iter().map(fix).collect();
    let hash = s.rules.hash();
    set.cassette.header.rules.hash = hash.clone();
    set.cassette.header.provenance.rules_hash = hash;
    set.cassette.seal();
    let rules_path = base.dir.join(&set.cassette.header.rules.path);
    let text = serde_json::to_string_pretty(&s.rules).map(|t| t + "\n").expect("rules print");
    std::fs::write(&rules_path, text).map_err(|e| fail(format!("{}: {e}", rules_path.display()), "make the set directory writable"))?;
    set.rules = s.rules.clone();
    let mut store = crate::adapters::FsStore::new(base.dir.parent().unwrap_or(Path::new(".")));
    mimic_core::ports::Store::write(&mut store, &base.name, &set).map_err(|e| fail(e.to_string(), "make the set directory writable"))
}

/// `wf mimic remask <set>`: the committed interactions masked again by the set's current `rules.json`, the rules hash in the header and
/// the provenance re-sealed. The way to land a rules change without a new recording; tags and everything else are kept.
pub fn remask_verb(p: &Parsed) -> Done {
    let base = set_ref(&p.positional[0]);
    let mut set = base.read()?;
    let compiled = set.rules.compile().map_err(|e| fail(e.to_string(), "fix rules.json"))?;
    let masked: Vec<Interaction> = set.cassette.interactions.iter().map(|i| compiled.interaction(i)).collect();
    let changed: Vec<String> = masked.iter().zip(&set.cassette.interactions).filter(|(a, b)| a != b).map(|(a, _)| a.id.clone()).collect();
    let (before, hash) = (set.cassette.header.rules.hash.clone(), set.rules.hash());
    set.cassette.interactions = masked;
    set.cassette.header.rules.hash = hash.clone();
    set.cassette.header.provenance.rules_hash = hash.clone();
    set.cassette.seal();
    let mut store = crate::adapters::FsStore::new(base.dir.parent().unwrap_or(Path::new(".")));
    mimic_core::ports::Store::write(&mut store, &base.name, &set).map_err(|e| fail(e.to_string(), "make the set directory writable"))?;
    let out = json!({"ok": true, "set": base.name, "rules_hash": {"before": before, "after": hash}, "changed": changed, "warnings": [],
        "next": format!("wf mimic verify {}", base.dir.display())});
    Ok((out, 0))
}

pub fn stabilize_verb(p: &Parsed) -> Done {
    if p.positional.len() < 2 {
        return Err(fail("name at least two recordings of the same corpus", "wf mimic record <set> --allow-nondeterministic, copy the set, record again, then wf mimic stabilize <set> <copy>"));
    }
    let min_pct = match p.val("min-pct") {
        None => DEFAULT_MIN_PCT,
        Some(v) => v.parse::<f64>().map_err(|_| fail(format!("--min-pct {v} is not a number"), "wf mimic stabilize --help"))?,
    };
    let sets: Vec<_> = p.positional.iter().map(|a| set_ref(a)).collect();
    let contents: Vec<_> = sets.iter().map(|s| s.read()).collect::<Result<_, _>>()?;
    let runs: Vec<Vec<Interaction>> = contents.iter().map(|c| c.cassette.interactions.clone()).collect();
    let mut warnings: Vec<String> = Vec::new();
    for (s, c) in sets.iter().zip(&contents).skip(1) {
        if c.rules.hash() != contents[0].rules.hash() {
            warnings.push(format!("{} was recorded under other rules than {}: the first recording's rules are the starting point", s.name, sets[0].name));
        }
    }
    let stabilization = stabilize(&contents[0].rules, &runs).map_err(|e| fail(e.to_string(), "fix rules.json"))?;
    if stabilization.cases.is_empty() {
        return Err(fail("the recordings share no interaction id", "record the same corpus each time"));
    }
    let allow = allowlist(p, &sets[0].dir)?;
    let accounting = account(&stabilization, &allow);
    let acc = serde_json::to_value(&accounting).expect("accounting serialises");
    let names: Vec<String> = sets.iter().map(|s| s.name.clone()).collect();
    let body = report(&names, &stabilization, &acc);
    let ok = accounting.clean() && pct(&stabilization) >= min_pct;
    let mut wrote: Vec<String> = Vec::new();
    if p.flag("write") {
        let report_path = p.val("report").map(std::path::PathBuf::from).unwrap_or_else(|| sets[0].dir.join(REPORT));
        let text = serde_json::to_string_pretty(&body).map(|t| t + "\n").expect("report prints");
        std::fs::write(&report_path, text).map_err(|e| fail(format!("{}: {e}", report_path.display()), "make the set directory writable"))?;
        rewrite(&sets[0], contents[0].clone(), &stabilization)?;
        wrote.push(sets[0].dir.join(&contents[0].cassette.header.rules.path).display().to_string());
        wrote.push(sets[0].dir.join("interactions.jsonl").display().to_string());
        wrote.push(report_path.display().to_string());
    }
    wf_kit::logfmt::mcp("info", "mimic", "stabilize.done", &[("stable_pct", pct(&stabilization).to_string()), ("proposals", stabilization.proposals.len().to_string())]);
    let mut out = body;
    let next = match (ok, p.flag("write")) {
        (true, true) => "wf mimic verify <set>".to_string(),
        (true, false) => format!("wf mimic stabilize {} --write", p.positional.join(" ")),
        (false, _) if !accounting.clean() => format!("list each unstable id in {ALLOWLIST} with the cause shown and a reason, or remove the cause at its source (driver state, a seed)"),
        _ => "record more or fix the sources of the unstable cases (see unstable[].cause)".to_string(),
    };
    out["ok"] = json!(ok);
    out["min_pct"] = json!(min_pct);
    out["wrote"] = json!(wrote);
    out["warnings"] = json!(warnings);
    out["next"] = json!(next);
    Ok((out, if ok { 0 } else { 1 }))
}

/// A set's instability is accounted for: every interaction tagged `nondeterministic` is in `stability-allowlist.json` with a reason, and
/// every entry names a tagged interaction, so the tags and the allowlist cannot drift apart.
pub fn accounting_problems(dir: &Path, interactions: &[Interaction]) -> Vec<mimic_core::cassette::Problem> {
    let problem = |message: String| mimic_core::cassette::Problem { field: "stability".into(), message };
    let tagged: Vec<&str> = interactions.iter().filter(|i| i.tags.iter().any(|t| t == NONDETERMINISTIC)).map(|i| i.id.as_str()).collect();
    let path = dir.join(ALLOWLIST);
    let entries = match std::fs::read_to_string(&path) {
        Ok(text) => match StabilityAllowlist::parse(&text) {
            Ok(a) => a.entries,
            Err(e) => return vec![problem(e)],
        },
        Err(_) => Vec::new(),
    };
    let mut out: Vec<_> = tagged.iter().filter(|id| !entries.iter().any(|e| e.id == **id)).map(|id| problem(format!("{id} is tagged nondeterministic but {ALLOWLIST} does not list it with a cause and a reason (wf mimic stabilize)"))).collect();
    out.extend(entries.iter().filter(|e| !tagged.contains(&e.id.as_str())).map(|e| problem(format!("{ALLOWLIST} lists {}, which is not tagged nondeterministic: stale entry", e.id))));
    out
}
