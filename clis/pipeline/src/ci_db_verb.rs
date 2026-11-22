//! `wf ci db merge|record|query`: the CI database of `wf_bucket::ci_db`, which keeps what CI used to send to HEALTH_RPT. `merge` folds the
//! jobs' local files into the sealed database on the state ref (the report job), `record` adds one JSON row to the job's local file, and
//! `query` runs one SQL statement on an unsealed working copy. `--sealed-file <path>` uses a sealed file instead of the state ref.
use serde_json::{json, Map, Value};
use wf_bucket::ci_db::{self, Working};

fn fail(error: impl Into<String>, next: &str) -> i32 {
    println!("{}", json!({ "ok": false, "error": error.into(), "next": next }));
    1
}

fn take(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == flag) {
        None => Ok(None),
        Some(i) if i + 1 < args.len() => {
            let value = args.remove(i + 1);
            args.remove(i);
            Ok(Some(value))
        }
        Some(_) => Err(format!("{flag} takes a value")),
    }
}

fn working(sealed_file: &Option<String>) -> Result<Working, String> {
    match sealed_file {
        None => Working::fetch(),
        Some(path) => Working::from_sealed(ci_db::key()?, std::fs::read(path).ok()),
    }
}

fn save(work: &Working, sealed_file: &Option<String>, message: &str) -> Result<Value, String> {
    match sealed_file {
        None => work.publish(message).map(|commit| json!(commit)),
        Some(path) => std::fs::write(path, work.sealed()?).map(|()| Value::Null).map_err(|e| format!("{path}: {e}")),
    }
}

pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    let mut rest = rest.to_vec();
    let sealed_file = match take(&mut rest, "--sealed-file") {
        Ok(v) => v,
        Err(e) => return Some(fail(e, "wf ci db --help")),
    };
    Some(match verb.as_str() {
        "merge" => {
            if rest.is_empty() {
                return Some(fail("merge wants the jobs' database files", "wf ci db merge <rows.sqlite>..."));
            }
            let outcome = working(&sealed_file).and_then(|work| {
                let conn = work.connect()?;
                let mut merged = 0;
                let mut skipped = Vec::new();
                for file in rest.iter().filter(|f| std::path::Path::new(f).is_file()) {
                    match ci_db::merge(&conn, std::path::Path::new(file)) {
                        Ok(n) => merged += n,
                        Err(e) => skipped.push(json!({ "file": file, "error": e })),
                    }
                }
                // each task that ran in the merged runs gets its reason now, with the whole history in the database
                let mut runs: Vec<String> = rest.iter().flat_map(|f| crate::ci_cache_misses::runs_of(std::path::Path::new(f))).collect();
                runs.sort();
                runs.dedup();
                let mut classified = 0;
                let mut unclassified = Vec::new();
                for run in &runs {
                    match wf_bucket::miss_reason::classify_db(&conn, Some(run), true) {
                        Ok(rows) => classified += rows.len(),
                        Err(e) => unclassified.push(json!({ "run": run, "error": e })),
                    }
                }
                drop(conn);
                let published = save(&work, &sealed_file, &format!("ci db: {merged} rows"))?;
                Ok(json!({ "ok": true, "merged": merged, "skipped": skipped, "classified": classified, "unclassified": unclassified, "published": published }))
            });
            match outcome {
                Ok(out) => {
                    println!("{out}");
                    0
                }
                Err(e) => fail(e, "check CI_DB_KEY and that the state ref is reachable"),
            }
        }
        "record" => {
            let (Some(table), Some(row)) = (rest.first(), rest.get(1)) else {
                return Some(fail("record wants a table and a JSON object", "wf ci db record TEST_RESULTS '{\"suite\":\"flow\"}'"));
            };
            let row: Map<String, Value> = match serde_json::from_str::<Value>(row) {
                Ok(Value::Object(map)) => map,
                _ => return Some(fail("the row is not a JSON object", "wf ci db record <table> '<json object>'")),
            };
            match ci_db::record(table, &row) {
                Ok(recorded) => {
                    println!("{}", json!({ "ok": true, "recorded": recorded, "path": ci_db::local_path().map(|p| p.display().to_string()) }));
                    0
                }
                Err(e) => fail(e, "the tables are TEST_RESULTS, WF_USAGE and HEALTH_REPORTS"),
            }
        }
        "query" => {
            let Some(statement) = rest.first() else { return Some(fail("query wants one SQL statement", "wf ci db query 'SELECT COUNT(*) FROM CI_JOBS'")) };
            match working(&sealed_file).and_then(|work| ci_db::query(&work.connect()?, statement)) {
                Ok(rows) => {
                    println!("{}", json!({ "ok": true, "rows": rows }));
                    0
                }
                Err(e) => fail(e, "check the statement and CI_DB_KEY"),
            }
        }
        _ => return None,
    })
}
