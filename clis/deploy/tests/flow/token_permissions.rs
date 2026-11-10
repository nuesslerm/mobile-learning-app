//! `wf vm ops token-permissions`, run on agent-shell (which holds the PAT): read-only GETs to a fake GitHub, yes/no per probe, the token
//! and every reply body kept out of the output.
use crate::support::{run_wf, MockReply, MockServer, RunOpts};
use serde_json::{json, Value};

const PAT: &str = "github_pat_11AAAAAAA0zzzzzzzzzzzz_secretsecretsecret";
const REPO: &str = "/repos/nuesslerm/mobile-learning-app";

fn github(user: u16, repo: u16, runners: u16, contents: u16) -> MockServer {
    let server = MockServer::start(None);
    let mut with_expiry = MockReply::json(user, json!({ "login": "owner-login-xyz" }));
    with_expiry.headers.insert("github-authentication-token-expiration".into(), "2027-01-31 00:00:00 UTC".into());
    server.routes(&[
        ("GET /user", with_expiry),
        (&format!("GET {REPO}"), MockReply::json(repo, json!({ "full_name": "private-name", "permissions": { "admin": true, "push": false, "pull": true } }))),
        (&format!("GET {REPO}/actions/runners"), MockReply::json(runners, json!({ "runners": [{ "name": "runner-secret-name" }] }))),
        (&format!("GET {REPO}/contents/README.md"), MockReply::json(contents, json!({ "content": "readme-body-xyz" }))),
    ]);
    server
}

fn probe(server: &MockServer, pat: Option<&str>) -> crate::support::RunResult {
    let mut opts = RunOpts::default().clean_env().env("WF_GITHUB_API", &format!("http://127.0.0.1:{}", server.port));
    if let Some(pat) = pat {
        opts = opts.env("VM_PROVISION_GITHUB_TOKEN", pat);
    }
    run_wf(&["vm", "ops", "token-permissions"], &opts)
}

fn granted(report: &Value, name: &str) -> (Value, Value) {
    let row = report["probes"].as_array().unwrap().iter().find(|p| p["probe"] == name).unwrap_or_else(|| panic!("{name} missing in {report}"));
    (row["granted"].clone(), row["status"].clone())
}

#[test]
fn a_token_that_reads_contents_and_runners_is_reported_yes_with_only_gets_and_no_body_in_the_output() {
    // Given a GitHub that accepts the token and answers every read
    let server = github(200, 200, 200, 200);

    // When the probe runs with the PAT in its environment, as on agent-shell
    let r = probe(&server, Some(PAT));

    // Then it exits 0 with yes/no and a status per probe and the token's expiry
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(report["ok"], json!(true));
    assert_eq!(report["expires"], json!("2027-01-31 00:00:00 UTC"));
    assert_eq!(report["repoPermissions"], json!({ "admin": "yes", "push": "no", "pull": "yes" }));
    assert_eq!(granted(&report, "contents-read"), (json!("yes"), json!(200)));
    assert_eq!(granted(&report, "administration-read"), (json!("yes"), json!(200)));

    // and every request was a GET carrying the PAT, to GitHub only
    let asked = server.requests();
    assert_eq!(asked.len(), 4);
    assert!(asked.iter().all(|q| q.method == "GET" && q.auth.as_deref() == Some(format!("Bearer {PAT}").as_str())), "{asked:?}");

    // and neither the token nor any reply body is in the output
    let everything = format!("{}{}", r.stdout, r.stderr);
    for secret in [PAT, "github_pat_", "owner-login-xyz", "private-name", "runner-secret-name", "readme-body-xyz"] {
        assert!(!everything.contains(secret), "{secret} leaked: {everything}");
    }
}

#[test]
fn a_token_without_administration_or_with_none_at_all_fails_naming_the_probe() {
    // Given a GitHub that refuses the runners read (the token lacks Administration: read)
    let no_admin = github(200, 200, 403, 200);

    // When the probe runs
    let r = probe(&no_admin, Some(PAT));

    // Then it exits 1 and says no for that probe and yes for contents
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!((report["ok"].clone(), granted(&report, "administration-read"), granted(&report, "contents-read")), (json!(false), (json!("no"), json!(403)), (json!("yes"), json!(200))));

    // Given a revoked token, which GitHub answers 401 everywhere
    let revoked = github(401, 401, 401, 401);
    // When the probe runs, Then token-accepted is no
    let r = probe(&revoked, Some(PAT));
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!((r.exit_code, granted(&report, "token-accepted")), (1, (json!("no"), json!(401))));

    // Given no token in the environment, When the probe runs, Then it names the key, asks GitHub nothing and exits 1
    let idle = github(200, 200, 200, 200);
    let r = probe(&idle, None);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("VM_PROVISION_GITHUB_TOKEN is not set") && idle.requests().is_empty(), "{}", r.stderr);
}
