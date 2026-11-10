//! `wf flow run` and `wf flow goldens`: the flow harness (vitest over the app's `*.flow.test.tsx`, harness doubles, jsdom) as the third runner
//! beside `wf browser test` and `wf android test`. Flow tests are TypeScript, so there is no Lua script; the report is the
//! reporter's `.flow-report/{report.json,index.html}` and the logpoints are the tests' own `logAt`/`breakAt` calls.
use wf_kit::args::{parse_spec, Positional, Spec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

const SPEC: Spec = Spec { values: &["file", "app", "theme"], flags: &["publish", "json", "goldens", "update-goldens"], positional: Positional::AtMost(1), ..Spec::NONE };

fn app_dir(app: &str) -> PathBuf {
    wf_kit::root::repo_root().join("frontend").join(app)
}

/// `wf flow run [<test file>] [--file <test file>] [--app mla-tamagui] [--goldens | --update-goldens] [--theme light|dark|both]
/// [--publish]`: exit code is vitest's, or 1 when a golden differs; stdout ends with `report: <path>` and, with `--publish`,
/// `bundle: <url>`.
pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    if verb == "goldens" {
        return crate::flow_goldens::run(rest);
    }
    if verb != "run" {
        return None;
    }
    let p = parse_spec(rest, &SPEC)?;
    let app = p.val("app").unwrap_or("mla-tamagui").to_string();
    let file = p.val("file").map(str::to_string).or_else(|| p.positional.first().cloned());
    let dir = app_dir(&app);
    let report_dir = dir.join(".flow-report");
    let _ = std::fs::create_dir_all(&report_dir);
    let goldens = p.flag("goldens") || p.flag("update-goldens");
    // every run paints its screenshots; a plain run in the light theme only, a golden run in both unless --theme says
    let theme = p.val("theme").unwrap_or(if goldens { "both" } else { "light" });
    let themes: Vec<&str> = if theme == "both" { vec!["light", "dark"] } else { vec![theme] };
    harness_core::run_stop::arm();
    let mut code = 0;
    for theme in themes {
        let mut cmd = Command::new("pnpm");
        cmd.args(["exec", "vitest", "run", "--config", "vitest.flow.config.ts", "--configLoader", "runner", "--passWithNoTests"]);
        if let Some(f) = &file {
            cmd.arg(f);
        }
        cmd.current_dir(&dir).env("FLOW_DEBUG_OUT", report_dir.join("debug.jsonl"));
        // each test freezes its screenshots as HTML; jsdom cannot paint, so Chromium renders the frozen pages after
        let snapshots = dir.join(".flow-goldens").join(theme).join("snapshots");
        let _ = std::fs::remove_dir_all(dir.join(".flow-goldens").join(theme));
        cmd.env("FLOW_THEME", theme).env("FLOW_GOLDENS_OUT", &snapshots);
        if goldens {
            cmd.env("FLOW_GOLDENS", "1");
        }
        let status = match cmd.status() {
            Ok(s) => s,
            Err(e) => {
                println!("{}", json!({ "error": format!("cannot run vitest in {}: {e}", dir.display()), "next": "pnpm install, then wf flow run" }));
                return Some(1);
            }
        };
        code = code.max(status.code().unwrap_or(1));
        let mode = if p.flag("update-goldens") { "update" } else if goldens { "compare" } else { "save" };
        code = code.max(check_goldens(&dir, theme, &snapshots, mode, file.is_some()));
    }
    let report = report_dir.join("report.json");
    println!("report: {}", report_dir.join("index.html").display());
    let summary = summary(&report, code);
    if p.flag("json") {
        println!("{summary}");
    }
    if p.flag("publish") {
        let ran_on = std::env::var("WF_SITE").unwrap_or_else(|_| wf_kit::proc::hostname());
        match harness_core::run::publish(&report_dir, &ran_on, true) {
            Ok(base) => println!("bundle: {base}/bundle.json"),
            Err(e) => wf_kit::wf_err!("wf: the flow run's bundle was not published: {e}"),
        }
    }
    Some(code)
}

/// `wf flow goldens` over one theme's frozen pages: paints them (`save`), compares with `tests/goldens`, or rewrites them.
fn check_goldens(dir: &Path, theme: &str, snapshots: &Path, mode: &str, one_file: bool) -> i32 {
    let Ok(exe) = harness_core::device::self_exe() else { return 1 };
    let report = dir.join(".flow-goldens").join(theme).join("report.html");
    let mut cmd = Command::new(exe);
    cmd.current_dir(dir).args(["flow", "goldens", "--snapshots"]).arg(snapshots).args(["--goldens", "tests/goldens", "--color-scheme", theme, "--threshold", "0.0001", "--report"]).arg(&report);
    match mode {
        "update" => {
            cmd.arg("--update");
        }
        "save" => {
            cmd.arg("--save");
        }
        _ => {}
    }
    if one_file {
        cmd.arg("--keep-unused");
    }
    let code = cmd.status().map_or(1, |s| s.code().unwrap_or(1));
    println!("goldens ({theme}): {}", report.display());
    code
}

/// What a reader or the queue needs in one line: verdict, counts, where the page is.
pub fn summary(report: &Path, code: i32) -> Value {
    let r: Value = std::fs::read_to_string(report).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    json!({
        "verdict": r["verdict"].as_str().unwrap_or(if code == 0 { "pass" } else { "fail" }),
        "exitCode": code,
        "tests": r["summary"]["total"],
        "failed": r["summary"]["failed"],
        "files": r["summary"]["files"],
        "durationMs": r["durationMs"],
        "report": report.with_file_name("index.html").display().to_string(),
        "debug": report.with_file_name("debug.jsonl").display().to_string(),
    })
}
