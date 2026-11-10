//! F1: the smoke-test, swap and app-redeploy scripts the service renders, run as their siblings run them (`World::run_script`) in the case's world, against `docker` and `wf` doubles.
//! Every failure branch ends with whole compose files, no staging dir, marker or temp file, and the outcome recorded and emitted with the OTLP host and token.
use crate::redeploy::{APP_SIBLING, HISTORY_APPEND};
use crate::self_redeploy::{swap_world, wait_swap, BUILT_COMMIT, BUILT_TAG, RUNNING_TAG};
use crate::{expect_json, post, world};
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use vm_ops_api_integration::{case, GithubRaw, ScriptRun, Service, ToolRule};

const WITHIN: Duration = Duration::from_secs(60);
const OTLP: &str = "otlp-tok";
const OTLP_HOST: &str = "otlp.example";
const OLD_COMPOSE: &str = "# old compose\nservices: {}\n";
const OLD_DEFAULTS: &str = "OLD_DEFAULT=1\n";
const OLD_ENV: &str = "OLD_ENV=1\nIMAGE_TAG=sha-0000000\nHOST_LABEL=main\n";
const LOCAL: &str = "BWS_ACCESS_TOKEN=bws-tok\nLOCAL_ONLY=1\n";
const NEW_COMPOSE: &str = "# new compose\nservices: {}\n";
const NEW_DEFAULTS: &str = "NEW_DEFAULT=1\n";
/// The files of the compose dir, in the order the swap replaces them.
const FILES: [(&str, &str); 3] = [(".env.defaults", OLD_DEFAULTS), (".env", OLD_ENV), ("docker-compose.yml", OLD_COMPOSE)];

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// The service after its self-build ended and it swapped itself in, with the swap sibling as it recorded it.
fn swapped() -> (Service, Vec<String>) {
    let mut w = swap_world();
    w.set("BETTERSTACK_OTLP_TOKEN", OTLP).set("BETTERSTACK_OTLP_HOST", OTLP_HOST);
    let svc = w.start();
    expect_json(&post(&svc, &format!("/redeploy/smoke?ref={BUILT_COMMIT}&tag={BUILT_TAG}"), b""), 200, &json!({ "smoke": "up", "port": 18999, "ref": BUILT_COMMIT, "tag": BUILT_TAG }));
    expect_json(&post(&svc, &format!("/redeploy?ref={BUILT_COMMIT}&tag={BUILT_TAG}"), b""), 202, &json!({ "started": true, "swapping": BUILT_COMMIT, "tag": BUILT_TAG }));
    let swap = wait_swap(&svc, Duration::from_secs(30));
    (svc, swap)
}

/// The path of each file a script's provision line fetches, as the raw double serves it.
fn provisioned(script: &str) -> Vec<String> {
    let url = Regex::new(r"'https://raw\.githubusercontent\.com(/[^']+)'").expect("a regex");
    url.captures_iter(script).map(|c| c[1].to_string()).collect()
}

/// A raw double serving the new compose file and the given .env.defaults of a script's provision line, but for the one `withheld`.
fn raw_serving(script: &str, withheld: Option<&str>, defaults: &str) -> GithubRaw {
    let served: Vec<(String, &str)> = provisioned(script)
        .into_iter()
        .filter(|p| withheld.is_none_or(|w| !p.ends_with(w)))
        .map(|p| if p.ends_with(".env.defaults") { (p, defaults) } else { (p, NEW_COMPOSE) })
        .collect();
    GithubRaw::serve(&served.iter().map(|(p, text)| (p.as_str(), text.as_bytes())).collect::<Vec<_>>())
}

fn raw_for(script: &str, withheld: Option<&str>) -> GithubRaw {
    raw_serving(script, withheld, NEW_DEFAULTS)
}

fn compose(sub: &[&str]) -> ToolRule {
    ToolRule::docker(&[&["compose"], sub].concat())
}

fn pass(rule: ToolRule) -> ToolRule {
    rule.answer("", "", 0)
}

fn fail(rule: ToolRule, code: i32) -> ToolRule {
    rule.answer("", "boom\n", code)
}

/// The compose dir as the old stack left it: the three files, the `.env.local` and nothing else.
fn seed(vm: &Path, local: &str) {
    let _ = std::fs::remove_dir_all(vm);
    std::fs::create_dir_all(vm).expect("the compose dir");
    for (file, text) in FILES {
        std::fs::write(vm.join(file), text).expect("an old file");
    }
    std::fs::write(vm.join(".env.local"), local).expect("a .env.local");
}

/// A temp file name that `cp` cannot write: a symlink to a directory that is not there, which the script's cleanup removes.
fn block_temp(vm: &Path, name: &str) {
    std::os::unix::fs::symlink(vm.join("no-such-dir/x"), vm.join(name)).expect("a dangling symlink");
}

/// The three files as they are now, in the order of [`FILES`].
fn files(vm: &Path) -> [Option<String>; 3] {
    FILES.map(|(file, _)| read(&vm.join(file)))
}

fn old_files() -> [Option<String>; 3] {
    FILES.map(|(_, text)| Some(text.to_string()))
}

fn prev_files(vm: &Path) -> [Option<String>; 3] {
    FILES.map(|(f, _)| read(&vm.join(format!("{f}.prev"))))
}

fn no_prev(vm: &Path) -> bool {
    !FILES.iter().any(|(f, _)| exists(&vm.join(format!("{f}.prev"))))
}

/// The call's snapshot of `path`, when it was read.
fn seen<'a>(call: &'a Value, path: &Path) -> Option<&'a str> {
    call["seen"][path.display().to_string()].as_str()
}

/// One `wf log emit` of the script: its level and message, the OTLP host and token the call was handed, its whole argv.
struct Emit {
    level: String,
    message: String,
    host: Option<String>,
    token: Option<String>,
    argv: String,
}

fn emits(run: &ScriptRun) -> Vec<Emit> {
    let value = |argv: &[&str], flag: &str| argv.iter().position(|a| *a == flag).and_then(|i| argv.get(i + 1)).map(|s| s.to_string());
    let rows = run.calls_of("wf", &["log", "emit"]).into_iter().map(|c| {
        let argv: Vec<&str> = c["argv"].as_array().expect("an argv").iter().filter_map(Value::as_str).collect();
        Emit {
            level: value(&argv, "--level").unwrap_or_default(),
            message: value(&argv, "--message").unwrap_or_default(),
            host: c["env"]["BETTERSTACK_OTLP_HOST"].as_str().map(String::from),
            token: c["env"]["BETTERSTACK_OTLP_TOKEN"].as_str().map(String::from),
            argv: c["argv"].to_string(),
        }
    });
    rows.collect()
}

/// The docker double's compose subcommands, in order.
fn compose_calls(run: &ScriptRun) -> Vec<String> {
    run.calls_of("docker", &["compose"]).iter().map(|c| c["argv"][1].as_str().unwrap_or("").to_string()).collect()
}

/// The last line of the redeploy history the script appended.
fn history(vm: &Path) -> Value {
    let text = read(&vm.join("redeploy-history.jsonl")).unwrap_or_default();
    serde_json::from_str(text.lines().last().expect("a history line")).expect("a JSON history line")
}

/// What every ended swap leaves: no staging dir, marker or temp file, the history line and the outcome event of `message`
/// with the exit code and whether the old stack was `restored` (`none`, `ok` or `failed`), the event handed the OTLP host
/// and token the env sync unset.
fn assert_recorded(run: &ScriptRun, vm: &Path, code: i32, message: &str, restored: &str) {
    assert_eq!(run.exit.code, Some(code), "stderr: {}\nlog: {:?}", run.exit.stderr, read(&vm.join("last-redeploy.log")));
    let entries: Vec<String> = std::fs::read_dir(vm).expect("the compose dir").filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert!(!entries.iter().any(|f| f == ".staging" || f == ".swap-pending" || f.ends_with(".tmp")), "left behind: {entries:?}");
    let line = history(vm);
    assert_eq!((&line["outcome"], &line["exitCode"], &line["tag"], &line["restored"]), (&json!(message), &json!(code), &json!(BUILT_TAG), &json!(restored)), "{line}");
    let outcome = emits(run).pop().expect("an outcome event");
    let level = if message == "redeploy.succeeded" { "info" } else { "error" };
    assert_eq!((outcome.level.as_str(), outcome.message.as_str()), (level, message));
    assert_eq!((outcome.host.as_deref(), outcome.token.as_deref()), (Some(OTLP_HOST), Some(OTLP)), "{}", outcome.argv);
    assert!(outcome.argv.contains(&format!("exitCode={code}")) && outcome.argv.contains(&format!("restored={restored}")), "{}", outcome.argv);
}

/// Runs the swap in the compose dir as it is, against the rules, the raw double serving all but `withheld`. `also_see` names
/// more files each recorded call snapshots, beside the compose files, the staged ones and the marker.
fn run_swap_on(svc: &Service, swap: &[String], rules: &[ToolRule], withheld: Option<&str>, also_see: &[PathBuf]) -> ScriptRun {
    let (vm, raw) = (svc.world().dir().join("vm"), raw_for(&swap[swap.len() - 1], withheld));
    let stage = vm.join(".staging");
    let mut see: Vec<PathBuf> = [&vm, &stage].iter().flat_map(|d| FILES.map(|(f, _)| d.join(f))).collect();
    see.push(vm.join(".swap-pending"));
    see.extend(also_see.iter().cloned());
    svc.world().run_script(swap, &raw, rules, &see, WITHIN)
}

/// Runs the swap in a fresh compose dir with this `.env.local`.
fn run_swap(svc: &Service, swap: &[String], rules: &[ToolRule], withheld: Option<&str>, local: &str) -> ScriptRun {
    seed(&svc.world().dir().join("vm"), local);
    run_swap_on(svc, swap, rules, withheld, &[])
}

fn healthy() -> Vec<ToolRule> {
    vec![pass(compose(&["config"])), pass(compose(&["pull"])), pass(compose(&["up"]))]
}

fn up(n: u64) -> ToolRule {
    compose(&["up"]).nth(n)
}

/// The smoke-test sibling the service recorded for `POST /redeploy/smoke`, run in a fresh compose dir against docker answering `rules`.
fn run_smoke(svc: &Service, rules: &[ToolRule]) -> ScriptRun {
    let smoke = svc.world().docker_calls().into_iter().next().expect("the smoke test ran first");
    let vm = svc.world().dir().join("vm");
    seed(&vm, LOCAL);
    let raw = raw_for(&smoke[smoke.len() - 1], None);
    let stage = vm.join(".smoke-staging");
    let see: Vec<PathBuf> = FILES.iter().map(|(f, _)| vm.join(f)).chain([stage.join(".env"), stage.join("docker-compose.yml")]).collect();
    svc.world().run_script(&smoke, &raw, rules, &see, WITHIN)
}

/// docker answering for a throwaway container that runs and passes its probe.
fn smoke_container() -> Vec<ToolRule> {
    vec![ToolRule::docker(&["rm"]).answer("", "", 0), ToolRule::docker(&["inspect"]).answer("true\n", "", 0), ToolRule::docker(&["exec"]).answer("", "", 0)]
}

/// docker answering a smoke test that passes: the staged compose is valid and pulls.
fn smoke_passes() -> Vec<ToolRule> {
    [vec![pass(compose(&["config"])), pass(compose(&["pull"])), pass(compose(&["run"]))], smoke_container()].concat()
}

fn untouched(svc: &Service) -> bool {
    let vm = svc.world().dir().join("vm");
    files(&vm) == old_files() && no_prev(&vm) && !exists(&vm.join(".smoke-staging")) && !exists(&vm.join(".swap-pending"))
}

case!(the_smoke_test_runs_the_pulled_image_beside_the_live_one_on_the_spare_port_and_swaps_nothing, ["POST /redeploy"], {
    // Given the smoke test the service recorded, and docker answering that the throwaway container passes its probe
    let (svc, _) = swapped();
    let run = run_smoke(&svc, &smoke_passes());
    // Then it exits 0 having checked and pulled the staged compose, started the throwaway container on the spare port and
    // probed it from inside, never bringing the stack up
    assert_eq!(run.exit.code, Some(0), "{}", run.exit.stderr);
    assert_eq!(compose_calls(&run), ["config", "pull", "run"]);
    let started = run.calls_of("docker", &["compose", "run"]);
    assert_eq!(started[0]["argv"].to_string(), r#"["compose","run","-d","--no-deps","--name","vm-ops-api-smoke","-e","PORT=18999","-e","CI_PORT=18998","ops-api"]"#);
    assert_eq!(run.calls_of("docker", &["exec"]).len(), 1, "the probe");
    assert!(run.exit.stdout.lines().any(|l| l == "[smoke] vm-ops-api healthy on port 18999"), "{}", run.exit.stdout);
    // And the throwaway container is left running for the tailnet probe, and the compose dir is as it was
    assert_eq!(run.calls_of("docker", &["rm", "-f", "vm-ops-api-smoke"]).len(), 1, "cleared before, left running after");
    assert!(untouched(&svc));
});

case!(a_smoke_container_that_dies_fails_the_script_with_its_log_and_swaps_nothing, ["POST /redeploy"], {
    // Given a throwaway container that is not running when it is asked after, and has a log
    let (svc, _) = swapped();
    let mut rules = vec![ToolRule::docker(&["inspect"]).answer("false\n", "", 0), ToolRule::docker(&["logs"]).answer("thread 'main' panicked: no such file\n", "", 0)];
    rules.extend(smoke_passes());
    // When the smoke test runs (the first rule that matches wins)
    let run = run_smoke(&svc, &rules);
    // Then it exits 1 with the container's log on stderr, removes the container, and the compose dir is as it was
    assert_eq!(run.exit.code, Some(1), "{}", run.exit.stderr);
    assert!(run.exit.stderr.contains("did not answer on port 18999; nothing was swapped") && run.exit.stderr.contains("panicked: no such file"), "{}", run.exit.stderr);
    assert!(run.calls_of("docker", &["compose", "up"]).is_empty());
    assert!(untouched(&svc));
});

case!(the_self_swap_stages_checks_pulls_swaps_and_pins_the_tag_only_after_up_exits_0, ["POST /redeploy"], {
    // Given the swap sibling the service recorded, an old stack in the compose dir with a .env.local, and docker answering
    // 0 to the config check, the pull and the up
    let (svc, swap) = swapped();
    let run = run_swap(&svc, &swap, &healthy(), None, LOCAL);
    let vm = svc.world().dir().join("vm");
    let stage = vm.join(".staging");
    // Then the script exits 0 and compose was asked, in order, to check, pull and up, the up waiting for the new stack to
    // be healthy
    assert_eq!(compose_calls(&run), ["config", "pull", "up"], "{}", run.exit.stderr);
    let calls = run.calls_of("docker", &["compose"]);
    assert_eq!(calls.iter().map(|c| c["argv"].to_string()).collect::<Vec<_>>(), [r#"["compose","config","-q"]"#, r#"["compose","pull","ops-api"]"#, r#"["compose","up","-d","--wait","--wait-timeout","120"]"#]);
    // And the check and the pull ran in the staging dir, against the new files, while the old ones were still in place
    let staged_env = seen(calls[0], &stage.join(".env")).expect("the staged .env").to_string();
    for c in &calls[..2] {
        assert_eq!(c["cwd"], json!(stage.display().to_string()));
        assert_eq!(seen(c, &stage.join("docker-compose.yml")), Some(NEW_COMPOSE));
        assert_eq!([seen(c, &vm.join("docker-compose.yml")), seen(c, &vm.join(".env"))], [Some(OLD_COMPOSE), Some(OLD_ENV)]);
    }
    // and the staged .env is the new defaults and the vault's secrets, HOST_LABEL and the .env.local's lines
    for line in [NEW_DEFAULTS.trim(), "VM_OPS_API_AUTH_TOKEN=vault-token", "HOST_LABEL=main", "LOCAL_ONLY=1"] {
        assert!(staged_env.contains(line), "{line} not in {staged_env}");
    }
    // And up ran in the compose dir with the new files swapped in, the swap marker gone and no pin yet, with the new tag
    // and without the OTLP token, which the env sync unsets so compose reads the new .env
    let up = calls[2];
    assert_eq!(up["cwd"], json!(vm.display().to_string()));
    assert_eq!([seen(up, &vm.join("docker-compose.yml")), seen(up, &vm.join(".env"))], [Some(NEW_COMPOSE), Some(staged_env.as_str())]);
    assert_eq!(seen(up, &vm.join(".swap-pending")), None);
    assert_eq!((up["env"]["IMAGE_TAG"].as_str(), up["env"].get("BETTERSTACK_OTLP_TOKEN")), (Some(BUILT_TAG), None));
    // And afterwards the files are the new ones, the .env pinned to the tag, the old ones kept as .prev, whole
    assert_eq!(files(&vm), [Some(NEW_DEFAULTS.into()), Some(format!("{staged_env}IMAGE_TAG={BUILT_TAG}\n")), Some(NEW_COMPOSE.into())]);
    assert_eq!(prev_files(&vm), old_files());
    // And the outcome is recorded and emitted with the OTLP host and token the sibling was given, as the start event was
    assert_recorded(&run, &vm, 0, "redeploy.succeeded", "none");
    let started = &emits(&run)[0];
    assert_eq!((started.message.as_str(), started.host.as_deref(), started.token.as_deref()), ("redeploy.started", Some(OTLP_HOST), Some(OTLP)));
    assert_eq!(history(&vm)["ref"], json!(BUILT_COMMIT));
});

case!(a_self_swap_that_fails_before_the_swap_touches_no_file_and_records_the_outcome, ["POST /redeploy"], {
    let (svc, swap) = swapped();
    let vault = ToolRule::new("wf", &["secret", "sync-env"]);
    let mangled = format!("{LOCAL}TS_VM_AUTH_KEY=\"\\\"vault-key\\\"\"\n");
    // a BWS line the shell cannot parse: sourced as it is, it would end the script before it recorded anything
    let unparsable = format!("{LOCAL}BWS_ACCESS_TOKEN=abc(def\n");
    // Given each step before the swap failing in turn, with its own exit code
    let cases: [(&str, Vec<ToolRule>, Option<&str>, &str, i32, Vec<&str>); 6] = [
        ("provisioning cannot get the compose file", healthy(), Some("docker-compose.yml"), LOCAL, 1, vec![]),
        ("the .env.local has a bootstrap line that does not parse", healthy(), None, unparsable.as_str(), 1, vec![]),
        ("the env sync fails", [vec![vault.answer("", "vault unreachable\n", 7)], healthy()].concat(), None, LOCAL, 7, vec![]),
        ("the .env.local mangles a resolved secret", healthy(), None, mangled.as_str(), 1, vec![]),
        ("the compose check fails", [vec![fail(compose(&["config"]), 3)], healthy()].concat(), None, LOCAL, 3, vec!["config"]),
        ("the pull fails", [vec![fail(compose(&["pull"]), 4)], healthy()].concat(), None, LOCAL, 4, vec!["config", "pull"]),
    ];
    for (what, rules, withheld, local, code, compose_ran) in cases {
        // When the swap runs
        let run = run_swap(&svc, &swap, &rules, withheld, local);
        let vm = svc.world().dir().join("vm");
        // Then it stops with that command's exit code, before any later compose call, and no compose file changed or was
        // kept as .prev
        assert_eq!(compose_calls(&run), compose_ran, "{what}");
        assert_eq!(files(&vm), old_files(), "{what}");
        assert!(no_prev(&vm), "{what}");
        // And the outcome is still recorded and emitted, with nothing restored because nothing was swapped
        assert_recorded(&run, &vm, code, "redeploy.failed", "none");
    }
});

case!(a_failed_up_puts_the_previous_files_back_and_brings_the_old_stack_up_again, ["POST /redeploy"], {
    let (svc, swap) = swapped();
    let vm = svc.world().dir().join("vm");
    // Given the first up failing and the second passing
    let rules = [vec![fail(up(1), 5)], healthy()].concat();
    // When the swap runs
    let run = run_swap(&svc, &swap, &rules, None, LOCAL);
    // Then compose was asked to check, pull and up twice, and the script exits with the first up's code
    assert_eq!(compose_calls(&run), ["config", "pull", "up", "up"], "{}", run.exit.stderr);
    let ups = run.calls_of("docker", &["compose", "up"]);
    let (first, second) = (ups[0], ups[1]);
    // and the first up waited on the new files with the new tag, the second is the restore: no wait, the old files, and
    // the tag that was running, which the old .env does not pin on its own
    assert_eq!([seen(first, &vm.join("docker-compose.yml")), seen(first, &vm.join(".env.defaults"))], [Some(NEW_COMPOSE), Some(NEW_DEFAULTS)]);
    assert_eq!(first["env"]["IMAGE_TAG"].as_str(), Some(BUILT_TAG));
    assert!(first["argv"].to_string().contains("--wait") && !second["argv"].to_string().contains("--wait"), "{first} {second}");
    assert_eq!([seen(second, &vm.join("docker-compose.yml")), seen(second, &vm.join(".env"))], [Some(OLD_COMPOSE), Some(OLD_ENV)]);
    assert_eq!(second["env"]["IMAGE_TAG"].as_str(), Some(RUNNING_TAG), "{}", second["env"]);
    // And the old files are back, whole, and nothing is pinned or left as .prev
    assert_eq!(files(&vm), old_files());
    assert!(no_prev(&vm));
    assert_recorded(&run, &vm, 5, "redeploy.failed", "ok");
    // Given both ups failing
    let rules = [vec![fail(up(1), 5), fail(up(2), 6)], healthy()].concat();
    let run = run_swap(&svc, &swap, &rules, None, LOCAL);
    // Then the old files are back all the same, and the script says the restore failed and exits with the first code
    assert_eq!(compose_calls(&run), ["config", "pull", "up", "up"]);
    assert_eq!(files(&vm), old_files());
    assert!(read(&vm.join("last-redeploy.log")).unwrap_or_default().contains("restore: docker compose up -d failed"));
    assert_recorded(&run, &vm, 5, "redeploy.failed", "failed");
    // Given a staged .env that is gone when the swap reaches it, after .env.defaults was swapped in
    let rules = [vec![pass(compose(&["pull"])).removes(".env")], healthy()].concat();
    let run = run_swap(&svc, &swap, &rules, None, LOCAL);
    // Then the half swap is undone: every file is the old one again, and the one up is the restore's
    assert_eq!(compose_calls(&run), ["config", "pull", "up"], "{}", run.exit.stderr);
    let restore = run.calls_of("docker", &["compose", "up"])[0];
    assert_eq!([seen(restore, &vm.join("docker-compose.yml")), seen(restore, &vm.join(".env"))], [Some(OLD_COMPOSE), Some(OLD_ENV)]);
    assert_eq!(files(&vm), old_files());
    assert_recorded(&run, &vm, 1, "redeploy.failed", "ok");
});

case!(a_pin_that_cannot_be_written_after_a_passing_up_is_a_failed_swap_that_is_rolled_back, ["POST /redeploy"], {
    // Given a swap whose up passes, and a .env.tmp that the pin's copy cannot write
    let (svc, swap) = swapped();
    let vm = svc.world().dir().join("vm");
    seed(&vm, LOCAL);
    block_temp(&vm, ".env.tmp");
    // When it runs
    let run = run_swap_on(&svc, &swap, &healthy(), None, &[]);
    // Then the up that passed is rolled back like a failed one: the old files, the restore's up at the running tag, and a
    // failed outcome with the copy's exit code
    assert_eq!(compose_calls(&run), ["config", "pull", "up", "up"], "{}", run.exit.stderr);
    assert_eq!(run.calls_of("docker", &["compose", "up"])[1]["env"]["IMAGE_TAG"].as_str(), Some(RUNNING_TAG));
    assert_eq!(files(&vm), old_files());
    assert!(no_prev(&vm));
    // and the blocked temp file was removed rather than left beside the secrets
    assert_recorded(&run, &vm, 1, "redeploy.failed", "ok");
});

case!(a_previous_copy_that_cannot_be_kept_stops_the_swap_before_it_touches_a_file, ["POST /redeploy"], {
    // Given a .prev.tmp the copy of the first file cannot write
    let (svc, swap) = swapped();
    let vm = svc.world().dir().join("vm");
    seed(&vm, LOCAL);
    block_temp(&vm, ".env.defaults.prev.tmp");
    // When the swap runs
    let run = run_swap_on(&svc, &swap, &healthy(), None, &[]);
    // Then it stops after the pull, with the copy's exit code, swapping nothing and calling no up, the old files whole
    assert_eq!(compose_calls(&run), ["config", "pull"], "{}", run.exit.stderr);
    assert_eq!(files(&vm), old_files());
    assert!(no_prev(&vm));
    // and the outcome is recorded with nothing restored
    assert_recorded(&run, &vm, 1, "redeploy.failed", "none");
});

case!(a_swap_that_was_killed_between_two_renames_is_undone_by_the_next_run, ["POST /redeploy"], {
    let (svc, swap) = swapped();
    let vm = svc.world().dir().join("vm");
    // Given the dir a swap left when it died after two renames: the new .env.defaults and .env, the old compose file, the
    // marker, and the three old files as .prev
    let kill = |vm: &Path| {
        seed(vm, LOCAL);
        std::fs::write(vm.join(".env.defaults"), NEW_DEFAULTS).expect("a swapped file");
        std::fs::write(vm.join(".env"), "NEW_ENV=1\n").expect("a swapped file");
        for (file, text) in FILES {
            std::fs::write(vm.join(format!("{file}.prev")), text).expect("a kept file");
        }
        std::fs::write(vm.join(".swap-pending"), "").expect("the marker");
    };
    kill(&vm);
    // When the next swap fails at provisioning
    let run = run_swap_on(&svc, &swap, &healthy(), Some("docker-compose.yml"), &[]);
    // Then it first put the old files back, and it leaves them: the dir is whole again
    assert_eq!(files(&vm), old_files(), "{}", run.exit.stderr);
    assert!(no_prev(&vm));
    assert_recorded(&run, &vm, 1, "redeploy.failed", "none");
    // Given the same dir, When the next swap passes
    kill(&vm);
    let run = run_swap_on(&svc, &swap, &healthy(), None, &[]);
    // Then it kept the old files, not the mixed ones, as .prev, and swapped in the new
    assert_eq!(prev_files(&vm), old_files(), "{}", run.exit.stderr);
    assert_eq!(read(&vm.join("docker-compose.yml")).as_deref(), Some(NEW_COMPOSE));
    assert_recorded(&run, &vm, 0, "redeploy.succeeded", "none");
});

case!(a_second_swap_keeps_the_first_ones_files_as_previous_and_starts_from_a_clear_staging_dir, ["POST /redeploy"], {
    let (svc, swap) = swapped();
    let vm = svc.world().dir().join("vm");
    // Given a first swap that passed, and a staging dir that something left behind
    let run = run_swap(&svc, &swap, &healthy(), None, LOCAL);
    assert_recorded(&run, &vm, 0, "redeploy.succeeded", "none");
    let first = files(&vm);
    assert_ne!(first, old_files());
    let stale = vm.join(".staging/stale");
    std::fs::create_dir_all(vm.join(".staging")).expect("a staging dir");
    std::fs::write(&stale, "left behind").expect("a stale file");
    // and a .env.local that makes the second swap's .env differ
    std::fs::write(vm.join(".env.local"), "BWS_ACCESS_TOKEN=bws-tok\nLOCAL_ONLY=2\n").expect("a .env.local");
    // When a second swap runs and its up fails
    let rules = [vec![fail(up(1), 5)], healthy()].concat();
    let run = run_swap_on(&svc, &swap, &rules, None, &[stale.clone()]);
    // Then the staging dir was cleared before anything was staged
    assert!(run.calls_of("docker", &["compose", "config"]).iter().all(|c| seen(c, &stale).is_none()));
    // And the restore put the first swap's files back, not the old stack's: .prev was this swap's own copy
    assert_eq!(files(&vm), first, "{}", run.exit.stderr);
    assert_recorded(&run, &vm, 5, "redeploy.failed", "ok");
});

case!(an_app_redeploy_pins_its_tag_only_after_compose_succeeds, ["POST /services/:app/redeploy"], {
    // Given leetcode-backend redeployed at a sha tag, its app sibling recorded
    let mut w = world();
    w.rule(ToolRule::docker(&["run", "--rm"]).rest(HISTORY_APPEND).answer("", "", 0)).rule(ToolRule::docker(&["run", "--pull", "missing"]).rest(APP_SIBLING).answer("", "", 0));
    let svc = w.start();
    assert_eq!(post(&svc, "/services/leetcode-backend/redeploy?tag=sha-89abcde", b"").status, 200);
    let sibling = svc.world().docker_calls()[1].clone();
    let app = svc.world().dir().join("home/ubuntu/leetcode-backend");
    let env = app.join(".env");
    let pin = "IMAGE_TAG=sha-89abcde";
    // and its .env.defaults carrying IMAGE_TAG=latest, as four apps' do, which the synced .env keeps
    let defaults = "IMAGE_TAG=latest\n";
    for (compose_exit, pinned) in [(0, true), (1, false)] {
        let raw = raw_serving(&sibling[sibling.len() - 1], None, defaults);
        let _ = std::fs::remove_dir_all(&app);
        // When its script runs with compose passing, and with the up failing
        let up_fails = fail(compose(&["-f", "docker-compose.prod.yml", "up"]), compose_exit);
        let rules = [if compose_exit == 0 { vec![pass(compose(&[]))] } else { vec![up_fails, pass(compose(&[]))] }, smoke_container()].concat();
        let run = svc.world().run_script(&sibling, &raw, &rules, &[env.clone()], WITHIN);
        // Then the .env ends with the pin only when up passed, and the script fails when it did not
        let text = read(&env).unwrap_or_default();
        assert_eq!(run.exit.code, Some(compose_exit), "{}", run.exit.stderr);
        assert_eq!(text.ends_with(&format!("{pin}\n")), pinned, "{text}");
        // and the pin is the last IMAGE_TAG line, which compose takes over the default before it, or the default alone
        let last = text.lines().rev().find(|l| l.starts_with("IMAGE_TAG=")).unwrap_or_default();
        assert_eq!(last, if pinned { pin } else { "IMAGE_TAG=latest" }, "{text}");
        assert_eq!(text.matches("IMAGE_TAG=").count(), 1 + usize::from(pinned), "{text}");
        // and up never saw the pin: it comes after
        let ups = run.calls_of("docker", &["compose", "-f", "docker-compose.prod.yml", "up"]);
        assert!(!ups.is_empty() && ups.iter().all(|c| !seen(c, &env).unwrap_or("").contains(pin)), "{ups:?}");
        assert!(!exists(&app.join(".env.tmp")));
    }
});
