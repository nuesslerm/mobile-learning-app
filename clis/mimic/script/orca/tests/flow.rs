//! The Orca target against a fake sidecar: the driver runs a legacy Orca case through the phone, the repository a case names is made fresh and
//! masked, and a new server starts after `reset`.
use mimic_core::ports::{Driver, Request};
use mimic_script_core::Policy;
use mimic_script_host::ScriptDriver;
use mimic_script_orca::OrcaLaunch;
use serde_json::{json, Value};

const FAKE: &str = r#"while read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":"\([0-9]*\)".*/\1/p')
  case "$line" in
    *'"op":"connect"'*) echo "{\"id\":\"$id\",\"ok\":{\"state\":\"connected\"}}" ;;
    *'"op":"request"'*) echo "{\"id\":\"$id\",\"ok\":{\"response\":{\"ok\":true,\"result\":{\"echo\":1}}}}" ;;
    *'"op":"quit"'*) echo "{\"id\":\"$id\",\"ok\":{}}"; exit 0 ;;
    *) echo "{\"id\":\"$id\",\"ok\":{}}" ;;
  esac
done"#;

fn driver() -> ScriptDriver {
    let launch = OrcaLaunch { sidecar: vec!["sh".into(), "-c".into(), FAKE.into()], env: vec![("PATH".into(), "/usr/bin:/bin".into())], cwd: None };
    ScriptDriver::new(Box::new(launch), Policy::default(), Vec::new(), false, Some("1.4.259".into()))
}

fn ask(driver: &mut ScriptDriver, input: Value) -> Vec<Value> {
    driver.run(&Request { id: "c".into(), op: Some("script".into()), input }).unwrap().steps.into_iter().map(|s| s.payload).collect()
}

#[test]
fn given_a_case_with_a_repository_when_run_then_the_request_carries_the_masked_path_and_the_answer_comes_from_the_sidecar() {
    let mut driver = driver();

    let steps = ask(&mut driver, json!({"requests": [{"method": "repo.add", "params": {"path": "{git.main}"}}], "setup": [{"git": "main"}]}));

    assert_eq!(steps, [json!({"method": "repo.add", "params": {"path": "<scratch>/main"}}), json!({"ok": true, "result": {"echo": 1}})]);
}

#[test]
fn given_two_cases_naming_the_same_repository_when_run_then_each_gets_a_fresh_one_and_both_are_masked_the_same() {
    let mut driver = driver();
    let case = json!({"requests": [{"method": "repo.add", "params": {"path": "{git.main}"}}], "setup": [{"git": "main"}]});

    let first = ask(&mut driver, case.clone());
    let second = ask(&mut driver, case);

    assert_eq!(first, second);
    assert_eq!(first[0]["params"]["path"], "<scratch>/main");
}

#[test]
fn given_a_reset_when_another_case_runs_then_a_new_sidecar_serves_it() {
    let mut driver = driver();
    let case = json!({"requests": [{"method": "files.list"}]});

    let before = ask(&mut driver, case.clone());
    driver.reset().unwrap();
    let after = ask(&mut driver, case);

    assert_eq!(before, after);
}
