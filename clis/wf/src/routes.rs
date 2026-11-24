//! The command surface: every `wf` command by its first word, and the small routing each one does among its own words.
//! There is one spelling for everything; a first word not listed here is an unknown command.
//!
//! A handler gets the words after its command word and returns `None` for a shape it does not serve, which is reported as
//! unsupported. Verbs: `check`, `ship`, `rca` (`run`, `ask`, `plan` and `worker` only say they are gone). Devices: `android`, `browser`. Nouns: `vm`, `secret`, `repo`, `cache`, `lib`, `t3`, `exec`, `mimic`, `ci`.
//! `t3`, `exec` and `mimic` run separate binaries (`external`), and so does `help` for them.
use crate::{external, log_emit};
use pipeline::{arch_rules, dead_refs, e2e_run, export_site, test_results, visual};
use wf_kit::args::{parse_spec, probe, Positional, Spec};
use pipeline::verify;

/// The commands. The last four are plumbing that hooks and the browser SDK call: reachable, not listed in `--help`.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[
    ("check", check),
    ("ship", deploy::run),
    ("ask", ask_gone),
    ("plan", plan_gone),
    ("run", wf_harness::run::gone),
    ("flow", harness_flow::COMMANDS[0].1),
    ("test", test),
    ("ci", pipeline::ci),
    ("android", wf_harness::android::run),
    ("browser", wf_harness::browser::run),
    ("vm", deploy::run_vm),
    ("secret", secret),
    ("repo", repo),
    ("bench", wf_bench::run),
    ("knowledge", knowledge::run),
    ("content", content),
    ("agent-config", agent_config_sync::verb::run),
    ("rca", rca::rca),
    ("cache", wf_bucket::cache::run),
    ("bucket", wf_bucket::bucket_run),
    ("lib", wf_lib::run),
    ("t3", external::t3),
    ("exec", external::exec),
    ("mimic", external::mimic),
    ("help", external::help),
    ("log", log),
    ("worker", worker_gone),
    ("browser-daemon", wf_harness::browser_daemon::run),
    ("mcp-guard", mcp_guard),
    ("build-sha", build_sha),
];

fn content(args: &[String]) -> Option<i32> {
    let [verb, deck, options @ ..] = args else { return None; };
    if verb != "import-tamil" || options.iter().any(|option| option != "--check") {
        return None;
    }
    let root = wf_kit::root::repo_root();
    let status = std::process::Command::new("python3")
        .arg(root.join("clis/anki-content/import_tamil.py"))
        .arg(deck)
        .arg(root.join("frontend/mla-tamagui/features/tamil-writing/data/lessons"))
        .args(options)
        .status();
    Some(match status {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("{}", serde_json::json!({ "error": error.to_string() }));
            1
        }
    })
}

/// The commit this binary was built from (the build passes it in), printed by `wf --version`.
fn build_sha(_: &[String]) -> Option<i32> {
    println!("{}", option_env!("WF_BUILD_SHA").unwrap_or("unknown"));
    Some(0)
}

pub fn is_command(name: &str) -> bool {
    COMMANDS.iter().any(|(n, _)| *n == name)
}

fn words(parts: &[&str], rest: &[String]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).chain(rest.iter().cloned()).collect()
}

/// `args` without the first `flag`, when it is there.
fn without(args: &[String], flag: &str) -> Option<Vec<String>> {
    let at = args.iter().position(|a| a == flag)?;
    let mut rest = args.to_vec();
    rest.remove(at);
    Some(rest)
}

/// `check`: `--staged`, `--budget` and `--deployed <sha>` are the verifier's own modes; `--health`, `--visual` and `--drift` are
/// the other gates.
fn check(rest: &[String]) -> Option<i32> {
    if rest.iter().any(|a| a == "--health") {
        return deploy::check(rest);
    }
    if let Some(a) = without(rest, "--visual") {
        return visual::run(&a);
    }
    if without(rest, "--drift").is_some_and(|a| a.is_empty()) {
        return Some(drift());
    }
    if let Some(left) = without(rest, "--dead-refs") {
        if left.is_empty() || left == ["--staged"] {
            return Some(dead_refs::run(&wf_kit::root::repo_root(), !left.is_empty()));
        }
    }
    if rest == ["--arch"] {
        return Some(arch_rules::run(&wf_kit::root::repo_root()));
    }
    let args = without(rest, "--deployed").unwrap_or_else(|| rest.to_vec());
    // The mode is the one whose option is on the line, wherever it stands.
    let modes = probe(&args, &[&verify::STAGED, &pipeline::pushed::PUSHED, &deploy::verify_tier3::DRY_RUN, &verify::BUDGET, &deploy::verify_tier3::SPEC, &pipeline::layout::LAYOUT])?;
    if modes.flag("layout") {
        pipeline::layout::run(&args)
    } else if modes.flag("pushed") {
        pipeline::pushed::run(&args)
    } else if modes.flag("staged") {
        verify::staged(&args, |root| dead_refs::gate(root).or_else(|| arch_rules::gate(root)))
    } else if modes.flag("dry-run") {
        deploy::verify_tier3::dry_run(&args)
    } else if modes.flag("budget") || modes.flag("unused-workspace-deps") || modes.flag("unused-deps") || modes.flag("workspace-crossings") || modes.flag("ts-packages") || modes.flag("cargo-inputs") {
        verify::budget(&args)
    } else {
        deploy::verify_tier3::run(&args)
    }
}

/// `check --drift`: the generated agent config, the editor bundle, the Pyodide bundle and the JavaScript runner's page (the app's own `check:editor-bundle`, `check:pyodide` and `check:js-runner` scripts) are all fresh.
fn drift() -> i32 {
    let config = agent_config_sync::verb::run(&words(&["sync", "--check"], &[])).unwrap_or(2);
    let check = |script: &str| {
        std::process::Command::new("pnpm")
            .args(["--filter", "mla-tamagui", "run", script])
            .current_dir(wf_kit::root::repo_root())
            .status()
            .map_or(2, |s| s.code().unwrap_or(2))
    };
    let (bundle, pyodide, js_runner) = (check("check:editor-bundle"), check("check:pyodide"), check("check:js-runner"));
    [config, bundle, pyodide, js_runner].into_iter().find(|code| *code != 0).unwrap_or(0)
}

/// `test`: planning and running the CI stages (`pipeline`), the e2e runner, preparing the web export, publishing a run, and the results of runs.
fn test(rest: &[String]) -> Option<i32> {
    match rest.first().map(String::as_str) {
        Some("metrics") => {
            println!("wf test metrics is gone: `wf test publish <run dir>` uploads the run and posts its test_runs row in one call");
            return Some(2);
        }
        Some("prepare") => return Some(export_site::run(&rest[1..])),
        Some("export") => {
            println!("wf test export is now `wf test prepare --web`: it prepares what the browser e2e tests run against, which is not a test");
            return Some(2);
        }
        Some("drift" | "backends") => return pipeline::run_test(rest),
        Some("publish") => return pipeline::run_test(rest),
        Some("goldens" | "ios-visual") => return wf_harness::run_test(rest),
        Some("run") if rest.get(1).is_some_and(|a| a == "--performance") => return wf_kit::performance::integration(&rest[1..]),
        Some("run") if rest.iter().any(|a| a == "--stage") => return pipeline::run_test(rest),
        Some("run") => return Some(e2e_run::run(&rest[1..])),
        Some("record") => {
            println!("wf test record is gone: the published bundles are the record, and the test-report skill writes the report");
            return Some(2);
        }
        _ => {}
    }
    let p = parse_spec(rest, &Spec { values: &["query"], flags: &["json"], positional: Positional::Any, ..Spec::NONE })?;
    Some(match p.positional.first().map(String::as_str).unwrap_or("") {
        "results" => test_results::run(p.val("query").unwrap_or(""), p.flag("json")),
        gone @ ("device" | "enqueue" | "worker" | "list" | "status" | "result" | "logs" | "wait" | "cancel" | "fetch" | "sync-wf") => {
            println!("{}", serde_json::json!({ "error": format!("wf test {gone} is gone: e2e tests run in the e2e-tests workflow (wf test run), no queue"), "next": "wf test run <test>... | --changed [--base <ref>] | --all [-- <run flags>]" }));
            2
        }
        other => {
            println!("{}", serde_json::json!({ "error": format!("unknown action '{other}'"), "next": "wf test --help" }));
            2
        }
    })
}

/// `ask`, `plan` and `worker` live under `rca` now: each prints the spelling that replaced it and exits 2.
fn moved(old: &str, new: &str, rest: &[String]) -> Option<i32> {
    println!("{}", serde_json::json!({ "error": format!("wf {old} is gone: it is `wf rca {new}`"), "next": format!("wf rca {new} {}", rest.join(" ")).trim() }));
    Some(2)
}

fn ask_gone(rest: &[String]) -> Option<i32> {
    moved("ask", "ask", rest)
}

fn plan_gone(rest: &[String]) -> Option<i32> {
    moved("plan", "plan", rest)
}

/// `worker visual` was the vision judge, which is `check --visual`; every other `worker` shape is the git-state worker.
fn worker_gone(rest: &[String]) -> Option<i32> {
    match rest.split_first() {
        Some((sub, tail)) if sub == "visual" => {
            println!("{}", serde_json::json!({ "error": "wf worker visual is gone: it is `wf check --visual`", "next": format!("wf check --visual {}", tail.join(" ")).trim() }));
            Some(2)
        }
        _ => moved("worker", "worker", rest),
    }
}

/// `secret`: the vault, and `login`, which captures a tool's own login.
fn secret(rest: &[String]) -> Option<i32> {
    match rest.split_first() {
        Some((sub, tail)) if sub == "login" => wf_login::run(tail),
        _ => secrets::run(rest),
    }
}

/// `repo` is gone: each of its verbs lives with the concept it belongs to.
fn repo(rest: &[String]) -> Option<i32> {
    let new = match rest.split_first()? {
        (verb, tail) if verb == "bench" => format!("wf bench {}", tail.join(" ")),
        (verb, tail) if verb == "knowledge" => format!("wf knowledge {}", tail.join(" ")),
        (verb, tail) if verb == "sync" => format!("wf agent-config sync {}", tail.join(" ")),
        (verb, tail) if verb == "ladder" => format!("wf rca ladder {}", tail.join(" ")),
        (verb, _) if verb == "bundle" => "pnpm --filter mla-tamagui run build:editor-bundle (or check:editor-bundle)".to_string(),
        _ => return None,
    };
    println!("{}", serde_json::json!({ "error": "wf repo is gone: its verbs live with what they work on", "next": new.trim() }));
    Some(2)
}

/// `wf log emit ...`; every other `log` shape is unsupported.
fn log(args: &[String]) -> Option<i32> {
    match args.split_first() {
        Some((sub, rest)) if sub == "emit" => log_emit::run(rest),
        _ => None,
    }
}

/// `mcp-guard` is the hook plumbing's old spelling of `agent-config mcp-guard`.
fn mcp_guard(rest: &[String]) -> Option<i32> {
    println!("{}", serde_json::json!({ "error": "wf mcp-guard is gone", "next": format!("wf agent-config mcp-guard {}", rest.join(" ")).trim() }));
    Some(2)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_thin_pipeline_binary_knows_every_word_the_router_routes() {
        // Given the router's table and the word list the thin binary scans dead references with
        let routed: Vec<&str> = super::COMMANDS.iter().map(|(n, _)| *n).collect();

        // When they are compared
        // Then they are the same words in the same order
        assert_eq!(routed, pipeline::ROUTER_WORDS);
    }
}
