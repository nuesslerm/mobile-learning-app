//! `wf rca login`: a provider CLI's own login started from the command line alone (no T3, no Orca) over the real rca service and the real
//! `rca-exec`, with a scripted codex that prints a device address and code and then keeps its token in the executor's home.
use crate::support::{run_wf, RunOpts, RunResult};
use exec_testkit::{RcaExec, RcaService, Script, ScriptedCli, TempRepo, Turn};
use serde_json::{json, Value};

const SECRET: &str = "s3cret";
const DEVICE: [&str; 2] = ["Open https://login.example.test/device to sign in.", "Enter the code ABCD-1234 there."];

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

struct Rig {
    cli: ScriptedCli,
    service: RcaService,
    home: tempfile::TempDir,
    _executor: RcaExec,
    _repo: TempRepo,
}

impl Rig {
    fn new() -> Rig {
        let cli = ScriptedCli::install();
        let service = RcaService::start(SECRET, &cli.path());
        let (repo, home) = (TempRepo::new(), tempfile::tempdir().unwrap());
        let _executor = RcaExec::serve_with_home(&service.url(), "w1", &repo, SECRET, &[], &cli.path(), Some(home.path()));
        Rig { cli, service, home, _executor, _repo: repo }
    }

    fn run_with(&self, bearer: Option<&str>, args: &[&str]) -> RunResult {
        let mut opts = RunOpts::default().env("CHAT_AGENT_API_SECRET", SECRET).env("RCA_HOST", "127.0.0.1").env("RCA_PORT", &self.service.addr().port().to_string()).env("WF_TAILNET_SOCKS", "");
        if let Some(token) = bearer {
            opts = opts.env("RCA_AGENT_BEARER", token);
        }
        run_wf(&[&["rca"], args].concat(), &opts)
    }

    fn login(&self, args: &[&str]) -> RunResult {
        self.run_with(None, &[&["login"], args].concat())
    }
}

#[test]
fn a_key_typed_opencode_login_takes_the_provider_and_the_key_as_two_answers_and_no_reply_carries_the_key() {
    // Given an executor and an opencode whose login asks for a provider and then a key (a scripted double of the line prompts, not the picker of the real CLI)
    let rig = Rig::new();
    rig.cli.script("opencode", &Script::of(vec![Turn::default().then_line("Keys are made at https://opencode.example.test/keys").then_line("Provider:").then_reading_a_line().then_line("API key:").then_reading_a_line().then_writing_credential("HOME", ".local/share/opencode/auth.json", "{\"key\":\"{paste}\"}")]).printing_version("0.0.0-flow"));
    // When the login starts, then the provider and the key are answered
    let started = rig.login(&["start", "opencode"]);
    assert_eq!(started.exit_code, 0, "{}{}", started.stdout, started.stderr);
    let session = line(&started)["result"]["auth"].clone();
    assert_eq!((session["method"].clone(), session["status"].clone(), session["url"].clone()), (json!("paste"), json!("pending"), json!("https://opencode.example.test/keys")), "{session}");
    let auth = session["auth"].as_str().unwrap();
    let answers: Vec<RunResult> = ["anthropic", "sk-flow-key-1234"].iter().map(|text| rig.login(&["respond", auth, text])).collect();
    let done = rig.login(&["complete", auth, "--wait", "30"]);
    // Then the login succeeded, the CLI kept the key in the executor's home, opencode reads signed in, and no reply of the service names the key
    assert_eq!(line(&done)["result"]["auth"]["status"], json!("succeeded"), "{}{}", done.stdout, done.stderr);
    assert_eq!(std::fs::read_to_string(rig.home.path().join(".local/share/opencode/auth.json")).unwrap(), "{\"key\":\"sk-flow-key-1234\"}");
    let status = rig.login(&["status"]);
    let opencode = line(&status)["result"]["providers"].as_array().unwrap().iter().find(|p| p["provider"] == "opencode").unwrap().clone();
    assert_eq!(opencode["status"], json!("signed_in"));
    for reply in answers.iter().chain([&started, &done, &status]) {
        assert!(!format!("{}{}", reply.stdout, reply.stderr).contains("sk-flow-key-1234"), "{}", reply.stdout);
    }
    // And a device-code method that opencode lacks is refused with the methods it has
    let refused = line(&rig.login(&["start", "opencode", "--method", "device_code"]));
    assert_eq!(refused["code"], json!("UNSUPPORTED_METHOD"), "{refused}");
}

#[test]
fn a_running_login_shows_its_address_and_code_as_fields_refuses_a_second_start_and_a_cancel_or_a_failure_ends_it_for_a_new_start() {
    // Given a codex whose first login waits for the person and whose second ends with an error
    let rig = Rig::new();
    let (first, second) = (Turn::default().then_line(DEVICE[0]).then_line(DEVICE[1]).hanging(), Turn::default().then_line(DEVICE[0]).complaining("device code denied").exiting(1));
    rig.cli.script("codex", &Script::of(vec![first, second]).printing_version("0.0.0-flow"));
    // When the login starts and its state is read without waiting
    let session = line(&rig.login(&["start", "codex"]))["result"]["auth"].clone();
    let auth = session["auth"].as_str().unwrap();
    let peeked = line(&rig.login(&["complete", auth, "--wait", "0"]))["result"]["auth"].clone();
    // Then the address and the code are fields of the pending session, and the status lists codex as authenticating
    assert_eq!((peeked["status"].clone(), peeked["url"].clone(), peeked["code"].clone()), (json!("pending"), json!("https://login.example.test/device"), json!("ABCD-1234")), "{peeked}");
    let listed = line(&rig.login(&["status"]))["result"]["providers"].as_array().unwrap().iter().find(|p| p["provider"] == "codex").unwrap().clone();
    assert_eq!(listed["status"], json!("authenticating"));
    // And a second start while it runs is refused naming the running login
    let again = rig.login(&["start", "codex"]);
    let refused = line(&again);
    assert_eq!((again.exit_code, refused["code"].clone()), (1, json!("AUTH_IN_PROGRESS")), "{refused}");
    assert!(refused["error"].as_str().unwrap().contains(auth), "{refused}");
    // When it is cancelled
    let cancelled = line(&rig.login(&["cancel", auth]))["result"]["auth"].clone();
    // Then it reads cancelled, a second cancel and an answer to it are refused as ended, and an unknown login is refused as unknown
    assert_eq!(cancelled["status"], json!("cancelled"));
    assert_eq!(line(&rig.login(&["cancel", auth]))["code"], json!("AUTH_ENDED"));
    assert_eq!(line(&rig.login(&["respond", auth, "x"]))["code"], json!("AUTH_ENDED"));
    assert_eq!(line(&rig.login(&["complete", "auth-99", "--wait", "0"]))["code"], json!("UNKNOWN_AUTH"));
    // And a new start may run, fails with the words the CLI wrote, and leaves codex signed out
    let next = line(&rig.login(&["start", "codex"]))["result"]["auth"].clone();
    let failed = line(&rig.login(&["complete", next["auth"].as_str().unwrap(), "--wait", "30"]))["result"]["auth"].clone();
    assert_eq!(failed["status"], json!("failed"), "{failed}");
    assert!(failed["error"].as_str().unwrap().contains("device code denied"), "{failed}");
    let listed = line(&rig.login(&["status"]))["result"]["providers"].as_array().unwrap().iter().find(|p| p["provider"] == "codex").unwrap().clone();
    assert_eq!(listed["status"], json!("signed_out"));
    assert!(!rig.home.path().join(".codex/auth.json").exists());
}

#[test]
fn a_login_started_under_one_bearer_is_answered_by_another_and_outlives_a_revoked_one() {
    // Given two bearers minted for the executor's service and a codex login started under the first
    let rig = Rig::new();
    rig.cli.script("codex", &Script::of(vec![Turn::default().then_line(DEVICE[0]).then_line(DEVICE[1]).then_sleeping(600).then_writing_credential("HOME", ".codex/auth.json", "{\"access_token\":\"t\"}")]).printing_version("0.0.0-flow"));
    let mint = |label: &str| line(&rig.run_with(None, &["bearers", "mint", "--label", label]));
    let (first, second) = (mint("flow-a"), mint("flow-b"));
    let (token_a, token_b) = (first["token"].as_str().unwrap(), second["token"].as_str().unwrap());
    let started = rig.run_with(Some(token_a), &["login", "start", "codex"]);
    let auth = line(&started)["result"]["auth"]["auth"].as_str().unwrap().to_string();
    // When the first bearer is revoked
    assert_eq!(line(&rig.run_with(None, &["bearers", "revoke", first["id"].as_str().unwrap()]))["ok"], json!(true));
    // Then it opens no socket, the second still reads the same login and waits for it to end, and the login belongs to the service, not the bearer
    let refused = rig.run_with(Some(token_a), &["login", "status"]);
    assert_eq!((refused.exit_code, line(&refused)["ok"].clone()), (1, json!(false)), "{}", refused.stdout);
    let done = rig.run_with(Some(token_b), &["login", "complete", &auth, "--wait", "30"]);
    assert_eq!(line(&done)["result"]["auth"]["status"], json!("succeeded"), "{}{}", done.stdout, done.stderr);
    assert!(rig.home.path().join(".codex/auth.json").is_file());
}
