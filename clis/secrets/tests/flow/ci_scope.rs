//! `wf secret` in CI (issue #367, wave 2): the token of each key's scope, `::add-mask::` of every fetched value, the write URLs only a
//! push to `dev` resolves, the public values that are repository variables, and `scope-check` of the workflows against the scopes.
use super::{held, listed, names, scope_keys, Vault, READ};
use crate::support::{repo_root, run_wf, RunOpts, RunResult};
use serde_json::{json, Value};

const DEPLOY_TOKEN: &str = "deploy-token";
/// One token reads every ci key: the ci-open keys sit in the same project as the ci-deploy ones.
const OPEN_TOKEN: &str = DEPLOY_TOKEN;
const OPEN_KEY: &str = "TEST_RUNS_ORDS_BASE_URL";
const OPEN_KEY_2: &str = "HEALTH_REPORTS_ORDS_BASE_URL";
const DEPLOY_KEY: &str = "GROQ_API_KEY";
const VM_KEY: &str = "VM_STATE_RESTIC_PASSWORD";
const WRITE_KEY: &str = "BUILD_CACHE_WRITE_PAR_URL";
const BUNDLE_KEY: &str = "VERIFICATION_ARTIFACTS_PAR_URL";
const PUBLIC_TOKEN_KEY: &str = "EXPO_PUBLIC_BETTERSTACK_TOKEN";
const PUBLIC_HOST_KEY: &str = "BETTERSTACK_INGEST_HOST";

fn secret_of(key: &str, value: &str) -> Value {
    json!({ "id": key, "key": key, "value": value })
}

/// A CI run's vault: each scope token's project holds the keys of its scope.
fn ci_vault() -> Vault {
    let v = Vault::new();
    v.holds(DEPLOY_TOKEN, json!([secret_of(OPEN_KEY, "open-value-aaaa"), secret_of(OPEN_KEY_2, "open-value-bbbb"), secret_of("BUILD_CACHE_READ_PAR_URL", "https://read.example/o/"), secret_of(DEPLOY_KEY, "deploy-value-cccc"), secret_of(WRITE_KEY, "https://write.example/o/"), secret_of(BUNDLE_KEY, "https://bundle.example/o/")]));
    v
}

fn both() -> Vec<(&'static str, &'static str)> {
    vec![("CI", "true"), ("BWS_CI_DEPLOY_ACCESS_TOKEN", DEPLOY_TOKEN)]
}

fn ci(v: &Vault, args: &[&str], env: &[(&str, &str)]) -> RunResult {
    v.forget();
    v.wf(args, env, None)
}

#[test]
fn ci_reads_each_key_with_the_token_of_its_scope_and_refuses_what_ci_may_not_read() {
    let v = ci_vault();

    // When a ci-open key and a ci-deploy key are read in CI with both scope tokens set
    let open = ci(&v, &["get", OPEN_KEY], &both());
    let open_calls = v.calls();
    let deploy = ci(&v, &["get", DEPLOY_KEY], &both());
    let deploy_calls = v.calls();
    // Then each is listed with the token of its own scope and nothing else
    assert_eq!((open.exit_code, open.stdout.as_str()), (0, "open-value-aaaa\n"), "{}", open.stderr);
    assert_eq!(open_calls, [listed(OPEN_TOKEN)]);
    assert_eq!((deploy.exit_code, deploy.stdout.as_str()), (0, "deploy-value-cccc\n"), "{}", deploy.stderr);
    assert_eq!(deploy_calls, [listed(DEPLOY_TOKEN)]);

    // When a vm key is asked for, by get, sync-env and the verify of a group that carries it
    let vm_get = ci(&v, &["get", VM_KEY], &both());
    let vm_sync = ci(&v, &["sync-env", VM_KEY, "--reveal"], &both());
    let vm_verify = ci(&v, &["verify", "--for", "vm-state-backup"], &both());
    // Then each fails naming the key, its scope and that CI cannot read it, and the vault was never asked
    for r in [&vm_get, &vm_sync, &vm_verify] {
        assert_eq!(r.exit_code, 1, "{}", r.stderr);
        assert!(r.stderr.contains(VM_KEY) && r.stderr.contains("scope vm") && r.stderr.contains("not readable in CI"), "{}", r.stderr);
    }
    assert!(v.calls().is_empty());

    // When a whole-registry walk runs in CI
    let list = ci(&v, &["list"], &both());
    // Then it succeeds and marks the keys CI may not read as refused
    assert_eq!(list.exit_code, 0, "{}", list.stderr);
    assert!(list.stdout.lines().any(|l| l.starts_with(VM_KEY) && l.contains("refused")), "{}", list.stdout);
}

#[test]
fn ci_without_a_scope_token_reads_as_it_always_did() {
    let v = Vault::new();
    v.holds(READ, json!([secret_of(DEPLOY_KEY, "deploy-value-cccc"), secret_of(VM_KEY, "restic-value-1234")]));

    // When CI has only the deploy-read token and asks for a ci-deploy key and for a vm key
    let deploy = ci(&v, &["get", DEPLOY_KEY], &[("CI", "true"), ("BWS_ADMIN_ACCESS_TOKEN", READ)]);
    let vm = ci(&v, &["get", VM_KEY], &[("CI", "true"), ("BWS_ADMIN_ACCESS_TOKEN", READ)]);
    // Then both come from the read token, so the VM runner keeps working until its container-level token is removed
    assert_eq!((deploy.exit_code, deploy.stdout.as_str()), (0, "deploy-value-cccc\n"), "{}", deploy.stderr);
    assert_eq!((vm.exit_code, vm.stdout.as_str()), (0, "restic-value-1234\n"), "{}", vm.stderr);

    // And outside CI the scope tokens are not consulted at all
    let local = ci(&v, &["get", DEPLOY_KEY], &[("BWS_ADMIN_ACCESS_TOKEN", READ)]);
    assert_eq!((local.exit_code, local.stdout.as_str()), (0, "deploy-value-cccc\n"), "{}", local.stderr);
}

fn mask_lines(text: &str) -> Vec<&str> {
    text.lines().filter_map(|l| l.strip_prefix("::add-mask::")).collect()
}

#[test]
fn in_github_actions_every_fetched_value_is_masked_before_it_is_used() {
    let v = ci_vault();
    let mut actions = both();
    actions.push(("GITHUB_ACTIONS", "true"));
    v.holds(OPEN_TOKEN, json!([secret_of(OPEN_KEY, "line-one-1234\nline-two-5678"), secret_of(OPEN_KEY_2, "open-value-bbbb")]));
    let out = v.dir.path().join("masked.env");

    // When a verb that prints no value writes two fetched keys, one of them multi-line, into an env file
    let r = ci(&v, &["sync-env", OPEN_KEY, OPEN_KEY_2, "--out", &out.display().to_string()], &actions);
    // Then stdout announces every value (each line of the multi-line one) ahead of the verb's own line, and a search of what the
    // run printed finds the values only inside the mask lines
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(mask_lines(&r.stdout), ["line-one-1234", "line-two-5678", "open-value-bbbb"], "{}", r.stdout);
    let mask_at = r.stdout.find("::add-mask::open-value-bbbb").unwrap();
    assert!(mask_at < r.stdout.find("Wrote 2 secret(s)").unwrap(), "{}", r.stdout);
    let outside: String = r.stdout.lines().filter(|l| !l.starts_with("::add-mask::")).chain(r.stderr.lines()).collect::<Vec<_>>().join("\n");
    assert!(["line-one-1234", "line-two-5678", "open-value-bbbb"].iter().all(|value| !outside.contains(value)), "{outside}");
    assert!(std::fs::read_to_string(&out).unwrap().contains("open-value-bbbb"));

    // When the value-printing verb get runs
    let got = ci(&v, &["get", OPEN_KEY_2], &actions);
    // Then stdout stays exactly the value (a caller captures it) and the mask goes to stderr, which the runner reads too
    assert_eq!(got.stdout, "open-value-bbbb\n");
    assert_eq!(mask_lines(&got.stderr), ["open-value-bbbb"], "{}", got.stderr);

    // When the same verb runs outside GitHub Actions, even in CI
    let plain = ci(&v, &["sync-env", OPEN_KEY_2, "--out", &out.display().to_string()], &both());
    // Then nothing is announced
    assert_eq!(plain.exit_code, 0, "{}", plain.stderr);
    assert!(!format!("{}{}", plain.stdout, plain.stderr).contains("::add-mask::"));
}

#[test]
fn write_urls_resolve_only_for_a_push_to_dev_in_github_actions() {
    let v = ci_vault();
    let actions = |event: &'static str, git_ref: &'static str| {
        let mut env = both();
        env.extend([("GITHUB_ACTIONS", "true"), ("GITHUB_EVENT_NAME", event), ("GITHUB_REF", git_ref)]);
        env
    };

    // When both write URLs are asked for by a push to dev
    let cache = ci(&v, &["get", WRITE_KEY], &actions("push", "refs/heads/dev"));
    let bundle = ci(&v, &["get", BUNDLE_KEY], &actions("push", "refs/heads/dev"));
    // Then they resolve
    assert_eq!((cache.exit_code, cache.stdout.as_str()), (0, "https://write.example/o/\n"), "{}", cache.stderr);
    assert_eq!((bundle.exit_code, bundle.stdout.as_str()), (0, "https://bundle.example/o/\n"), "{}", bundle.stderr);

    // When they are asked for by a dispatch on dev (the forced cold run), Then they resolve too
    let dispatched = ci(&v, &["get", WRITE_KEY], &actions("workflow_dispatch", "refs/heads/dev"));
    assert_eq!((dispatched.exit_code, dispatched.stdout.as_str()), (0, "https://write.example/o/\n"), "{}", dispatched.stderr);

    // When they are asked for by a schedule on main (GitHub runs schedules only from the default branch), Then they resolve too
    let scheduled = ci(&v, &["get", WRITE_KEY], &actions("schedule", "refs/heads/main"));
    assert_eq!((scheduled.exit_code, scheduled.stdout.as_str()), (0, "https://write.example/o/\n"), "{}", scheduled.stderr);

    // When they are asked for by a pull request, by a push to another branch, by a dispatch on another branch, by a schedule on another ref and by a pull request on main, through get and sync-env
    let refused = [("pull_request", "refs/pull/7/merge"), ("push", "refs/heads/feature"), ("workflow_dispatch", "refs/heads/feature"), ("schedule", "refs/heads/feature"), ("pull_request", "refs/heads/main")];
    let blocked = v.dir.path().join("blocked.env");
    for (event, git_ref) in refused {
        let got = ci(&v, &["get", WRITE_KEY], &actions(event, git_ref));
        let synced = ci(&v, &["sync-env", BUNDLE_KEY, "--out", &blocked.display().to_string()], &actions(event, git_ref));
        // Then each fails with the event and ref named, never asks the vault, and writes nothing
        for r in [&got, &synced] {
            assert_eq!(r.exit_code, 1, "{event}: {}", r.stderr);
            assert!(r.stderr.contains(&format!("write URL refused for {event}/{git_ref}")), "{}", r.stderr);
            assert!(!r.stdout.contains("example"), "{}", r.stdout);
        }
        assert!(v.calls().is_empty() && !blocked.exists());
    }

    // When a read URL is asked for by the same pull request
    let read = ci(&v, &["get", "BUILD_CACHE_READ_PAR_URL"], &actions("pull_request", "refs/pull/7/merge"));
    // Then it still resolves
    assert_eq!((read.exit_code, read.stdout.as_str()), (0, "https://read.example/o/\n"), "{}", read.stderr);

    // When the write URL is asked for outside GitHub Actions (the local gate, the VM by hand), whatever the event variables say
    v.holds(READ, json!([secret_of(WRITE_KEY, "https://write.example/o/")]));
    let local = ci(&v, &["get", WRITE_KEY], &[("BWS_ADMIN_ACCESS_TOKEN", READ), ("GITHUB_EVENT_NAME", "pull_request"), ("GITHUB_REF", "refs/pull/7/merge")]);
    // Then it is unchanged
    assert_eq!((local.exit_code, local.stdout.as_str()), (0, "https://write.example/o/\n"), "{}", local.stderr);
}

#[test]
fn public_betterstack_values_come_from_the_environment_and_never_from_the_vault() {
    let v = ci_vault();
    v.holds(OPEN_TOKEN, json!([secret_of(PUBLIC_HOST_KEY, "vault-host.example"), secret_of(PUBLIC_TOKEN_KEY, "vault-token")]));

    // When CI asks for the public values with the repository variables set
    let mut env = both();
    env.extend([(PUBLIC_TOKEN_KEY, "variable-token-1234"), (PUBLIC_HOST_KEY, "variable-host.example")]);
    let token = ci(&v, &["get", PUBLIC_TOKEN_KEY], &env);
    let host = ci(&v, &["sync-env", PUBLIC_HOST_KEY, "--reveal"], &env);
    // Then they come from those variables and the vault is not asked
    assert_eq!((token.exit_code, token.stdout.as_str()), (0, "variable-token-1234\n"), "{}", token.stderr);
    assert_eq!((host.exit_code, host.stdout.as_str()), (0, "EXPO_PUBLIC_BETTERSTACK_HOST=\"variable-host.example\"\n"), "{}", host.stderr);
    assert!(v.calls().is_empty());

    // When one is not set, although the vault holds a value for it
    let missing = ci(&v, &["get", PUBLIC_HOST_KEY], &both());
    // Then the error names the variable and where to set it, and the vault is still not asked
    assert_eq!(missing.exit_code, 1);
    assert!(missing.stderr.contains(PUBLIC_HOST_KEY) && missing.stderr.contains("repository variable"), "{}", missing.stderr);
    assert!(v.calls().is_empty());
}

struct Planted {
    dir: tempfile::TempDir,
}

impl Planted {
    fn new(workflow: &str, jobs: Value) -> Planted {
        let dir = tempfile::Builder::new().prefix("wf-flow-scope-check-").tempdir().unwrap();
        std::fs::create_dir(dir.path().join("workflows")).unwrap();
        std::fs::write(dir.path().join("workflows/probe.yml"), workflow).unwrap();
        std::fs::write(dir.path().join("jobs.json"), json!({ "jobs": jobs }).to_string()).unwrap();
        Planted { dir }
    }

    fn check(&self) -> (i32, Value) {
        let workflows = self.dir.path().join("workflows").display().to_string();
        let jobs = self.dir.path().join("jobs.json").display().to_string();
        let r = run_wf(&["secret", "scope-check", "--json", "--workflows", &workflows, "--jobs", &jobs], &RunOpts::default().clean_env());
        (r.exit_code, serde_json::from_str(&r.stdout).unwrap_or_else(|e| panic!("{e}: {}{}", r.stdout, r.stderr)))
    }
}

fn job<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["jobs"].as_array().unwrap().iter().find(|j| j["id"] == id).unwrap_or_else(|| panic!("{id} missing in {report}"))
}

#[test]
fn scope_check_fails_an_open_job_that_asks_for_a_deploy_key_and_passes_one_that_does_not() {
    // Given a planted workflow whose open job asks for a ci-deploy key and for a group that carries one, an open job that asks only
    // for ci-open keys, and a deploy job that asks for a vm key
    let planted = Planted::new(
        "name: probe\non: workflow_dispatch\njobs:\n  greedy:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          wf secret get GROQ_API_KEY\n          wf secret sync-env --for ci --out .env\n  modest:\n    runs-on: ubuntu-latest\n    steps:\n      - run: wf secret get TEST_RUNS_ORDS_BASE_URL\n  shipper:\n    runs-on: ubuntu-latest\n    steps:\n      - run: wf secret get VM_STATE_RESTIC_PASSWORD\n",
        json!({ "probe.yml:greedy": { "class": "open", "keys": [] }, "probe.yml:modest": { "class": "open", "keys": [] }, "probe.yml:shipper": { "class": "deploy", "keys": [] } }),
    );

    // When the scopes are checked
    let (code, report) = planted.check();

    // Then it exits 1 and lists the violations per job, naming the key and its scope, and the clean job is ok
    assert_eq!((code, report["ok"].clone()), (1, json!(false)));
    let greedy = job(&report, "probe.yml:greedy");
    assert_eq!(greedy["ok"], json!(false));
    let violations: Vec<&str> = greedy["violations"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(violations.contains(&"probe.yml:greedy: open job asks for GROQ_API_KEY (scope ci-deploy, needs the ci-deploy token)"), "{violations:?}");
    assert!(violations.iter().any(|v| v.contains("BUILD_CACHE_WRITE_PAR_URL")), "the group ci carries a ci-deploy key: {violations:?}");
    assert!(greedy["asks"].as_array().unwrap().iter().any(|a| a == &json!({ "key": "GROQ_API_KEY", "scope": "ci-deploy", "via": "workflow" })));
    assert!(greedy["asks"].as_array().unwrap().iter().any(|a| a["key"] == "BUILD_CACHE_READ_PAR_URL" && a["via"] == "group ci"));
    let modest = job(&report, "probe.yml:modest");
    assert_eq!((modest["ok"].clone(), modest["asks"].clone()), (json!(true), json!([{ "key": "TEST_RUNS_ORDS_BASE_URL", "scope": "ci-open", "via": "workflow" }])));
    assert!(job(&report, "probe.yml:shipper")["violations"][0].as_str().unwrap().contains("VM_STATE_RESTIC_PASSWORD (scope vm, not readable in CI)"));

    // Given the same workflow without the open job's deploy asks and without the vm ask
    let clean = Planted::new(
        "name: probe\non: workflow_dispatch\njobs:\n  modest:\n    runs-on: ubuntu-latest\n    steps:\n      - run: wf secret get TEST_RUNS_ORDS_BASE_URL\n  shipper:\n    runs-on: ubuntu-latest\n    steps:\n      - run: wf secret get GROQ_API_KEY\n",
        json!({ "probe.yml:modest": { "class": "open", "keys": [] }, "probe.yml:shipper": { "class": "deploy", "keys": [] } }),
    );
    // When the scopes are checked
    let (code, report) = clean.check();
    // Then it passes with no violation
    assert_eq!((code, report["ok"].clone(), report["violations"].clone()), (0, json!(true), json!([])));

    // Given a job that is not classified yet asks for a key, and a declared job that no workflow has
    let loose = Planted::new("name: probe\non: workflow_dispatch\njobs:\n  stray:\n    runs-on: ubuntu-latest\n    steps:\n      - run: wf secret get GROQ_API_KEY\n", json!({ "probe.yml:ghost": { "class": "open", "keys": [] } }));
    // When the scopes are checked
    let (code, report) = loose.check();
    // Then both are violations
    let text = report["violations"].to_string();
    assert!(code == 1 && text.contains("probe.yml:stray: asks for GROQ_API_KEY but has no class") && text.contains("probe.yml:ghost: declared in job-scopes.json but not a job of any workflow"), "{text}");
}

#[test]
fn scope_check_of_the_repository_names_every_declared_job_and_prints_the_same_report_twice() {
    // When the repository's own workflows are checked against the committed job classes
    let run = || run_wf(&["secret", "scope-check", "--json"], &RunOpts::default().clean_env().cwd(&repo_root()));
    let (first, second) = (run(), run());

    // Then the report is stable JSON whose exit code follows its ok, no declared job is missing from the workflows, no key or group
    // is unknown, and every job that asks for a key has a class
    assert_eq!(first.stdout, second.stdout);
    let report: Value = serde_json::from_str(&first.stdout).unwrap_or_else(|e| panic!("{e}: {}", first.stdout));
    assert_eq!(first.exit_code, if report["ok"] == json!(true) { 0 } else { 1 });
    assert!(report["jobs"].as_array().unwrap().iter().any(|j| j["id"] == "pipeline.yml:flow-tests" && j["class"] == "deploy"));
    for violation in report["violations"].as_array().unwrap() {
        let text = violation.as_str().unwrap();
        assert!(!["declared in job-scopes.json", "unknown group", "not a registry key", "has no class"].iter().any(|m| text.contains(m)), "{text}");
    }
}

#[test]
fn scope_check_lets_a_deploy_job_verify_a_group_whose_vm_only_key_it_never_reads() {
    // Given a deploy job that verifies the group of vm-ops-api (which carries VM_PROVISION_GITHUB_TOKEN for the VM), and one that names the key itself
    let planted = Planted::new(
        "name: probe\non: workflow_dispatch\njobs:\n  leg:\n    runs-on: ubuntu-latest\n    steps:\n      - run: wf secret verify --for vm-ops-api\n  greedy:\n    runs-on: ubuntu-latest\n    steps:\n      - run: wf secret get VM_PROVISION_GITHUB_TOKEN\n",
        json!({ "probe.yml:leg": { "class": "deploy", "keys": [] }, "probe.yml:greedy": { "class": "deploy", "keys": [] } }),
    );

    // When the scopes are checked
    let (code, report) = planted.check();

    // Then the group alone is clean of the key and the job that names it is the only violation
    assert_eq!(code, 1);
    let leg = job(&report, "probe.yml:leg");
    assert!(!leg["asks"].as_array().unwrap().iter().any(|a| a["key"] == "VM_PROVISION_GITHUB_TOKEN"), "{leg}");
    assert!(report["violations"].to_string().contains("probe.yml:greedy: deploy job asks for VM_PROVISION_GITHUB_TOKEN (scope vm, not readable in CI)"), "{report}");
    assert!(!report["violations"].to_string().contains("probe.yml:leg: deploy job asks for VM_PROVISION_GITHUB_TOKEN"), "{report}");
}

/// The scope's registry keys as vault secrets, each value distinct and carrying the label.
fn project_of(scope: &str, label: &str) -> Vec<Value> {
    scope_keys(scope).iter().enumerate().map(|(i, (_, name))| json!({ "id": i.to_string(), "key": name, "value": format!("{label}-secret-of-{name}") })).collect()
}

/// The ci project as a CI run sees it: one project, one token, holding exactly the ci-open and the ci-deploy keys of the registry.
fn two_projects() -> Vault {
    let v = Vault::new();
    let mut all = project_of("ci-open", "open");
    all.extend(project_of("ci-deploy", "deploy"));
    v.holds(DEPLOY_TOKEN, Value::Array(all));
    v
}

fn push_to_dev() -> Vec<(&'static str, &'static str)> {
    let mut env = both();
    env.extend([("GITHUB_ACTIONS", "true"), ("GITHUB_EVENT_NAME", "push"), ("GITHUB_REF", "refs/heads/dev")]);
    env
}

fn only_token(v: &Vault, token: &str) -> bool {
    let calls = v.calls();
    !calls.is_empty() && calls.iter().all(|c| c.contains(&format!("token={token} ")))
}

#[test]
fn one_project_serves_both_scopes_to_the_one_ci_token_and_verify_reports_exactly_the_scope() {
    let v = two_projects();
    let env = push_to_dev();
    let open: Vec<String> = scope_keys("ci-open").into_iter().map(|(k, _)| k).collect();
    let deploy: Vec<String> = scope_keys("ci-deploy").into_iter().map(|(k, _)| k).collect();
    assert_eq!(open.len(), 6);

    // When every ci-open key is read in GitHub Actions with both tokens set
    for key in &open {
        let r = ci(&v, &["get", key], &env);
        // Then each resolves from the open project, and the vault was asked with the open token only
        assert_eq!((r.exit_code, r.stdout.clone()), (0, format!("open-secret-of-{}\n", scope_keys("ci-open").iter().find(|(k, _)| k == key).unwrap().1)), "{key}: {}", r.stderr);
        assert!(only_token(&v, OPEN_TOKEN), "{key}: {:?}", v.calls());
    }

    // When every ci-deploy key is read the same way
    for key in &deploy {
        let r = ci(&v, &["get", key], &env);
        // Then each resolves from the deploy project with the deploy token only
        assert_eq!(r.exit_code, 0, "{key}: {}", r.stderr);
        assert!(r.stdout.starts_with("deploy-secret-of-"), "{key}: {}", r.stdout);
        assert!(only_token(&v, DEPLOY_TOKEN), "{key}: {:?}", v.calls());
    }

    // When a vm key is read
    let vm = ci(&v, &["get", VM_KEY], &env);
    // Then it is refused before any project is asked
    assert_eq!(vm.exit_code, 1);
    assert!(vm.stderr.contains("scope vm") && vm.stderr.contains("not readable in CI"), "{}", vm.stderr);
    assert!(v.calls().is_empty());

    // When each CI scope is verified with the token of a CI run
    for (scope, token, own, other) in [("ci-open", OPEN_TOKEN, &open, &deploy), ("ci-deploy", DEPLOY_TOKEN, &deploy, &open)] {
        let r = ci(&v, &["verify", "--scope", scope], &env);
        // Then the report lists exactly the scope's keys, all present, none missing, no key of the other scope, no value, and only
        // the scope's own token went to the vault
        assert_eq!(r.exit_code, 0, "{scope}: {}{}", r.stdout, r.stderr);
        let report: Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!((report["scope"].clone(), report["ok"].clone()), (json!(scope), json!(true)));
        let reported = names(&report, "keys");
        assert_eq!(reported, own.iter().cloned().collect(), "{scope}");
        assert!(reported.is_disjoint(&other.iter().cloned().collect()), "{scope}");
        assert!(report["keys"].as_array().unwrap().iter().all(|k| k["status"] == "present"));
        assert!(names(&report, "missing").is_empty() && names(&report, "exposed").is_empty());
        assert!(!r.stdout.contains("secret-of-"), "{}", r.stdout);
        assert!(only_token(&v, token), "{scope}: {:?}", v.calls());
    }
}

#[test]
fn verify_scope_in_ci_names_a_key_missing_from_the_ci_project() {
    // Given a ci project that lost one ci-open key
    let v = two_projects();
    let (lost, lost_name) = scope_keys("ci-open").remove(0);
    let mut all = project_of("ci-open", "open");
    all.retain(|s| s["key"] != json!(lost_name));
    all.extend(project_of("ci-deploy", "deploy"));
    v.holds(DEPLOY_TOKEN, Value::Array(all));

    // When the ci-open scope is verified
    let r = ci(&v, &["verify", "--scope", "ci-open"], &push_to_dev());

    // Then the key is reported missing
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(names(&report, "missing"), [lost].into_iter().collect());
    assert!(only_token(&v, OPEN_TOKEN), "{:?}", v.calls());
}

fn parity(v: &Vault, args: &[&str]) -> (i32, Value, String) {
    let mut full = vec!["parity", "--json"];
    full.extend_from_slice(args);
    let r = ci(v, &full, &push_to_dev());
    let text: String = format!("{}{}", r.stdout, r.stderr).lines().filter(|l| !l.starts_with("::add-mask::")).collect::<Vec<_>>().join("\n");
    (r.exit_code, serde_json::from_str(&r.stdout).unwrap_or_else(|e| panic!("{e}: {text}")), text)
}

fn scope_of<'a>(report: &'a Value, scope: &str) -> &'a Value {
    report["scopes"].as_array().unwrap().iter().find(|s| s["scope"] == scope).unwrap()
}

const NONCE: &str = "shared-run-nonce-0123456789";

#[test]
fn parity_of_two_runner_kinds_with_one_nonce_is_identical_and_a_divergent_value_or_missing_key_shows() {
    let v = two_projects();

    // When two runner kinds (a VM runner and a hosted one: same tokens, one with a different working directory and workflow event
    // variables) run the parity verb with the same nonce
    let (vm_code, vm_run, vm_text) = parity(&v, &["--nonce", NONCE]);
    let hosted = ci(&v, &["parity", "--json", "--nonce", NONCE], &[("CI", "true"), ("GITHUB_ACTIONS", "true"), ("GITHUB_EVENT_NAME", "push"), ("GITHUB_REF", "refs/heads/dev"), ("BWS_CI_DEPLOY_ACCESS_TOKEN", DEPLOY_TOKEN), ("RUNNER_NAME", "hosted-1")]);
    // Then the reports are byte-identical, hold exactly the registry keys of each scope, and carry a hash per key
    assert_eq!((vm_code, hosted.exit_code), (0, 0), "{}", hosted.stderr);
    assert_eq!(hosted.stdout, serde_json::to_string_pretty(&vm_run).unwrap() + "\n");
    for scope in ["ci-open", "ci-deploy"] {
        let s = scope_of(&vm_run, scope);
        let expected: Vec<String> = { let mut k: Vec<String> = scope_keys(scope).into_iter().map(|(k, _)| k).collect(); k.sort(); k };
        assert_eq!((s["registry"].clone(), s["held"].clone(), s["missing"].clone()), (json!(expected), json!(expected), json!([])), "{scope}");
        assert_eq!(s["registryHash"], s["heldHash"]);
        assert_eq!(s["valueHashes"].as_object().unwrap().len(), expected.len());
    }
    // And no value or token appears in what the run printed outside the runner's mask commands
    assert!(!vm_text.contains("secret-of-") && !vm_text.contains(OPEN_TOKEN) && !vm_text.contains(DEPLOY_TOKEN), "{vm_text}");

    // When the ci-deploy project holds a different value for one key and lacks another, and the verb runs again with the same nonce
    let (key, name) = scope_keys("ci-deploy").remove(0);
    let (gone, gone_name) = scope_keys("ci-deploy").remove(1);
    let mut deploy: Vec<Value> = scope_keys("ci-deploy").iter().enumerate().map(|(i, (_, n))| json!({ "id": i.to_string(), "key": n, "value": format!("deploy-secret-of-{n}") })).collect();
    deploy.retain(|s| s["key"] != json!(gone_name));
    for s in deploy.iter_mut().filter(|s| s["key"] == json!(name)) {
        s["value"] = json!("stale-value-9999");
    }
    deploy.extend(project_of("ci-open", "open"));
    v.holds(DEPLOY_TOKEN, Value::Array(deploy));
    let (code, drifted, text) = parity(&v, &["--nonce", NONCE]);
    // Then it exits 1, names the missing key, differs from the first run in the changed value's hash and the held names' hash only,
    // and leaves the ci-open scope identical
    assert_eq!(code, 1, "{text}");
    assert_eq!(drifted["ok"], json!(false));
    let (before, after) = (scope_of(&vm_run, "ci-deploy"), scope_of(&drifted, "ci-deploy"));
    assert_eq!(after["missing"], json!([gone]));
    assert_ne!(after["heldHash"], before["heldHash"]);
    assert_eq!(after["registryHash"], before["registryHash"]);
    assert_ne!(after["valueHashes"][&key], before["valueHashes"][&key]);
    let untouched = scope_keys("ci-deploy").into_iter().map(|(k, _)| k).find(|k| *k != key && *k != gone).unwrap();
    assert_eq!(after["valueHashes"][&untouched], before["valueHashes"][&untouched]);
    assert_eq!(scope_of(&drifted, "ci-open"), scope_of(&vm_run, "ci-open"));
    assert!(!text.contains("stale-value-9999"), "{text}");
}

#[test]
fn parity_hashes_depend_on_the_nonce_and_a_nonce_is_generated_when_none_is_given() {
    let v = two_projects();

    // When the verb runs twice without a nonce, and once with an explicit one that is too short
    let (_, first, _) = parity(&v, &[]);
    let (_, second, _) = parity(&v, &[]);
    let short = ci(&v, &["parity", "--json", "--nonce", "short"], &push_to_dev());

    // Then each run prints its own nonce, the hashes of the same values differ between them, the names hashes do not, and a short
    // nonce is refused naming the rule
    let (n1, n2) = (first["nonce"].as_str().unwrap(), second["nonce"].as_str().unwrap());
    assert!(n1.len() >= 16 && n1 != n2);
    let key = scope_keys("ci-open").remove(0).0;
    assert_ne!(scope_of(&first, "ci-open")["valueHashes"][&key], scope_of(&second, "ci-open")["valueHashes"][&key]);
    assert_eq!(scope_of(&first, "ci-open")["heldHash"], scope_of(&second, "ci-open")["heldHash"]);
    assert_eq!(short.exit_code, 1);
    assert!(short.stderr.contains("--nonce must be at least 16 characters"), "{}", short.stderr);
}

#[test]
fn parity_lists_a_key_only_deploy_groups_call_optional_as_missing_optional_without_failing() {
    // Given a ci-deploy project that lacks GROQ_API_KEY, which no deploy group requires (the app falls back without it)
    let v = two_projects();
    let mut deploy = held(&scope_keys("ci-deploy"));
    deploy.retain(|s| s["key"] != json!("GROQ_API_KEY"));
    deploy.extend(project_of("ci-open", "open"));
    v.holds(DEPLOY_TOKEN, Value::Array(deploy));

    // When the parity verb runs
    let (code, report, text) = parity(&v, &["--nonce", NONCE]);

    // Then it passes, lists the key as missingOptional and not as missing, and prints no value
    assert_eq!(code, 0, "{text}");
    let s = scope_of(&report, "ci-deploy");
    assert_eq!((s["missing"].clone(), s["missingOptional"].clone()), (json!([]), json!(["GROQ_API_KEY"])), "{text}");
    assert!(!text.contains("secret-of-"), "{text}");
}
