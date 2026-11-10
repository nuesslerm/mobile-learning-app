//! The surface a caller meets first: version, help for every command, the program options wherever they stand, how a malformed or
//! retired command line is refused.
use crate::support::{repo_root, run_wf, wf_binary, RunOpts, RunResult};
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct HelpEntry {
    path: Vec<String>,
    bare: bool,
    text: String,
}

/// The crates linked into `wf`: itself and, transitively, every path dependency of a `[dependencies]` table (a dev-dependency is
/// not linked). A command served by a separate binary has its help in that binary, not in this table.
fn linked_crates() -> Vec<PathBuf> {
    let path = Regex::new(r#"\bpath\s*=\s*"([^"]+)""#).unwrap();
    let linked = |table: &str| table == "[dependencies]" || table.starts_with("[dependencies.") || (table.starts_with("[target.") && table.ends_with(".dependencies]"));
    let (mut crates, mut queue) = (Vec::new(), vec![repo_root().join("clis/wf")]);
    while let Some(dir) = queue.pop() {
        let dir = dir.canonicalize().unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        if crates.contains(&dir) {
            continue;
        }
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml");
        let mut table = "";
        for line in manifest.lines().map(str::trim) {
            if line.starts_with('[') {
                table = line;
            } else if linked(table) {
                queue.extend(path.captures(line).map(|c| dir.join(&c[1])));
            }
        }
        crates.push(dir);
    }
    crates
}

/// The rows of the `help_table.rs` of every crate linked into `wf`: each command's words, whether the bare group prints help, and
/// its committed text.
fn help_table() -> Vec<HelpEntry> {
    let row = Regex::new(r#"^ {4}\(("[^"]*"), (true|false), include_str!\("help/([^"]+)"\)\),$"#).unwrap();
    let source = |dir: &PathBuf| ["src", "src-rs"].into_iter().map(|d| dir.join(d)).find(|d| d.join("help_table.rs").is_file());
    let mut crates: Vec<PathBuf> = linked_crates().into_iter().filter(|dir| source(dir).is_some()).collect();
    crates.sort();
    crates
        .iter()
        .flat_map(|dir| {
            let src = source(dir).expect("help table");
            let table = std::fs::read_to_string(src.join("help_table.rs")).expect("help table");
            table
                .lines()
                .filter_map(|line| {
                    let captures = row.captures(line)?;
                    let path: String = serde_json::from_str(&captures[1]).expect("a JSON string");
                    Some(HelpEntry {
                        path: path.split(' ').filter(|part| !part.is_empty()).map(str::to_string).collect(),
                        bare: &captures[2] == "true",
                        text: std::fs::read_to_string(src.join("help").join(&captures[3])).expect("help text"),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `wf <args>` in `state`, with an `adb` there that lists no devices and no device named.
fn wf_in(state: &Path, args: &[&str]) -> RunResult {
    let adb = state.join("adb");
    if !adb.exists() {
        std::fs::write(&adb, "#!/bin/sh\necho 'List of devices attached'\n").unwrap();
        std::fs::set_permissions(&adb, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    run_wf(args, &RunOpts::default().cwd(state).bin_dir(state).env("XDG_STATE_HOME", &state.display().to_string()).env("ANDROID_SERIAL", "").env("WF_ANDROID_SERIAL", ""))
}

/// A help text without the device verbs `wf browser|android --help` generate from the command table after the committed text.
fn committed<'a>(entry: &HelpEntry, out: &'a str) -> &'a str {
    match entry.path.as_slice() {
        [noun] if noun == "browser" || noun == "android" => {
            let (text, verbs) = out.split_at(out.find("\nDevice commands").unwrap_or(out.len()));
            assert!(verbs.contains("  tap-on ") && verbs.contains("  extended-wait-until "), "wf {noun}: no generated device verbs in {out}");
            text
        }
        _ => out,
    }
}

/// An envelope with its duration zeroed, so two runs of one verb compare equal.
fn unmeasured(text: &str) -> String {
    Regex::new(r#""ms":\d+"#).unwrap().replace_all(text, r#""ms":0"#).into_owned()
}

fn untimed(text: &str) -> String {
    Regex::new(r"\bts=\d+").unwrap().replace_all(text, "ts=0").into_owned()
}

fn only_json_line(stdout: &str) -> Value {
    let lines: Vec<&str> = stdout.trim().lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "one JSON line expected: {stdout}");
    serde_json::from_str(lines[0]).unwrap_or_else(|e| panic!("{e}: {stdout}"))
}

#[test]
fn a_new_caller_learns_the_commands_from_the_version_and_the_help_of_every_command() {
    // Given the binary and the committed help table
    let table = help_table();
    assert!(table.len() > 50 && table.iter().any(|e| e.path.is_empty()) && table.iter().all(|e| e.text.starts_with("Usage: wf")));

    // When the caller asks for the version
    let version = run_wf(&["--version"], &RunOpts::default());
    // Then it is one semantic version line
    assert_eq!(version.exit_code, 0);
    assert!(Regex::new(r"^\d+\.\d+\.\d+\n$").unwrap().is_match(&version.stdout), "{:?}", version.stdout);

    // When the caller asks for the top-level help
    let top = run_wf(&["--help"], &RunOpts::default());
    // Then it names the program and lists only the verbs, harnesses and nouns
    assert_eq!(top.exit_code, 0);
    assert!(top.stdout.contains("Usage: wf") && top.stdout.contains("Dev workflow CLI"), "{}", top.stdout);
    let names: Vec<&str> = top.stdout.lines().skip_while(|l| *l != "Commands:").skip(1).filter_map(|l| l.strip_prefix("  ")).filter(|l| !l.starts_with(' ')).filter_map(|l| l.split_whitespace().next()).collect();
    assert_eq!(names, ["check", "ship", "offload", "flow", "test", "ci", "android", "browser", "vm", "secret", "agent-config", "bench", "knowledge", "cache", "lib", "t3", "exec", "mimic", "parity", "help"]);

    // When every command of the table is asked for its help
    for entry in &table {
        let mut args: Vec<&str> = entry.path.iter().map(String::as_str).collect();
        args.push("--help");
        let r = run_wf(&args, &RunOpts::default().clean_env());
        // Then the committed text comes out on stdout with exit 0 (the device nouns add their generated verb list)
        assert_eq!((r.exit_code, committed(entry, &r.stdout)), (0, entry.text.as_str()), "wf {}", entry.path.join(" "));
    }

    // When a bare command group runs without a sub-word
    for entry in table.iter().filter(|e| e.bare) {
        let args: Vec<&str> = entry.path.iter().map(String::as_str).collect();
        let r = run_wf(&args, &RunOpts::default().clean_env());
        // Then its help goes to stderr with exit 1
        assert_eq!((r.exit_code, committed(entry, &r.stderr)), (1, entry.text.as_str()), "wf {}", entry.path.join(" "));
    }
    let r = run_wf(&["secret"], &RunOpts::default());
    assert_eq!(r.exit_code, 1, "secret");
    assert!(r.stderr.starts_with("Usage: wf secret"), "secret: {}", r.stderr);

    // And the commands an agent reaches for most each print their own usage line
    for (args, usage) in [
        (vec!["check", "--help"], "Usage: wf check"),
        (vec!["ship", "eas", "--help"], "Usage: wf ship eas"),
        (vec!["offload", "ask", "--help"], "Usage: wf offload ask"),
        (vec!["offload", "plan", "--help"], "Usage: wf offload plan"),
        (vec!["vm", "creds", "sync", "--help"], "Usage: wf vm creds sync"),
        (vec!["vm", "net", "auth-key", "status", "--help"], "Usage: wf vm net auth-key status"),
        (vec!["vm", "net", "device", "list", "--help"], "Usage: wf vm net device list"),
        (vec!["vm", "ops", "health", "--help"], "Usage: wf vm ops health"),
        (vec!["secret", "login", "--help"], "Usage: wf secret login"),
        (vec!["bench", "cli", "--help"], "Usage: wf bench cli"),
        (vec!["agent-config", "sync", "--help"], "Usage: wf agent-config sync"),
        (vec!["knowledge", "rank", "--help"], "Usage: wf knowledge rank"),
        (vec!["offload", "ladder", "clear-cooldown", "--help"], "Usage: wf offload ladder clear-cooldown"),
    ] {
        let r = run_wf(&args, &RunOpts::default());
        assert_eq!(r.exit_code, 0, "{args:?}: {}{}", r.stdout, r.stderr);
        assert!(r.stdout.starts_with(usage), "{args:?}: {}", r.stdout);
    }

    // And the reference the owner reads has a section for every command
    let reference = std::fs::read_to_string(repo_root().join("docs/reference/wf-cli.md")).expect("docs/reference/wf-cli.md");
    let missing: Vec<String> = table
        .iter()
        .filter(|e| !e.path.is_empty())
        .map(|e| format!("`wf {}`", e.path.join(" ")))
        .filter(|heading| !reference.lines().any(|l| l.starts_with('#') && l.ends_with(heading.as_str())))
        .collect();
    assert!(missing.is_empty(), "commands without a section in docs/reference/wf-cli.md (regenerate it): {missing:?}");
}

/// The binaries CI installs beside `wf` (`bins=` in the install-wf action), `wf` itself left out.
fn installed_beside_wf() -> Vec<String> {
    let action = std::fs::read_to_string(repo_root().join(".github/actions/install-wf/action.yml")).expect("the install-wf action");
    let bins = Regex::new(r#"\bbins="([^"]+)""#).unwrap().captures(&action).expect("bins=\"...\" in the install-wf action")[1].to_string();
    bins.split_whitespace().filter(|b| *b != "wf").map(str::to_string).collect()
}

/// `dir/<name>`, standing in for a separate binary: one JSON line with its name, where it was put, its pid and its argv; exit 7.
fn stub_binary(dir: &Path, name: &str, at: &str) {
    let script = r#"#!/bin/sh
printf '{"binary":"NAME","at":"AT","pid":%s,"argv":[' $$
sep=
for a in "$@"; do printf '%s"%s"' "$sep" "$a"; sep=,; done
printf ']}\n'
exit 7
"#;
    let file = dir.join(name);
    std::fs::write(&file, script.replace("NAME", name).replace("AT", at)).unwrap();
    std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// The binary under test alone in a fresh directory (linked, else copied), since `wf` looks for a separate binary beside its own path.
fn lone_wf() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new().prefix("wf-flow-lone-").tempdir().unwrap();
    let (built, wf) = (wf_binary().canonicalize().unwrap(), dir.path().join("wf"));
    if std::fs::hard_link(&built, &wf).is_err() {
        std::fs::copy(&built, &wf).unwrap();
    }
    (dir, wf)
}

/// `wf` run from its own directory with `path` as its whole PATH: exit code, stdout, and the pid it started with.
fn run_lone(wf: &Path, path: &Path, args: &[&str]) -> (i32, String, u32) {
    let home = wf.parent().unwrap();
    let mut command = Command::new(wf);
    command.args(args).env_clear().env("PATH", path).env("HOME", home).env("WF_USAGE", "off").current_dir(home).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command.spawn().unwrap_or_else(|e| panic!("cannot start {}: {e}", wf.display()));
    let pid = child.id();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), pid)
}

#[test]
fn a_command_in_its_own_binary_runs_in_place_of_wf_from_beside_it_else_from_path() {
    // Given the commands `wf --help` lists that no crate linked into wf serves, and the binaries CI installs beside wf
    let served: Vec<String> = help_table().into_iter().filter_map(|e| e.path.first().cloned()).collect();
    let top = run_wf(&["--help"], &RunOpts::default());
    let external: Vec<String> = top.stdout.lines().skip_while(|l| *l != "Commands:").skip(1).filter_map(|l| l.strip_prefix("  ")).filter(|l| !l.starts_with(' ')).filter_map(|l| l.split_whitespace().next()).filter(|name| *name != "help" && !served.iter().any(|s| s == name)).map(str::to_string).collect();
    let binaries = installed_beside_wf();
    assert!(!external.is_empty(), "{}", top.stdout);
    // and a wf with a stand-in for each binary beside it, another set on its PATH, and a wf with nothing beside it
    let (beside, wf) = lone_wf();
    let (on_path, empty, (_alone, alone)) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), lone_wf());
    for binary in &binaries {
        stub_binary(beside.path(), binary, "beside");
        stub_binary(on_path.path(), binary, "path");
    }

    let mut owners: Vec<String> = Vec::new();
    for command in &external {
        let c = command.as_str();
        let mut answered_by: Vec<String> = Vec::new();
        for (args, argv) in [(vec![c, "serve", "--port", "1"], vec![c, "serve", "--port", "1"]), (vec!["--json", c, "pair"], vec![c, "pair", "--json"]), (vec![c, "--help"], vec![c, "--help"]), (vec!["help", c], vec!["help", c])] {
            for (wf, at) in [(&wf, "beside"), (&alone, "path")] {
                // When the command runs
                let (code, stdout, pid) = run_lone(wf, on_path.path(), &args);
                // Then its binary answers in wf's own process, beside wf before PATH, with the command line and its exit code
                let answer = only_json_line(&stdout);
                assert_eq!((code, answer["at"].as_str(), answer["pid"].as_u64(), answer["argv"].clone()), (7, Some(at), Some(u64::from(pid)), json!(argv)), "wf {args:?}: {stdout}");
                answered_by.push(answer["binary"].as_str().unwrap_or_default().to_string());
            }
        }
        answered_by.dedup();
        assert_eq!(answered_by.len(), 1, "wf {c}: one binary answers every shape: {answered_by:?}");

        // When the binary is neither beside wf nor on its PATH
        let (code, stdout, _) = run_lone(&alone, empty.path(), &[c, "serve"]);
        // Then wf names that binary and how to get it, as one JSON line, with exit 1
        let missing = only_json_line(&stdout);
        assert!(code == 1 && missing["ok"] == false && missing["binary"] == answered_by[0].as_str(), "wf {c} serve: {stdout}");
        assert!(missing["error"].as_str().is_some_and(|e| e.contains(&answered_by[0])) && missing["next"].as_str().is_some_and(|n| n.contains(&answered_by[0])), "{missing}");
        owners.extend(answered_by);
    }
    // and each command has a binary of its own, every one of them installed beside wf
    owners.sort();
    let mut installed = binaries;
    installed.sort();
    assert_eq!(owners, installed);
}

#[test]
fn a_caller_with_a_retired_or_malformed_command_line_is_refused_with_the_reason() {
    // Given an empty working directory
    let dir = tempfile::tempdir().unwrap();
    let wf = |args: &[&str]| run_wf(args, &RunOpts::default().cwd(dir.path()));

    // When `repo`, `mcp-guard`, `ask`, `plan` or `worker` runs, Then each prints the spelling that replaced it and exits 2
    for (argv, next) in [(vec!["repo", "sync", "--check"], "wf agent-config sync --check"), (vec!["repo", "bench", "cli"], "wf bench cli"), (vec!["repo", "knowledge", "rank", "x"], "wf knowledge rank x"), (vec!["repo", "ladder", "clear-cooldown", "p"], "wf offload ladder clear-cooldown p"), (vec!["mcp-guard"], "wf agent-config mcp-guard"), (vec!["ask", "--stats"], "wf offload ask --stats"), (vec!["plan", "x"], "wf offload plan x"), (vec!["worker", "check", "--plan", "x"], "wf offload worker check --plan x"), (vec!["worker", "visual", "--files", "a"], "wf check --visual --files a")] {
        let r = wf(&argv);
        assert_eq!(r.exit_code, 2, "{argv:?}: {}{}", r.stdout, r.stderr);
        assert!(r.stdout.contains(&format!("\"next\":\"{next}\"")), "{argv:?}: {}", r.stdout);
    }

    // When an earlier top-level name or a verb-first alias runs
    for old in ["verify", "deploy", "work", "status", "dispatch", "agents", "secrets", "auth", "vm-ops", "tailscale", "report-health", "cli-bench", "cm6", "visual", "metro", "ios", "app"] {
        let r = wf(&[old, "--help"]);
        // Then it is an unknown command with exit 1
        assert_eq!(r.exit_code, 1, "{old}: {}{}", r.stdout, r.stderr);
        assert!(r.stderr.contains(&format!("error: unknown command '{old}'")), "{old}: {}", r.stderr);
    }
    for alias in [["prepare", "ios"], ["start", "ios"]] {
        assert_eq!(wf(&alias).exit_code, 1, "{alias:?}");
    }

    // When the earlier global --app, the admin flags that left ask, or a positional vm logs no longer takes are given
    let app = wf(&["offload", "plan", "--check", "x", "--app", "mla-tamagui"]);
    // Then clap refuses each with exit 2
    assert_eq!(app.exit_code, 2);
    assert!(app.stderr.contains("unexpected argument '--app'"), "{}", app.stderr);
    for flag in ["--clear-cooldown", "--pricing-sync"] {
        assert_eq!(wf(&["offload", "ask", flag]).exit_code, 2, "{flag}");
    }
    assert_eq!(wf(&["vm", "logs", "remote-chat-agent"]).exit_code, 2);

    // When options are wrong in each way clap knows
    let bogus = wf(&["vm", "net", "device", "list", "--bogus"]);
    let extra = wf(&["vm", "net", "device", "list", "extra"]);
    let no_value = wf(&["check", "--visual", "--files"]);
    let missing = wf(&["log", "emit", "--service", "x"]);
    let flag_value = wf(&["check", "--budget=1"]);
    // Then each names the problem in clap's words, with the usage, and exits 2
    assert_eq!((bogus.exit_code, extra.exit_code, no_value.exit_code, missing.exit_code, flag_value.exit_code), (2, 2, 2, 2, 2));
    assert!(bogus.stderr.contains("error: unexpected argument '--bogus' found") && bogus.stderr.contains("Usage: wf vm net device list"), "{}", bogus.stderr);
    assert!(extra.stderr.contains("error: unexpected argument 'extra' found"), "{}", extra.stderr);
    assert!(no_value.stderr.contains("error: a value is required for '--files <files>' but none was supplied"), "{}", no_value.stderr);
    assert!(missing.stderr.contains("error: the following required arguments were not provided:") && missing.stderr.contains("--message <message>"), "{}", missing.stderr);
    assert!(flag_value.stderr.contains("error: unexpected value '1' for '--budget' found; no more were expected"), "{}", flag_value.stderr);
    for (argv, flag) in [(vec!["offload", "ask", "--stats=1"], "--stats"), (vec!["check", "--health", "--list-unit-targets=1"], "--list-unit-targets")] {
        let r = wf(&argv);
        assert_eq!(r.exit_code, 2, "{flag}");
        assert!(r.stderr.contains(&format!("error: unexpected value '1' for '{flag}' found; no more were expected")), "{flag}: {}", r.stderr);
    }
    for argv in [vec!["offload", "ask", "--stats", "--bogus"], vec!["check", "--budget", "--bogus"], vec!["check", "--health", "--list-unit-targets", "--bogus"]] {
        let r = wf(&argv);
        assert_eq!(r.exit_code, 2, "{argv:?}");
        assert!(r.stderr.contains("error: unexpected argument '--bogus' found"), "{argv:?}: {}", r.stderr);
    }

    // When a program option is broken before any command
    let dangling = wf(&["--frontend"]);
    let unknown = wf(&["--bogus", "android", "adb", "devices"]);
    // Then clap refuses it too
    assert_eq!((dangling.exit_code, unknown.exit_code), (2, 2));
    assert!(dangling.stderr.contains("a value is required for"), "{}", dangling.stderr);
    assert!(unknown.stderr.contains("error: unexpected argument '--bogus' found"), "{}", unknown.stderr);
    // and a device noun's own verb answers the same mistakes as one JSON line carrying its usage
    let own = only_json_line(&wf(&["android", "lease", "status", "--bogus"]).stdout);
    let surplus = wf(&["android", "lease", "status", "extra"]);
    assert!(own["error"] == "unexpected argument '--bogus' found" && own["usage"].as_str().unwrap().starts_with("wf android lease"), "{own}");
    assert!(surplus.exit_code == 2 && only_json_line(&surplus.stdout)["error"].as_str().unwrap().contains("unexpected value 'extra'"), "{}", surplus.stdout);

    // When the words name no command shape at all
    let shape = wf(&["log", "nothing"]);
    // Then wf says so in its own words
    assert_eq!(shape.exit_code, 2);
    assert!(shape.stderr.contains("the Rust wf does not support these arguments"), "{}", shape.stderr);
}

#[test]
fn the_program_options_mean_the_same_wherever_the_caller_puts_them() {
    // Given an empty state directory (no registry, no slots)
    let state = tempfile::Builder::new().prefix("wf-flow-surface-").tempdir().unwrap();
    let s = state.path();

    // When a device verb runs plainly and with --json, on a device whose screen adb reports
    std::fs::write(s.join("adb"), "#!/bin/sh\necho 'Physical size: 480x1040'\n").unwrap();
    std::fs::set_permissions(s.join("adb"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let plain = wf_in(s, &["android", "get-device-state", "screen", "--device", "e1"]);
    let json = wf_in(s, &["android", "get-device-state", "screen", "--device", "e1", "--json"]);
    // Then both are the same one clean JSON line, the envelope with the result, and no logfmt mixed in
    assert_eq!((plain.exit_code, json.exit_code), (0, 0), "{}{}", json.stdout, json.stderr);
    assert_eq!(unmeasured(&plain.stdout), unmeasured(&json.stdout));
    let answer = only_json_line(&json.stdout);
    assert_eq!((answer["ok"].clone(), answer["device"].clone(), answer["result"].clone()), (json!(true), json!("e1"), json!({ "width": 480, "height": 1040, "dpr": 1 })), "{answer}");
    assert!(!json.stdout.contains("level=info"));

    // When a failing device verb runs with and without --json (adb lists no device)
    let failed_json = wf_in(s, &["android", "get-device-state", "--json"]);
    let failed_plain = wf_in(s, &["android", "get-device-state"]);
    // Then both are one line {ok:false, error, next} on stdout, never a `wf:` message
    assert_eq!((failed_json.exit_code, failed_plain.exit_code), (1, 1), "{}", failed_json.stdout);
    let failure = only_json_line(&failed_json.stdout);
    assert_eq!(failure["ok"], false);
    assert!(!failure["error"].as_str().unwrap().is_empty() && failure["next"].is_string() && !failed_json.stdout.contains("wf: "), "{failure}");
    assert_eq!(unmeasured(&failed_plain.stdout), unmeasured(&failed_json.stdout));

    // When --json stands before the command, between it and its sub-word, or after it
    for command in [["android", "get-device-state"], ["repo", "ladder"]] {
        let after = wf_in(s, &[command[0], command[1], "--json"]);
        let before = wf_in(s, &["--json", command[0], command[1]]);
        let between = wf_in(s, &[command[0], "--json", command[1]]);
        // Then the answer is the same
        assert_eq!((before.exit_code, unmeasured(&before.stdout)), (after.exit_code, unmeasured(&after.stdout)), "{command:?}: {}", before.stderr);
        assert_eq!((between.exit_code, unmeasured(&between.stdout)), (after.exit_code, unmeasured(&after.stdout)), "{command:?}: {}", between.stderr);
    }
    // and the shape the browser SDK sends reaches the browser command
    let sdk = wf_in(s, &["browser", "--json", "status"]);
    assert!(!sdk.stderr.contains("does not support these arguments") && !sdk.stderr.contains("unknown command"), "{}", sdk.stderr);

    // When --frontend is given in any position and either spelling, even with an app wf does not know
    for args in [
        vec!["--frontend", "mla-tamagui", "android", "adb", "devices"],
        vec!["android", "--frontend", "mla-tamagui", "adb", "devices"],
        vec!["android", "--frontend=mla-tamagui", "adb", "devices"],
        vec!["--frontend=mla-tamagui", "android", "adb", "devices"],
        vec!["android", "adb", "devices", "--frontend", "mla-tamagui"],
        vec!["--frontend", "other", "android", "adb", "devices"],
    ] {
        let r = wf_in(s, &args);
        // Then the command runs
        assert_eq!(r.exit_code, 0, "{args:?}: {}", r.stderr);
        assert!(!r.stderr.contains("unknown command"), "{args:?}");
    }
    for command in [vec!["android", "adb", "devices"], vec!["test", "plan"], vec!["check", "--health", "--list-unit-targets"]] {
        let expected = wf_in(s, &command);
        for args in [
            [vec!["--frontend", "mla-tamagui"], command.clone()].concat(),
            [vec!["--frontend=mla-tamagui"], command.clone()].concat(),
            [command.clone(), vec!["--frontend", "mla-tamagui"]].concat(),
            [command.clone(), vec!["--frontend=mla-tamagui"]].concat(),
        ] {
            let actual = wf_in(s, &args);
            // and answers exactly as without it
            assert_eq!(actual.exit_code, 0, "{args:?}: {}", actual.stderr);
            assert_eq!(untimed(&actual.stdout), untimed(&expected.stdout), "{args:?}");
        }
    }
    // and inside a command's own options it is read and dropped
    let own = wf_in(s, &["check", "--health", "--frontend", "other", "--list-unit-targets"]);
    assert_eq!((own.exit_code, untimed(&own.stdout)), (0, untimed(&wf_in(s, &["check", "--health", "--list-unit-targets"]).stdout)), "{}", own.stderr);
    for args in [vec!["--frontend", "mla-tamagui", "offload", "plan", "--check", "no-such-slug"], vec!["offload", "plan", "--frontend", "mla-tamagui", "--check", "no-such-slug"], vec!["offload", "plan", "--check", "no-such-slug", "--frontend=mla-tamagui"]] {
        let r = run_wf(&args, &RunOpts::default().cwd(&repo_root()));
        assert_eq!(r.exit_code, 2, "{args:?}");
        assert!(format!("{}{}", r.stdout, r.stderr).contains("no units file:"), "{args:?}: {}{}", r.stdout, r.stderr);
    }
}
