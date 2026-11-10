//! `wf ship ci-runner`: the runner is redeployed with a one-hour registration token as RUNNER_TOKEN and its own one-hour Tailscale key as
//! TS_CI_RUNNER_AUTH_KEY, and the PAT that mints the token reaches GitHub and nothing else. vm-ops-api, the GitHub API and the Tailscale API
//! are fakes; the PAT and the Tailscale credentials are given to `wf` the way agent-shell holds them, in its environment.
use crate::support::{run_wf, MockReply, MockServer};
use wf_vmops::testing::{answers, FakeVm};
use serde_json::json;

const PAT: &str = "github_pat_11AAAAAAA0zzzzzzzzzzzz_secretsecretsecret";
const TOKEN: &str = "AABBCC4DDEEFF-registration";
const NODE_KEY: &str = "tskey-auth-kRunner123CNTRL-secretsecretsecret";
const KEYS: &str = "POST /tailnet/example.com/keys";
const MINT: &str = "POST /repos/nuesslerm/mobile-learning-app/actions/runners/registration-token";

struct World {
    vm: FakeVm,
    github: MockServer,
    tailscale: MockServer,
}

impl World {
    fn new(github: MockReply) -> Self {
        let world = Self { vm: FakeVm::start(), github: MockServer::start(None), tailscale: MockServer::start(None) };
        world.github.routes(&[(MINT, github)]);
        world.tailscale.routes(&[("POST /oauth/token", MockReply::json(200, json!({ "access_token": "tok" }))), (KEYS, MockReply::json(200, json!({ "key": NODE_KEY })))]);
        world.vm.replies_then_last(&[answers::reply(200, &answers::redeploy("ci-runner", "latest", "dev", 0, "[redeploy] up\n", ""))]);
        world
    }

    fn ship(&self, pat: Option<&str>) -> crate::support::RunResult {
        self.ship_with(pat, &[], &[])
    }

    fn ship_with(&self, pat: Option<&str>, extra: &[(&str, &str)], flags: &[&str]) -> crate::support::RunResult {
        let mut opts = self
            .vm
            .opts()
            .env("WF_GITHUB_API", &format!("http://127.0.0.1:{}", self.github.port))
            .env("WF_TAILSCALE_API", &format!("http://127.0.0.1:{}", self.tailscale.port))
            .env("TS_TAILNET_ID", "example.com")
            .env("TS_VM_OAUTH_CLIENT_SECRET", "oauth-s3cret");
        if let Some(pat) = pat {
            opts = opts.env("VM_PROVISION_GITHUB_TOKEN", pat);
        }
        for (k, v) in extra {
            opts = opts.env(k, v);
        }
        let mut args = vec!["ship", "ci-runner", "--ref", "dev"];
        args.extend_from_slice(flags);
        run_wf(&args, &opts)
    }
}

#[test]
fn the_runner_is_redeployed_with_a_registration_token_and_the_pat_goes_nowhere_else() {
    // Given agent-shell, which holds the PAT, and GitHub minting a registration token
    let w = World::new(MockReply::json(201, json!({ "token": TOKEN, "expires_at": "2026-11-08T13:00:00Z" })));

    // When the runner is shipped
    let r = w.ship(Some(PAT));

    // Then GitHub was asked once, with the PAT
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let asked = w.github.requests();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].auth.as_deref(), Some(format!("Bearer {PAT}").as_str()));

    // and the runner's own key was minted non-reusable, non-ephemeral, preauthorized, tagged tag:ci-self-hosted, for one hour
    let minted = w.tailscale.requests().into_iter().find(|q| q.url.ends_with("/keys")).expect("a key was minted");
    let asked: serde_json::Value = serde_json::from_str(&minted.body).unwrap();
    assert_eq!(asked["capabilities"]["devices"]["create"], json!({ "reusable": false, "ephemeral": false, "preauthorized": true, "tags": ["tag:ci-self-hosted"] }));
    assert_eq!(asked["expirySeconds"], 3600);

    // and vm-ops-api got one signed redeploy whose body is the token as RUNNER_TOKEN and the key as TS_CI_RUNNER_AUTH_KEY
    let sent = w.vm.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].url, "/services/ci-runner/redeploy?tag=latest&ref=dev");
    assert_eq!(sent[0].body, format!("RUNNER_TOKEN={TOKEN}\nTS_CI_RUNNER_AUTH_KEY={NODE_KEY}\n"));
    w.vm.assert_signed(&sent[0], "POST", &sent[0].url, &sent[0].body);

    // and the PAT is in no request to the VM and in no output line, and the token is not printed either
    let everything = format!("{}{}{:?}", r.stdout, r.stderr, sent.iter().map(|q| (&q.url, &q.headers, &q.body)).collect::<Vec<_>>());
    assert!(!everything.contains(PAT) && !everything.contains("github_pat_"), "the PAT leaked: {everything}");
    assert!(!format!("{}{}", r.stdout, r.stderr).contains(TOKEN));

    // and the key is in the body alone: not in the URL, the headers or any output line
    assert!(!everything.contains(NODE_KEY.trim_start_matches("tskey-auth-")) || sent[0].body.contains(NODE_KEY));
    assert!(!sent[0].url.contains("tskey-") && !format!("{:?}{}{}", sent[0].headers, r.stdout, r.stderr).contains("tskey-auth-"));
}

#[test]
fn when_no_runner_key_can_be_minted_the_ship_goes_on_with_one_line_and_sends_no_key() {
    // Given Tailscale refusing the key request, When the runner is shipped
    let w = World::new(MockReply::json(201, json!({ "token": TOKEN })));
    w.tailscale.routes(&[("POST /oauth/token", MockReply::json(200, json!({ "access_token": "tok" }))), (KEYS, MockReply::json(403, json!({ "message": "requested tags are invalid or not permitted" })))]);
    let r = w.ship(Some(PAT));

    // Then it still succeeds, says in one line that the shared key stays, and the body carries the token alone
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let lines: Vec<&str> = r.stdout.lines().filter(|l| l.contains("keeps joining with the shared key")).collect();
    assert_eq!(lines.len(), 1, "{}", r.stdout);
    assert!(lines[0].contains("403"), "{}", lines[0]);
    let sent = w.vm.requests();
    assert_eq!(sent[0].body, format!("RUNNER_TOKEN={TOKEN}\n"));

    // Given no Tailscale credential readable where wf runs, Then the same degradation
    let w = World::new(MockReply::json(201, json!({ "token": TOKEN })));
    let opts = w.vm.opts().env("WF_GITHUB_API", &format!("http://127.0.0.1:{}", w.github.port)).env("VM_PROVISION_GITHUB_TOKEN", PAT).env("TS_TAILNET_ID", "").env("TS_VM_OAUTH_CLIENT_SECRET", "");
    let r = run_wf(&["ship", "ci-runner", "--ref", "dev"], &opts);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("keeps joining with the shared key"), "{}", r.stdout);
    assert_eq!(w.vm.requests()[0].body, format!("RUNNER_TOKEN={TOKEN}\n"));
    assert!(w.tailscale.requests().is_empty());
}

#[test]
fn a_tailscale_key_given_in_the_environment_is_sent_as_is_and_nothing_is_minted() {
    // Given agent-shell, which holds the PAT but not the Tailscale credentials, and a key minted elsewhere
    let w = World::new(MockReply::json(201, json!({ "token": TOKEN, "expires_at": "2026-11-08T13:00:00Z" })));
    let given = "tskey-auth-kGiven456CNTRL-secretsecretsecret";

    // When the runner is shipped with the key in the environment
    let r = w.ship_with(Some(PAT), &[("TS_CI_RUNNER_AUTH_KEY", given)], &[]);

    // Then the Tailscale API was never asked, the body carries the given key, and the key is in no output line
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(w.tailscale.requests().is_empty(), "{:?}", w.tailscale.requests());
    let sent = w.vm.requests();
    assert_eq!(sent[0].body, format!("RUNNER_TOKEN={TOKEN}\nTS_CI_RUNNER_AUTH_KEY={given}\n"));
    assert!(!format!("{}{}", r.stdout, r.stderr).contains(given));
}

#[test]
fn wipe_state_asks_the_redeploy_to_wipe_the_runner_volumes_and_the_default_does_not() {
    // Given agent-shell and GitHub minting a registration token
    let w = World::new(MockReply::json(201, json!({ "token": TOKEN, "expires_at": "2026-11-08T13:00:00Z" })));

    // When the runner is shipped with --wipe-state
    let r = w.ship_with(Some(PAT), &[], &["--wipe-state"]);

    // Then the redeploy request asks for wipeState and the plain ship does not
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(w.vm.requests()[0].url.contains("wipeState=1"), "{}", w.vm.requests()[0].url);
    let plain = World::new(MockReply::json(201, json!({ "token": TOKEN, "expires_at": "2026-11-08T13:00:00Z" })));
    assert_eq!(plain.ship(Some(PAT)).exit_code, 0);
    assert!(!plain.vm.requests()[0].url.contains("wipeState"), "{}", plain.vm.requests()[0].url);
}

#[test]
fn a_runner_without_the_pat_or_with_github_refusing_ships_nothing() {
    // Given no PAT, When the runner is shipped, Then it fails naming the key and asks nobody
    let w = World::new(MockReply::json(201, json!({ "token": TOKEN })));
    let r = w.ship(None);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("VM_PROVISION_GITHUB_TOKEN is not set"), "{}", r.stderr);
    assert!(w.github.requests().is_empty() && w.vm.requests().is_empty());

    // Given GitHub refusing the PAT, Then nothing is redeployed and the PAT is not echoed
    let w = World::new(MockReply::json(403, json!({ "message": "Resource not accessible by personal access token" })));
    let r = w.ship(Some(PAT));
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("HTTP 403") && r.stderr.contains("Administration: write"), "{}", r.stderr);
    assert!(w.vm.requests().is_empty());
    assert!(!format!("{}{}", r.stdout, r.stderr).contains(PAT));
}
