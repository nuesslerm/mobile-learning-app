//! `wf rca login`: a provider CLI's own login started from the command line alone (no T3, no Orca) over the real rca service and the real
//! `rca-exec`, with a scripted codex that prints a device address and code and then keeps its token in the executor's home.
use crate::support::{run_wf, RunOpts, RunResult};
use exec_testkit::{RcaExec, RcaService, Script, ScriptedCli, TempRepo, Turn};
use serde_json::{json, Value};

const SECRET: &str = "s3cret";

fn line(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}{}", r.stdout, r.stderr))
}

#[test]
fn a_login_started_with_wf_rca_login_alone_leaves_the_cli_signed_in_at_the_executor_and_a_logout_ends_it() {
    // Given the rca service, an executor with its own home, and a scripted codex that prints a device address and code and then writes its token
    let cli = ScriptedCli::install();
    cli.script("codex", &Script::of(vec![Turn::default().then_line("Open https://login.example.test/device to sign in.").then_line("Enter the code ABCD-1234 there.").then_sleeping(400).then_writing_credential("HOME", ".codex/auth.json", "{\"access_token\":\"t\"}")]).printing_version("0.0.0-flow"));
    let service = RcaService::start(SECRET, &cli.path());
    let (repo, home) = (TempRepo::new(), tempfile::tempdir().unwrap());
    let _executor = RcaExec::serve_with_home(&service.url(), "w1", &repo, SECRET, &[], &cli.path(), Some(home.path()));
    let port = service.addr().port().to_string();
    let run = |args: &[&str]| run_wf(&[&["rca", "login"], args].concat(), &RunOpts::default().env("CHAT_AGENT_API_SECRET", SECRET).env("RCA_HOST", "127.0.0.1").env("RCA_PORT", &port).env("WF_TAILNET_SOCKS", ""));

    // When the login starts from the command line
    let started = run(&["start", "codex"]);
    // Then the address and code the CLI printed come back with the session
    assert_eq!(started.exit_code, 0, "{}{}", started.stdout, started.stderr);
    let session = line(&started)["result"]["auth"].clone();
    assert_eq!((session["status"].clone(), session["url"].clone(), session["code"].clone()), (json!("pending"), json!("https://login.example.test/device"), json!("ABCD-1234")), "{session}");
    // And completing it waits for the CLI to keep its token, in the executor's home
    let done = run(&["complete", session["auth"].as_str().unwrap(), "--wait", "30"]);
    assert_eq!(line(&done)["result"]["auth"]["status"], json!("succeeded"), "{}{}", done.stdout, done.stderr);
    assert!(home.path().join(".codex/auth.json").is_file());
    // And the status lists codex signed in
    let status = line(&run(&["status"]));
    let codex = status["result"]["providers"].as_array().unwrap().iter().find(|p| p["provider"] == "codex").unwrap().clone();
    assert_eq!(codex["status"], json!("signed_in"));

    // When it logs out
    let out = run(&["logout", "codex"]);
    // Then codex reads signed out and its credential file is gone
    assert_eq!(line(&out)["result"]["provider"]["status"], json!("signed_out"), "{}{}", out.stdout, out.stderr);
    assert!(!home.path().join(".codex/auth.json").exists());

    // And a provider that does not exist is refused with the service's code and what to do
    let unknown = run(&["start", "nope"]);
    let refused = line(&unknown);
    assert_eq!((unknown.exit_code, refused["ok"].clone(), refused["code"].clone()), (1, json!(false), json!("UNKNOWN_PROVIDER")), "{refused}");
    assert!(refused["next"].as_str().is_some_and(|next| next.contains("wf rca login status")));
}
