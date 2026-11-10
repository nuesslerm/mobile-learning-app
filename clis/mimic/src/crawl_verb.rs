//! `wf mimic crawl <dir>... | --all [--check [--fresh]]`: derive a reference's endpoint inventory and request corpus from its pinned source,
//! guard their completeness against the committed catalog, and write `inventory.json`, `corpus.json` and `coverage.json` beside `crawl.toml`.
use super::{fail, Done, Fail};
use crate::adapters::CommandCrawler;
use crate::config::CrawlConfig;
use mimic_core::crawl::{self, Catalog, Crawled, Outputs, Problem};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use wf_kit::args::Parsed;

const FILES: [&str; 3] = ["inventory.json", "corpus.json", "coverage.json"];

fn find(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut sorted: Vec<_> = entries.flatten().collect();
    sorted.sort_by_key(|e| e.file_name());
    for e in sorted {
        let path = e.path();
        if path.is_dir() && !super::SKIP.iter().any(|s| e.file_name() == *s) {
            if path.join("crawl.toml").is_file() {
                out.push(path.clone());
            }
            find(&path, out);
        }
    }
}

fn read_catalog(cfg: &CrawlConfig) -> Result<Option<Catalog>, Fail> {
    let Some(path) = &cfg.catalog else { return Ok(None) };
    let text = std::fs::read_to_string(path).map_err(|e| fail(format!("catalog {}: {e}", path.display()), "run wf parity coverage, or fix catalog in crawl.toml"))?;
    Catalog::parse(&text).map(Some).map_err(|e| fail(format!("catalog {}: {e}", path.display()), "regenerate it with wf parity coverage"))
}

fn committed(dir: &Path) -> [Option<String>; 3] {
    FILES.map(|f| std::fs::read_to_string(dir.join(f)).ok())
}

fn differing(dir: &Path, fresh: &Outputs) -> Vec<&'static str> {
    let old = committed(dir);
    let new = [&fresh.inventory, &fresh.corpus, &fresh.coverage];
    FILES.iter().zip(old.iter().zip(new)).filter(|(_, (o, n))| o.as_deref() != Some(n.as_str())).map(|(f, _)| *f).collect()
}

fn problems_json(p: &[Problem]) -> Value {
    json!(p)
}

fn one(dir: &Path, check: bool, fresh: bool) -> Value {
    let name = dir.display().to_string();
    let fail_set = |error: String, next: String| json!({"dir": name, "ok": false, "error": error, "next": next});
    let cfg = match CrawlConfig::load(&dir.join("crawl.toml")) {
        Ok(c) => c,
        Err(e) => return fail_set(e, "fix crawl.toml (wf mimic crawl --help documents it)".into()),
    };
    let catalog = match read_catalog(&cfg) {
        Ok(c) => c,
        Err(f) => return fail_set(f.error, f.next),
    };
    let pin_commit = match cfg.pin_commit() {
        Ok(c) => c,
        Err(e) => return fail_set(e, "fix pin in crawl.toml".into()),
    };
    let rerun = format!("wf mimic crawl {name}");
    let crawled: Crawled = if check && !fresh {
        let (inv, corp) = (std::fs::read_to_string(dir.join("inventory.json")), std::fs::read_to_string(dir.join("corpus.json")));
        let (Ok(inv), Ok(corp)) = (inv, corp) else { return fail_set("inventory.json or corpus.json is missing".into(), rerun) };
        let report = match crawl::from_files(&inv, &corp) {
            Ok(r) => r,
            Err(e) => return fail_set(e, rerun),
        };
        let (problems, warnings) = crawl::check(&report, catalog.as_ref(), pin_commit.as_deref());
        Crawled { outputs: crawl::outputs(&report), summary: crawl::summary(&report), report, problems, warnings }
    } else {
        match crawl::crawl(&mut CommandCrawler::new(cfg.spec.clone()), catalog.as_ref(), pin_commit.as_deref()) {
            Ok(c) => c,
            Err(e) => return fail_set(e.to_string(), "check the reference's source is present (pnpm run submodule:init, or MIMIC_T3_ROOT / MIMIC_ORCA_ROOT) and the command of crawl.toml runs".into()),
        }
    };
    let mut problems = crawled.problems.clone();
    let stale = differing(dir, &crawled.outputs);
    let mut wrote: Vec<String> = Vec::new();
    if check {
        let stale_msg = if fresh { "the reference's source gives another crawl than the committed one" } else { "a committed file is not what its inventory and corpus derive" };
        problems.extend(stale.iter().map(|f| Problem { code: "stale-output", id: (*f).into(), detail: stale_msg.into() }));
    } else if problems.is_empty() {
        for (file, text) in FILES.iter().zip([&crawled.outputs.inventory, &crawled.outputs.corpus, &crawled.outputs.coverage]) {
            if stale.contains(file) {
                match std::fs::write(dir.join(file), text) {
                    Ok(()) => wrote.push(dir.join(file).display().to_string()),
                    Err(e) => return fail_set(format!("{}: {e}", dir.join(file).display()), "make the directory writable".into()),
                }
            }
        }
        wf_kit::logfmt::mcp("info", "mimic", "crawl.done", &[("app", cfg.app.clone()), ("endpoints", crawled.summary.endpoints.to_string()), ("cases", crawled.summary.cases.to_string())]);
    }
    let ok = problems.is_empty();
    let next = match (ok, problems.first()) {
        (true, _) => "nothing to do".to_string(),
        (false, Some(p)) if p.code == "lags-source" || p.code == "uncovered" || p.code == "stale-pin" || p.code == "stale-output" => format!("{rerun}  (the crawl no longer matches the pinned source; fix the crawler if it still lags, then commit the files)"),
        (false, _) => "fix the crawler so every endpoint it lists has cases of each class, then re-run".to_string(),
    };
    json!({"dir": name, "app": cfg.app, "ok": ok, "mode": if check { if fresh { "check-fresh" } else { "check" } } else { "crawl" }, "summary": crawled.summary,
        "problems": problems_json(&problems), "warnings": crawled.warnings, "changed": stale, "wrote": wrote, "next": next})
}

pub fn crawl_verb(p: &Parsed) -> Done {
    let mut dirs: Vec<PathBuf> = p.positional.iter().map(PathBuf::from).collect();
    if p.flag("all") {
        let root = p.val("root").map_or_else(wf_kit::root::repo_root, PathBuf::from);
        find(&root, &mut dirs);
    }
    if dirs.is_empty() {
        return Err(fail("no crawl directory: name one (it holds a crawl.toml) or pass --all", "wf mimic crawl <dir>... | --all [--root <dir>] [--check [--fresh]]"));
    }
    if p.flag("fresh") && !p.flag("check") {
        return Err(fail("--fresh compares a new crawl with the committed files, so it needs --check", "wf mimic crawl <dir> --check --fresh"));
    }
    let sets: Vec<Value> = dirs.iter().map(|d| one(d, p.flag("check"), p.flag("fresh"))).collect();
    let ok = sets.iter().all(|s| s["ok"] == true);
    let next = sets.iter().find(|s| s["ok"] != true).and_then(|s| s["next"].as_str()).unwrap_or("nothing to do").to_string();
    Ok((json!({"ok": ok, "sets": sets, "next": next, "warnings": []}), i32::from(!ok)))
}
