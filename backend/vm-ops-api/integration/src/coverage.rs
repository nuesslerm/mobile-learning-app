//! Which route ids of plan section 2's table have a case. A case names the routes whose contract it checks in its
//! `case!` (`case!(name, ["GET /health"], { … })`), and every route id must have one: the list of ids no case checked
//! yet shrank with each unit until B8 deleted it.
use regex::Regex;
use std::path::Path;

/// Method and path of each row of the routes table (plan section 2), in its order.
pub const ROUTE_IDS: [&str; 25] = [
    "GET /health",
    "GET /services/:app/ps",
    "GET /stats",
    "GET /services/:app/logs",
    "GET /redeploy/status",
    "POST /services/:app/redeploy",
    "POST /services/:app/retire",
    "POST /services/orca-serve/pair",
    "POST /redeploy",
    "POST /clis/install",
    "POST /state/:verb",
    "GET /state/jobs/<jobId>",
    "GET /opencode-log, /opencode-log/, /opencode-log/<runId>",
    "GET /opencode-log/api/runs",
    "GET /opencode-log/api/runs/<runId>",
    "POST /opencode-log/<runId>/start",
    "POST /opencode-log/<runId>/line",
    "POST /opencode-log/<runId>/finish",
    "POST /services/agent-shell/exec-script",
    "GET /services/agent-shell/exec-script/<jobId>",
    "POST /services/agent-shell/exec-script/<jobId>/cancel",
    "GET /services/agent-shell/vm-ledger/<name>",
    "GET /services/agent-shell/artifact",
    "POST /agent-shell/credentials",
    "any <anything else>",
];

/// One `case!` that names route ids: the case as libtest lists it (`<module>::<fn>`) and the ids.
#[derive(Debug, Clone)]
pub struct Declared {
    pub case: String,
    pub routes: Vec<String>,
}

/// The `case!` invocations with a route list in the suite's sources (`dir` holds `main.rs` and one file per module).
pub fn declared_in(dir: &Path) -> Vec<Declared> {
    let invocation = Regex::new(r"(?s)case!\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*,\s*\[([^\]]*)\]").expect("a regex");
    let quoted = Regex::new(r#""([^"]*)""#).expect("a regex");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("the suite's sources in {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    let mut declared = Vec::new();
    for file in files {
        let module = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let source = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        for c in invocation.captures_iter(&source) {
            let routes = quoted.captures_iter(&c[2]).map(|q| q[1].to_string()).collect();
            declared.push(Declared { case: format!("{module}::{}", &c[1]), routes });
        }
    }
    declared
}

/// Every reason the coverage is wrong: a route id no case checks, and any name that matches nothing (a case that names an
/// unknown id, a case with routes that is not a test of this binary).
pub fn problems(route_ids: &[&str], declared: &[Declared], tests: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let live: Vec<&Declared> = declared.iter().filter(|d| tests.contains(&d.case)).collect();
    for d in declared.iter().filter(|d| !tests.contains(&d.case)) {
        out.push(format!("{} names routes but is not a test of this binary", d.case));
    }
    for d in &live {
        for r in d.routes.iter().filter(|r| !route_ids.contains(&r.as_str())) {
            out.push(format!("{} names {r:?}, which is no route id", d.case));
        }
    }
    for id in route_ids {
        if !live.iter().any(|d| d.routes.iter().any(|r| r == id)) {
            out.push(format!("{id:?} has no case"));
        }
    }
    out
}
