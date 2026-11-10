//! `wf vm net`: the tailnet's device records, the VM's auth key (its expiry and its rotation into the vault), and joining this
//! machine to the tailnet (`net resolve` is in `clis/wf-vmops`). The Tailscale admin API is a fake; `bws`, `tailscale` and
//! `tailscaled` are stubs keeping state in files.
use crate::support::{create_sh_stub, iso_from_ms, now_ms, run_wf, MockReply, MockServer, RunOpts, RunResult};
use serde_json::{json, Value};
use std::time::Duration;

const LIST: &str = "GET /tailnet/example.com/devices";
const KEYS: &str = "GET /tailnet/example.com/keys";
const MINTED: &str = "tskey-auth-minted-0123456789";

/// The Tailscale API with its OAuth exchange, and a vault whose `secret list` answers `$BWS_LIST` and whose writes are logged.
struct Admin {
    dir: tempfile::TempDir,
    api: MockServer,
}

impl Admin {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("wf-flow-tailnet-").tempdir().unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        create_sh_stub(&dir.path().join("bin/bws"), "echo \"$@\" >> \"$STUB_DIR/bws.log\"\nif [ \"$1 $2\" = \"secret list\" ]; then printf '%s' \"${BWS_LIST:-[]}\"; exit 0; fi\nexit \"${BWS_EDIT_EXIT:-0}\"");
        Self { dir, api: MockServer::start(None) }
    }

    /// The API answers `table` plus a granted OAuth exchange; earlier requests and vault writes are forgotten.
    fn routes(&self, table: &[(&str, MockReply)]) {
        let mut all: Vec<(&str, MockReply)> = vec![("POST /oauth/token", MockReply::json(200, json!({ "access_token": "tok" })))];
        all.retain(|(k, _)| !table.iter().any(|(t, _)| t == k));
        all.extend(table.iter().cloned());
        self.api.routes(&all);
        self.api.clear();
        let _ = std::fs::remove_file(self.dir.path().join("bws.log"));
    }

    fn wf(&self, args: &[&str], env: &[(&str, Option<&str>)]) -> RunResult {
        let mut merged: Vec<(String, String)> = [
            ("SECRETS_BACKEND", "bitwarden".to_string()),
            ("BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN", "read".to_string()),
            ("BWS_DEPLOY_SECRETS_EDIT_ACCESS_TOKEN", "edit".to_string()),
            ("STUB_DIR", self.dir.path().display().to_string()),
            ("HOME", self.dir.path().display().to_string()),
            ("WF_TAILSCALE_API", format!("http://127.0.0.1:{}", self.api.port)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        // the vault serves the tailnet's id and the OAuth client secret (a case may replace or drop them) besides any listing it brings
        let mut vaulted: Vec<(&str, Option<String>)> = vec![("TS_TAILNET_ID", Some("example.com".into())), ("TS_VM_OAUTH_CLIENT_SECRET", Some("s3cret/&".into())), ("TS_CI_OAUTH_CLIENT_SECRET", None)];
        for (key, value) in env {
            if let Some(slot) = vaulted.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value.map(str::to_string);
                continue;
            }
            merged.retain(|(k, _)| k != key);
            if let Some(v) = value {
                merged.push((key.to_string(), v.to_string()));
            }
        }
        let mut listing: Vec<Value> = merged.iter().find(|(k, _)| k == "BWS_LIST").and_then(|(_, v)| serde_json::from_str(v).ok()).unwrap_or_default();
        listing.extend(vaulted.iter().filter_map(|(k, v)| v.as_ref().map(|v| json!({ "id": k, "key": k, "value": v }))));
        merged.retain(|(k, _)| k != "BWS_LIST");
        merged.push(("BWS_LIST".to_string(), Value::Array(listing).to_string()));
        let mut full = vec!["vm", "net"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default().bin_dir(&self.dir.path().join("bin")).cwd(self.dir.path()).clean_env();
        for (key, value) in &merged {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    fn deletes(&self) -> Vec<String> {
        self.api.requests().iter().filter(|q| q.method == "DELETE").map(|q| q.url.clone()).collect()
    }

    fn vault_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("bws.log")).unwrap_or_default()
    }
}

fn minutes_ago(m: i128) -> String {
    iso_from_ms(now_ms() - m * 60_000 - 100)
}

fn in_days(d: i128) -> String {
    iso_from_ms(now_ms() + d * 86_400_000 + 3_600_000)
}

fn devices() -> Value {
    json!({ "devices": [
        { "id": "d1", "name": "vm-ops-api-main.tail.ts.net", "lastSeen": minutes_ago(2), "tags": ["tag:mobile-learning"] },
        { "id": "d2", "name": "vm-gateway-main.tail.ts.net", "lastSeen": minutes_ago(60), "tags": ["tag:mobile-learning"] },
        { "id": "d3", "name": "markus-phone.tail.ts.net", "lastSeen": minutes_ago(5000), "tags": [] },
        { "id": "d4", "name": "vm-ops-api-main-1.tail.ts.net", "lastSeen": minutes_ago(1), "tags": ["tag:mobile-learning"] },
    ] })
}

#[test]
fn a_revoked_device_management_client_falls_back_to_the_acl_sync_client_and_two_refusals_fail_naming_both() {
    let a = Admin::new();
    // Given the device-management client is revoked (401) and the ACL-sync client's secret is set
    a.api.queue(vec![MockReply::text(401, "revoked"), MockReply::json(200, json!({ "access_token": "tok-ci" })), MockReply::json(200, devices())], false);
    // When the devices are listed
    let r = a.wf(&["device", "list"], &[("TS_CI_OAUTH_CLIENT_SECRET", Some("ci-s3cret"))]);
    // Then the second client's token is used and the listing succeeds
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let requests = a.api.requests();
    assert_eq!(requests.iter().filter(|q| q.url.ends_with("/oauth/token")).count(), 2);
    assert!(requests.iter().any(|q| q.url.contains("/devices") && q.auth.as_deref() == Some("Bearer tok-ci")));
    // Given both clients are refused
    a.api.clear();
    a.api.queue(vec![MockReply::text(401, "revoked"), MockReply::text(401, "revoked")], true);
    // When the devices are listed
    let r = a.wf(&["device", "list"], &[("TS_CI_OAUTH_CLIENT_SECRET", Some("ci-s3cret"))]);
    // Then it fails naming both secrets and leaks neither value
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("Both TS_VM_OAUTH_CLIENT_SECRET and TS_CI_OAUTH_CLIENT_SECRET were refused"), "{}", r.stderr);
    assert!(!r.stderr.contains("ci-s3cret") && !r.stderr.contains("s3cret/&"), "{}", r.stderr);
}

#[test]
fn the_tailnet_s_device_records_are_listed_with_a_verdict_each_and_stale_ones_retired() {
    let a = Admin::new();
    let with_deletes = || [(LIST, MockReply::json(200, devices())), ("DELETE /device/d2", MockReply::json(200, json!({}))), ("DELETE /device/d4", MockReply::json(200, json!({})))];

    // Given four devices, When they are listed, Then each gets an aligned verdict line and the collision suffix is warned about
    a.routes(&[(LIST, MockReply::json(200, devices()))]);
    let r = a.wf(&["device", "list"], &[]);
    assert_eq!(r.exit_code, 0);
    assert_eq!(
        r.stdout,
        [
            "4 device(s) on the tailnet:",
            "  vm-ops-api-main         age=    2m  healthy         tag:mobile-learning",
            "  vm-gateway-main         age=   60m  stale           tag:mobile-learning <-- RETIRE",
            "  markus-phone            age= 5000m  untagged        (untagged)",
            "  vm-ops-api-main-1       age=    1m  online-suffixed tag:mobile-learning",
            "",
            "WARNING: 1 device(s) carry a collision suffix: vm-ops-api-main-1.",
            "vm-gateway forwards to the BARE hostname, so any service named this way is unreachable through it.",
            "Retire the stale record holding the bare name, then restart that host’s tailscale sidecar so it re-registers.",
            "",
        ]
        .join("\n")
    );
    // and the OAuth client secret was exchanged form-encoded for the bearer the listing used
    let requests = a.api.requests();
    assert_eq!((requests[0].method.as_str(), requests[0].url.as_str(), requests[0].body.as_str()), ("POST", "/oauth/token", "client_secret=s3cret%2F%26"));
    assert_eq!((requests[1].method.as_str(), requests[1].url.as_str(), requests[1].auth.as_deref()), ("GET", "/tailnet/example.com/devices", Some("Bearer tok")));
    // Given --stale-minutes, a long-offline tagged device is healthy; an unparseable lastSeen ages as '?'
    assert!(a.wf(&["device", "list", "--stale-minutes", "90"], &[]).stdout.contains("vm-gateway-main         age=   60m  healthy"));
    a.routes(&[(LIST, MockReply::json(200, json!({ "devices": [{ "id": "x", "name": "vm-gateway-a.t", "lastSeen": "never", "tags": ["tag:x"] }] })))]);
    assert!(a.wf(&["device", "list"], &[]).stdout.contains("  vm-gateway-a            age=    ?m  stale           tag:x"));

    // Given no tailnet id, a refused OAuth exchange or a 403, Then each fails with its hint
    a.routes(&[]);
    let r = a.wf(&["device", "list"], &[("TS_TAILNET_ID", None)]);
    assert!(r.exit_code == 1 && r.stderr.contains("wf: TS_TAILNET_ID is not set (or is still a placeholder).") && a.api.requests().is_empty(), "{}", r.stderr);
    a.routes(&[("POST /oauth/token", MockReply::json(401, json!({})))]);
    let r = a.wf(&["device", "list"], &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("wf: Tailscale OAuth exchange failed (401). Check TS_VM_OAUTH_CLIENT_SECRET"), "{}", r.stderr);
    a.routes(&[(LIST, MockReply::text(403, "nope"))]);
    let r = a.wf(&["device", "list"], &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("wf: Tailscale GET /tailnet/example.com/devices failed (403). nope The client authenticated but was refused"), "{}", r.stderr);

    // Given a stale managed device, When retired without --apply, Then it is a dry run; with --apply only it is deleted
    a.routes(&with_deletes());
    let r = a.wf(&["device", "retire"], &[]);
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("vm-gateway-main         age=   60m  stale           tag:mobile-learning <-- RETIRE") && r.stdout.contains("\nWould retire 1 device(s): vm-gateway-main\nDry run — re-run with --apply to delete.\n"), "{}", r.stdout);
    assert!(a.deletes().is_empty());
    let r = a.wf(&["device", "retire", "--apply"], &[]);
    assert!(r.exit_code == 0 && r.stdout.contains("\nRetired 1 device(s): vm-gateway-main\nNow restart the affected host’s tailscale sidecar"), "{}", r.stdout);
    assert_eq!(a.deletes(), ["/device/d2"]);
    // and --force takes an online suffixed one too, which is otherwise spared
    a.routes(&with_deletes());
    a.wf(&["device", "retire", "--apply", "--force"], &[]);
    assert_eq!(a.deletes(), ["/device/d2", "/device/d4"]);
    let mut only_suffixed = with_deletes();
    only_suffixed[0].1 = MockReply::json(200, json!({ "devices": [devices()["devices"][3].clone()] }));
    a.routes(&only_suffixed);
    assert!(a.wf(&["device", "retire"], &[]).stdout.contains("\nNothing to retire.\n1 online device(s) hold a collision suffix but were spared"));
    assert!(a.deletes().is_empty());

    // Given --name, Then only the stale named devices go; an unknown name fails before deleting anything
    a.routes(&with_deletes());
    assert_eq!(a.wf(&["device", "retire", "--name", "vm-gateway-main", "markus-phone", "--apply"], &[]).exit_code, 0);
    assert_eq!(a.deletes(), ["/device/d2"]);
    a.routes(&with_deletes());
    let r = a.wf(&["device", "retire", "--name", "ghost", "--apply"], &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("wf: no such device on the tailnet: ghost. Run `wf vm net device list` to see current names."), "{}", r.stderr);
    assert!(a.deletes().is_empty());
}

fn key(description: &str, expires: &str) -> Value {
    json!({ "id": "k", "description": description, "expires": expires })
}

#[test]
fn the_vm_auth_key_s_expiry_is_watched_and_a_rotation_mints_a_new_key_into_the_vault_without_showing_it() {
    let a = Admin::new();

    // Given a key 40 days from expiry, Then status reports the days left and exits 0; a 60-day threshold demands rotation
    let expires = in_days(40);
    a.routes(&[(KEYS, MockReply::json(200, json!({ "keys": [key("wf-rotate-TS-VM-AUTH-KEY", &expires)] })))]);
    let r = a.wf(&["auth-key", "status"], &[]);
    assert_eq!((r.exit_code, r.stdout.clone()), (0, format!("TS_VM_AUTH_KEY: 40 day(s) remaining (expires {expires})\n  expires in 40 day(s), above the 14-day threshold\n")));
    let r = a.wf(&["auth-key", "status", "--warn-days", "60"], &[]);
    assert!(r.exit_code == 1 && r.stdout.contains("(threshold 60 days)"));

    // Given the newest of two rotated keys is near expiry, Then it exits 1 saying to rotate
    a.routes(&[(KEYS, MockReply::json(200, json!({ "keys": [key("wf-rotate-TS-VM-AUTH-KEY", &in_days(5)), key("wf-rotate-TS-VM-AUTH-KEY", &in_days(2)), key("other", &in_days(80))] })))]);
    let r = a.wf(&["auth-key", "status"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stdout.contains("TS_VM_AUTH_KEY: 5 day(s) remaining") && r.stdout.contains("\nACTION NEEDED: run `wf vm net auth-key rotate --apply` (threshold 14 days).\n"), "{}", r.stdout);
    assert!(r.stderr.contains("wf: TS_VM_AUTH_KEY needs rotation: expires in 5 day(s), at or under the 14-day threshold."), "{}", r.stderr);
    // and when the client owns no keys the expiry is unknown and rotation is due
    a.routes(&[(KEYS, MockReply::json(200, json!({ "keys": null })))]);
    let r = a.wf(&["auth-key", "status"], &[]);
    assert!(r.exit_code == 1 && r.stdout.contains("TS_VM_AUTH_KEY: expiry UNKNOWN\n  no auth key owned by this OAuth client matches 'wf-rotate-TS-VM-AUTH-KEY'."), "{}", r.stdout);

    // Given no --apply, Then rotate explains and mints nothing, echoing the lifetime; out of range or another action is refused
    a.routes(&[]);
    let r = a.wf(&["auth-key", "rotate"], &[]);
    assert_eq!(
        (r.exit_code, r.stdout.as_str()),
        (0, "Dry run — would mint a 90-day reusable tagged key and store it as TS_VM_AUTH_KEY.\nNo key was minted: an unused key still counts against the tailnet’s key list and would need manual cleanup.\nRe-run with --apply to rotate.\n")
    );
    assert!(a.wf(&["auth-key", "rotate", "--expiry-days", "30"], &[]).stdout.contains("would mint a 30-day reusable"));
    let r = a.wf(&["auth-key", "rotate", "--expiry-days", "120"], &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("wf: expiryDays must be between 1 and 90"), "{}", r.stderr);
    let r = a.wf(&["auth-key", "revoke"], &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("wf: unknown auth-key action 'revoke' — the only supported action is 'rotate'."), "{}", r.stderr);
    assert!(a.api.requests().is_empty());

    // Given a vault holding the key, When rotated with --apply, Then a tagged ephemeral key is minted and stored under the same name, never printed
    let listing = r#"[{"id":"1","key":"TS_VM_AUTH_KEY","value":"old-key","organizationId":"o","projectId":"p"}]"#;
    let mint_routes = |reply: MockReply| [("POST /tailnet/example.com/keys", reply)];
    a.routes(&mint_routes(MockReply::json(200, json!({ "key": MINTED }))));
    let r = a.wf(&["auth-key", "rotate", "--apply"], &[("BWS_LIST", Some(listing))]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(
        r.stdout,
        format!("Rotated TS_VM_AUTH_KEY: minted a 90-day reusable tagged key ({} chars) and stored it.\nTailscale caps auth keys at 90 days, and expiry breaks the NEXT join rather than running nodes — so rotate again before then.\n", MINTED.len())
    );
    assert!(!format!("{}{}", r.stdout, r.stderr).contains(MINTED));
    let mint = a.api.requests().into_iter().find(|q| q.url.ends_with("/keys")).unwrap();
    assert_eq!((mint.method.as_str(), mint.auth.as_deref()), ("POST", Some("Bearer tok")));
    assert_eq!(
        serde_json::from_str::<Value>(&mint.body).unwrap(),
        json!({ "capabilities": { "devices": { "create": { "reusable": true, "ephemeral": true, "preauthorized": true, "tags": ["tag:mobile-learning"] } } }, "expirySeconds": 90 * 86400, "description": "wf-rotate-TS-VM-AUTH-KEY" })
    );
    assert!(a.vault_log().contains(&format!("secret edit --value {MINTED}")), "{}", a.vault_log());
    // and --expiry-days 30 mints for 30 days
    a.routes(&mint_routes(MockReply::json(200, json!({ "key": MINTED }))));
    let r = a.wf(&["auth-key", "rotate", "--apply", "--expiry-days", "30"], &[("BWS_LIST", Some(listing))]);
    assert!(r.stdout.contains("minted a 30-day reusable tagged key"), "{}", r.stderr);
    let mint = a.api.requests().into_iter().find(|q| q.url.ends_with("/keys")).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&mint.body).unwrap()["expirySeconds"].as_u64(), Some(30 * 86400), "a whole number: Tailscale refuses 2592000.0 as invalid JSON");

    // Given the mint refused, or answered without a key, Then rotation fails and the vault is untouched
    a.routes(&mint_routes(MockReply::json(403, json!({ "message": "no" }))));
    let r = a.wf(&["auth-key", "rotate", "--apply"], &[("BWS_LIST", Some(listing))]);
    assert!(r.exit_code == 1 && r.stderr.contains("failed (403)") && !a.vault_log().contains("secret edit"), "{}", r.stderr);
    a.routes(&mint_routes(MockReply::json(200, json!({}))));
    let r = a.wf(&["auth-key", "rotate", "--apply"], &[("BWS_LIST", Some(listing))]);
    assert!(r.exit_code == 1 && r.stderr.contains("Tailscale key creation returned no key."), "{}", r.stderr);
    // Given the vault write failing, Then rotation fails with the vault's error
    a.routes(&mint_routes(MockReply::json(200, json!({ "key": MINTED }))));
    let r = a.wf(&["auth-key", "rotate", "--apply"], &[("BWS_LIST", Some(listing)), ("BWS_EDIT_EXIT", Some("1"))]);
    assert!(r.exit_code == 1 && r.stderr.contains("Bitwarden") && !r.stdout.contains("Rotated"), "{}{}", r.stdout, r.stderr);
}

/// `tailscale status|up` reading and writing its mode in a file, and a `tailscaled` that sets the mode it starts into.
const TAILSCALE: &str = r#"mode=$(cat "$STUB_DIR/mode")
case "$1" in
  status)
    if [ "$2" = "--json" ]; then
      printf '{"Self":{"Online":%s},"Peer":{}}' "$(cat "$STUB_DIR/online" 2>/dev/null || echo true)"; exit 0
    fi
    case "$mode" in
      down) echo "failed to connect to local tailscaled; it doesn't appear to be running" >&2; exit 1;;
      loggedout) echo "Logged out."; exit 1;;
      *) echo "100.64.0.1 sandbox-agent user@ linux -"; exit 0;;
    esac;;
  up)
    echo "$@" >> "$STUB_DIR/up.log"
    if [ -n "$UP_FAIL" ]; then echo "$UP_FAIL" >&2; exit 1; fi
    echo joined > "$STUB_DIR/mode"; exit 0;;
esac
exit 0"#;

const HINT: &str = "Reach tailnet services through the proxy, e.g.\n  curl --proxy socks5h://localhost:1055 http://vm-ops-api-main:8080/health\n";

#[test]
fn net_up_starts_the_userspace_daemon_and_joins_this_machine_with_the_vault_key_only_when_it_must() {
    let dir = tempfile::Builder::new().prefix("wf-flow-net-up-").tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    create_sh_stub(&bin.join("tailscale"), TAILSCALE);
    // the daemon renames its mode into place: wf polls `tailscale status` meanwhile, and a read between `>`'s truncate and its
    // write finds no mode, which the stub answers as joined
    create_sh_stub(&bin.join("tailscaled"), "echo \"$@\" >> \"$STUB_DIR/daemon.log\"\necho \"${STATE_AFTER_START:-loggedout}\" > \"$STUB_DIR/mode.next\" && mv \"$STUB_DIR/mode.next\" \"$STUB_DIR/mode\"");
    let vault_with_key = r#"printf '%s' '[{"id":"1","key":"TS_VM_AUTH_KEY","value":"tskey-auth-xyz","organizationId":"o","projectId":"p"}]'"#;
    create_sh_stub(&bin.join("bws"), vault_with_key);
    let log = |name: &str| std::fs::read_to_string(dir.path().join(name)).unwrap_or_default();
    let up = |mode: &str, args: &[&str], env: &[(&str, &str)]| {
        std::fs::write(dir.path().join("mode"), mode).unwrap();
        for name in ["daemon.log", "up.log"] {
            let _ = std::fs::remove_file(dir.path().join(name));
        }
        let mut full = vec!["vm", "net", "up"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default().bin_dir(&bin).cwd(dir.path()).clean_env().timeout(Duration::from_secs(60)).env("STUB_DIR", &dir.path().display().to_string()).env("SECRETS_BACKEND", "bitwarden").env("BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN", "read");
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    };

    // Given a stopped daemon and a logged-out node, Then it starts the daemon in userspace, joins with the vault's key and says so
    let r = up("down", &[], &[]);
    assert_eq!((r.exit_code, r.stdout.clone()), (0, format!("started tailscaled (userspace, socks5 on localhost:1055)\njoined the tailnet as sandbox-agent\n{HINT}")));
    assert_eq!(log("daemon.log"), "--tun=userspace-networking --socks5-server=localhost:1055 --outbound-http-proxy-listen=localhost:1055\n");
    assert_eq!(log("up.log"), "up --authkey=tskey-auth-xyz --hostname=sandbox-agent --accept-dns=false --timeout=120s\n");
    // and --hostname asks for that name
    let r = up("down", &["--hostname", "sandbox-two"], &[]);
    assert!(r.stdout.contains("joined the tailnet as sandbox-two\n") && log("up.log").contains("--hostname=sandbox-two"), "{}{}up.log: {}", r.stdout, r.stderr, log("up.log"));

    // Given a running, logged-out daemon, Then it joins without starting another daemon
    assert_eq!(up("loggedout", &[], &[]).stdout, format!("joined the tailnet as sandbox-agent\n{HINT}"));
    assert_eq!(log("daemon.log"), "");

    // Given a node already joined and online, or a daemon that restarts joined, Then nothing is joined
    assert_eq!(up("joined", &[], &[]).stdout, format!("already on the tailnet as sandbox-agent — nothing to do\n{HINT}"));
    let r = up("down", &[], &[("STATE_AFTER_START", "joined")]);
    assert_eq!((r.exit_code, r.stdout.clone()), (0, format!("started tailscaled (userspace, socks5 on localhost:1055)\nalready on the tailnet as sandbox-agent — nothing to do\n{HINT}")));
    assert_eq!(log("up.log"), "");

    // Given the join failing, Then it fails with tailscale's stderr and never the key
    let r = up("loggedout", &[], &[("UP_FAIL", "backend error: bad key")]);
    assert!(r.exit_code == 1 && r.stderr.contains("tailscale up failed: backend error: bad key") && !r.stderr.contains("tskey-auth-xyz"), "{}", r.stderr);

    // Given no auth key in the vault, Then it fails naming the backend trap
    create_sh_stub(&bin.join("bws"), "printf '%s' '[]'");
    let r = up("loggedout", &[], &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("TS_VM_AUTH_KEY did not resolve from the vault, so there is no way to join."), "{}", r.stderr);
}

#[test]
fn replaced_vm_retire_deletes_every_tagged_vm_record_whatever_its_age_and_spares_the_rest() {
    let a = Admin::new();
    // Given the VM was just destroyed: its records were seen a minute ago (inside the stale window), a sandbox agent and a phone are not the VM's
    let fresh = minutes_ago(1);
    let table = [
        (LIST, MockReply::json(200, json!({ "devices": [
            { "id": "d1", "name": "vm-ops-api-main.tail.ts.net", "lastSeen": fresh, "tags": ["tag:mobile-learning"] },
            { "id": "d2", "name": "ci-runner.tail.ts.net", "lastSeen": fresh, "tags": ["tag:mobile-learning"] },
            { "id": "d3", "name": "sandbox-agent.tail.ts.net", "lastSeen": fresh, "tags": ["tag:mobile-learning"] },
            { "id": "d4", "name": "markus-phone.tail.ts.net", "lastSeen": fresh, "tags": [] },
        ] }))),
        ("DELETE /device/d1", MockReply::json(200, json!({}))),
        ("DELETE /device/d2", MockReply::json(200, json!({}))),
    ];

    // When the command the failed recreate prints (without --replaced-vm, the old one) runs, Then it retires nothing, which is the defect
    a.routes(&table);
    let r = a.wf(&["device", "retire", "--apply"], &[]);
    assert!(r.stdout.contains("Nothing to retire."), "{}", r.stdout);
    assert!(a.deletes().is_empty());

    // When the printed command runs as a dry run, Then it names the records and deletes nothing
    a.routes(&table);
    let r = a.wf(&["device", "retire", "--replaced-vm"], &[]);
    assert!(r.exit_code == 0 && r.stdout.contains("Would retire 2 VM node record(s): vm-ops-api-main, ci-runner") && r.stdout.contains("Dry run"), "{}", r.stdout);
    assert!(a.deletes().is_empty());

    // When it runs with --apply, Then exactly the VM's tagged records are deleted
    let r = a.wf(&["device", "retire", "--replaced-vm", "--apply"], &[]);
    assert!(r.exit_code == 0 && r.stdout.contains("Retired 2 VM node record(s): vm-ops-api-main, ci-runner"), "{}", r.stdout);
    assert_eq!(a.deletes(), ["/device/d1", "/device/d2"]);
}
