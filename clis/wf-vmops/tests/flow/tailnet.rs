//! `wf vm net resolve`: the name a tailnet host answers to now.
use crate::support::{run_wf, RunOpts};
use serde_json::{json, Value};

#[test]
fn resolve_names_the_tailnet_host_each_base_name_answers_to_and_fails_naming_one_that_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let resolve = |names: &[&str]| run_wf(&[&["vm", "net", "resolve"], names].concat(), &RunOpts::default().cwd(dir.path()));
    // Given a name the machine resolves, When it is resolved, Then that name is its own answer
    let r = resolve(&["localhost"]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(serde_json::from_str::<Value>(r.stdout.trim()).unwrap(), json!({ "localhost": "localhost" }));
    // Given a name neither it nor any of its -1 .. -5 drifts resolves, Then the line says null for it and the exit names it
    let r = resolve(&["localhost", "wf-flow-no-such-host"]);
    assert_eq!(r.exit_code, 1, "{}", r.stdout);
    assert_eq!(serde_json::from_str::<Value>(r.stdout.trim()).unwrap(), json!({ "localhost": "localhost", "wf-flow-no-such-host": null }));
    assert!(r.stderr.contains("no name resolves for wf-flow-no-such-host (tried each with -1 to -5)"), "{}", r.stderr);
}
