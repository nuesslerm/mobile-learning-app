//! `wf secret`: reading the vault (get, list, env, sync-env with its file layering), the sealed local copy, the drift doctor and
//! registry sync, the writers (set, delete, push-notes, create, update, rename, move), the BetterStack telemetry of a lookup, and the
//! guards that keep value-revealing subcommands away from agents. `bws` and `cargo` are stubs, the ingest is a mock HTTPS server.
use crate::support::{create_sh_stub, repo_root, run_wf, MockReply, MockServer, RunOpts, RunResult};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

#[path = "ci_scope.rs"]
mod ci_scope;

const GROQ: &str = "GROQ_API_KEY";
const TAILNET: &str = "TS_VM_AUTH_KEY";
const EDIT: &str = "admin-token";
/// One vault token now reads and writes: the read helpers in these tests use the admin token.
const READ: &str = EDIT;

/// The vault as `bws` serves it: `secret list` answers the listing kept for the token it was given (401 without one), every call
/// is logged with its token, and a write fails with STUB_WRITE_FAIL.
const STUB_BWS: &str = r#"echo "$* | token=$BWS_ACCESS_TOKEN url=$BWS_SERVER_URL" >> "$STUB_DIR/bws.log"
if [ "$1 $2" = "secret list" ]; then
  if [ -n "$STUB_FAIL_ONCE" ] && [ ! -f "$STUB_DIR/failed-once" ]; then touch "$STUB_DIR/failed-once"; echo "Received error message from server: [500 Internal Server Error]" >&2; exit 1; fi
  [ -n "$STUB_VAULT_DOWN" ] && { echo "[503 Service Unavailable]" >&2; exit 1; }
  f="$STUB_DIR/list-$BWS_ACCESS_TOKEN.json"
  if [ -f "$f" ]; then cat "$f"; else echo "[401 Unauthorized]" >&2; exit 1; fi
  exit 0
fi
if [ -n "$STUB_WRITE_FAIL" ]; then echo "$STUB_WRITE_FAIL" >&2; exit 1; fi
exit 0"#;

struct Vault {
    dir: tempfile::TempDir,
    bin: PathBuf,
    cwd: PathBuf,
}

fn registry() -> serde_json::Map<String, Value> {
    serde_json::from_str(&std::fs::read_to_string(repo_root().join("clis/secrets/src/data/registry.json")).unwrap()).unwrap()
}

fn note(key: &str) -> String {
    registry()[key]["description"].as_str().unwrap().to_string()
}

fn groq() -> Value {
    json!({ "id": "1", "key": GROQ, "value": "gsk_abcdefghijkl" })
}

fn tailnet() -> Value {
    json!({ "id": "2", "key": TAILNET, "value": "tskey-12345678" })
}

impl Vault {
    fn new() -> Vault {
        let dir = tempfile::Builder::new().prefix("wf-flow-secrets-").tempdir().unwrap();
        let (bin, cwd) = (dir.path().join("bin"), dir.path().join("cwd"));
        std::fs::create_dir(&bin).unwrap();
        std::fs::create_dir(&cwd).unwrap();
        create_sh_stub(&bin.join("bws"), STUB_BWS);
        Vault { dir, bin, cwd }
    }

    /// What `secret list` answers for `token`.
    fn holds(&self, token: &str, secrets: Value) {
        std::fs::write(self.dir.path().join(format!("list-{token}.json")), secrets.to_string()).unwrap();
    }

    fn wf(&self, args: &[&str], env: &[(&str, &str)], input: Option<&str>) -> RunResult {
        let mut full = vec!["secret"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default()
            .bin_dir(&self.bin)
            .cwd(&self.cwd)
            .clean_env()
            .env("SECRETS_BACKEND", "bitwarden")
            .env("SECRETS_LOG_BETTERSTACK", "false")
            .env("STUB_DIR", &self.dir.path().display().to_string())
            .env("XDG_STATE_HOME", &self.dir.path().join("state").display().to_string());
        if let Some(text) = input {
            opts = opts.input(text);
        }
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    /// With the read token (and `extra`), the call log forgotten first.
    fn read(&self, args: &[&str], extra: &[(&str, &str)]) -> RunResult {
        self.forget();
        let mut env = vec![("BWS_ADMIN_ACCESS_TOKEN", READ)];
        env.extend_from_slice(extra);
        self.wf(args, &env, None)
    }

    fn forget(&self) {
        let _ = std::fs::remove_file(self.dir.path().join("bws.log"));
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("bws.log")).map(|t| t.lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
    }

    fn writes(&self) -> Vec<String> {
        self.calls().into_iter().filter(|c| !c.starts_with("secret list")).collect()
    }

    fn history_path(&self) -> PathBuf {
        self.dir.path().join("state/wf-secrets/rotation-history.jsonl")
    }

    fn history(&self) -> Vec<Value> {
        std::fs::read_to_string(self.history_path()).map(|t| t.trim().lines().map(|l| serde_json::from_str(l).unwrap()).collect()).unwrap_or_default()
    }
}

fn listed(token: &str) -> String {
    format!("secret list --output json | token={token} url=")
}

#[test]
fn given_the_vault_answers_500_once_when_a_secret_is_read_then_the_listing_is_retried_and_answers() {
    // Given a vault that fails the first listing with a 500 and holds the key
    let v = Vault::new();
    v.holds(READ, json!([groq()]));

    // When the key is read
    let got = v.read(&["get", GROQ], &[("STUB_FAIL_ONCE", "1")]);

    // Then the value is printed after a second listing
    assert_eq!((got.exit_code, got.stdout.as_str()), (0, "gsk_abcdefghijkl\n"), "{}", got.stderr);
    assert_eq!(v.calls(), [listed(READ), listed(READ)]);
}

#[test]
fn a_developer_reads_secrets_from_the_vault_lists_them_masked_and_writes_env_files() {
    let v = Vault::new();
    v.holds(READ, json!([groq()]));

    // When a key in the vault, an unregistered key and a key absent from the vault are read
    let got = v.read(&["get", GROQ], &[]);
    let got_calls = v.calls();
    let unknown = v.read(&["get", "NOT_A_KEY"], &[]);
    let unknown_calls = v.calls();
    v.holds(READ, json!([tailnet()]));
    let absent = v.read(&["get", GROQ], &[]);
    // Then the raw value is printed after one listing with the read token, the unknown key names the valid ones without a vault
    // call, and the absent key is not found
    assert_eq!((got.exit_code, got.stdout.as_str()), (0, "gsk_abcdefghijkl\n"));
    assert_eq!(got_calls, [listed(READ)]);
    assert_eq!(unknown.exit_code, 1);
    assert!(unknown.stderr.contains("Unknown key: NOT_A_KEY\nValid keys: ") && unknown.stderr.contains(GROQ), "{}", unknown.stderr);
    assert!(unknown_calls.is_empty());
    assert_eq!((absent.exit_code, absent.stdout.as_str()), (1, ""));
    assert!(absent.stderr.contains("GROQ_API_KEY: not found"), "{}", absent.stderr);

    // When the token is rejected, when the EU server is named, 
    v.holds(READ, json!([groq()]));
    let rejected = v.read(&["get", GROQ], &[("BWS_ADMIN_ACCESS_TOKEN", "bad")]);
    v.read(&["get", GROQ], &[("BWS_SERVER_URL", "https://vault.bitwarden.eu")]);
    let eu_calls = v.calls();
    // Then the 401 comes with the EU hint, bws gets the server URL, and the one token answers
    assert_eq!(rejected.exit_code, 1);
    assert!(rejected.stderr.contains("Bitwarden Secrets Manager list failed (read token)") && rejected.stderr.contains("401 Unauthorized") && rejected.stderr.contains("set BWS_SERVER_URL=https://vault.bitwarden.eu"), "{}", rejected.stderr);
    assert_eq!(eu_calls[0], "secret list --output json | token=admin-token url=https://vault.bitwarden.eu");

    // When everything is listed and dumped as env lines
    let list = v.read(&["list"], &[]);
    v.holds(READ, json!([groq(), tailnet()]));
    let env = v.read(&["env"], &[]);
    let one = v.read(&["env", "--keys", GROQ], &[]);
    let none = v.read(&["env", "--keys", "NOT_A_REGISTERED_KEY"], &[]);
    // Then the list masks the value and marks the rest missing, and env prints the asked-for keys and the bootstrap token
    assert_eq!(list.exit_code, 0);
    let lines: Vec<&str> = list.stdout.split('\n').collect();
    assert!(Regex::new(r"^KEY\s+ {2}SOURCE {4}VALUE$").unwrap().is_match(lines[0]) && Regex::new(r"^-+ {2}-{8} {2}-{5}$").unwrap().is_match(lines[1]), "{}", list.stdout);
    assert!(Regex::new(r" {2}env {5} {2}gsk•{10}jkl$").unwrap().is_match(lines.iter().find(|l| l.starts_with("GROQ_API_KEY ")).unwrap()));
    assert!(Regex::new(r"missing {3}\(missing\)$").unwrap().is_match(lines.iter().find(|l| l.starts_with("ADB_ADMIN_PASSWORD ")).unwrap()));
    assert_eq!(lines.iter().filter(|l| l.contains("(missing)")).count(), registry().len() - 2);
    assert_eq!(env.stdout, "TS_VM_AUTH_KEY=\"tskey-12345678\"\nGROQ_API_KEY=\"gsk_abcdefghijkl\"\nBWS_ADMIN_ACCESS_TOKEN=\"admin-token\"\n");
    assert_eq!(one.stdout, "GROQ_API_KEY=\"gsk_abcdefghijkl\"\n");
    assert_eq!((none.exit_code, none.stdout.as_str()), (0, ""));

    // When the env file of an app whose required keys come from the groups file is written, previewed, revealed, and asked for with keys the vault lacks
    let groups: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(repo_root().join("clis/secrets/src/secrets/groups.json")).unwrap()).unwrap();
    let group = groups.iter().find(|g| !g["required"].as_array().unwrap().is_empty()).unwrap();
    let app = group["app"].as_str().unwrap();
    let required: Vec<&str> = group["required"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
    let value_of = |i: usize, key: &str| if i == 0 { "tskey-12345678".to_string() } else { format!("value-of-{key}") };
    let held: Vec<Value> = required.iter().enumerate().map(|(i, key)| json!({ "id": format!("k{i}"), "key": key, "value": value_of(i, key) })).collect();
    v.holds(READ, json!(held));
    let out = v.dir.path().join("out.env");
    let written = v.read(&["sync-env", "--for", app, "--out", &out.display().to_string()], &[]);
    let preview = v.read(&["sync-env", "--for", app], &[]);
    let revealed = v.read(&["sync-env", "--for", app, "--reveal"], &[]);
    v.holds(READ, json!([groq()]));
    let partial_out = v.dir.path().join("partial.env");
    let partial = v.read(&["sync-env", "--for", app, "--out", &partial_out.display().to_string()], &[]);
    // Then the file holds each required key quoted, the preview is redacted, --reveal prints them, and a partial file is refused naming what is missing
    let expected: std::collections::BTreeSet<String> = required.iter().enumerate().map(|(i, key)| format!("{key}=\"{}\"", value_of(i, key))).collect();
    let lines_of = |text: &str| -> std::collections::BTreeSet<String> { text.lines().map(String::from).collect() };
    assert_eq!((written.exit_code, written.stdout.clone()), (0, format!("Wrote {} secret(s) to {}\n", required.len(), out.display())));
    assert_eq!(lines_of(&std::fs::read_to_string(&out).unwrap()), expected);
    assert!(lines_of(&preview.stdout).contains(&format!("{}=\"tsk••••••••678\"", required[0])), "{}", preview.stdout);
    let shown = format!("{}{}", preview.stdout, preview.stderr);
    assert!(preview.stderr.contains("REDACTED preview") && !shown.contains("tskey-12345678") && required.iter().skip(1).all(|k| !shown.contains(&format!("value-of-{k}"))));
    assert_eq!(lines_of(&revealed.stdout), expected);
    assert_eq!(partial.exit_code, 1);
    assert!(required.iter().all(|k| partial.stderr.contains(k)) && partial.stderr.contains("Refusing to write a partial .env."), "{}", partial.stderr);
    assert!(!partial_out.exists());

    // When the vault holds a placeholder (with a value exported in the shell beside it), or the key is not deploy-scoped
    v.holds(READ, json!([{ "id": "2", "key": TAILNET, "value": "PLACEHOLDER_FILL_ME" }]));
    let placeholder = v.read(&["sync-env", TAILNET, "--out", &v.dir.path().join("o.env").display().to_string()], &[]);
    let shell_out = v.dir.path().join("a.env");
    let shell = v.read(&["sync-env", TAILNET, "--out", &shell_out.display().to_string()], &[(TAILNET, "ambient-value-1")]);
    let shell_calls = v.calls();
    let scoped = v.read(&["sync-env", "TS_CI_OAUTH_CLIENT_ID", "--reveal"], &[]);
    // Then the placeholder counts as missing, the shell value is ignored (the vault was asked, nothing written), and the other scope is left out
    assert_eq!(placeholder.exit_code, 1);
    assert!(placeholder.stderr.contains("1 secret(s) exist in the vault but still hold a PLACEHOLDER") && placeholder.stderr.contains("could not resolve required secret(s): TS_VM_AUTH_KEY"), "{}", placeholder.stderr);
    assert_eq!(shell.exit_code, 1);
    assert!(!shell_out.exists() && !shell.stderr.contains("AMBIENT") && shell_calls == [listed(READ)], "{}", shell.stderr);
    assert_eq!((scoped.exit_code, scoped.stdout.as_str()), (0, "\n"));
    assert!(scoped.stderr.contains("omitted 1 non-deploy-scoped secret(s) from the .env: TS_CI_OAUTH_CLIENT_ID") && v.calls().is_empty(), "{}", scoped.stderr);

    // When a .env.defaults base sits in the directory
    v.holds(READ, json!([tailnet()]));
    std::fs::write(v.cwd.join(".env.defaults"), "# base\nA=1\nexport B=\"two words\" # trailing\nTS_VM_AUTH_KEY=old\nC='single'\n").unwrap();
    let merged = v.read(&["sync-env", TAILNET, "--reveal"], &[]);
    std::fs::remove_file(v.cwd.join(".env.defaults")).unwrap();
    // Then it is merged under the secrets and re-quoted, and the collision is warned
    assert_eq!(merged.stdout, "A=\"1\"\nB=\"two words\"\nTS_VM_AUTH_KEY=\"tskey-12345678\"\nC=\"single\"\n");
    assert!(merged.stderr.contains("key(s) in .env.defaults overridden by resolved secrets") && merged.stderr.contains(TAILNET), "{}", merged.stderr);

    // When sync-env is given nothing to do, or an unknown group
    let usage = v.read(&["sync-env"], &[]);
    let group = v.read(&["sync-env", "--for", "nope"], &[]);
    // Then it says how to call it, or lists the known groups
    assert_eq!((usage.exit_code, usage.stderr.as_str()), (1, "Pass one or more KEY args or --for <app>\n"));
    assert_eq!(group.exit_code, 1);
    assert!(group.stderr.contains("Unknown --for group: nope\nKnown groups: ") && group.stderr.contains("reporting-service"), "{}", group.stderr);

    // When the value also lives in the shell and in env files: a stray .env, a repository's .env.local and .env, a user config
    let emit = |extra: &[(&str, &str)]| v.read(&["sync-env", TAILNET, "--reveal"], extra);
    std::fs::write(v.cwd.join(".env"), "TS_VM_AUTH_KEY=stale\n").unwrap();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(&v.cwd).status().unwrap().success());
    std::fs::write(v.cwd.join(".env"), "TS_VM_AUTH_KEY=from-env\n").unwrap();
    std::fs::write(v.cwd.join(".env.local"), "TS_VM_AUTH_KEY=\"from-local\"\n").unwrap();
    let home = v.dir.path().join("home");
    std::fs::create_dir_all(home.join(".config/wf")).unwrap();
    std::fs::write(home.join(".config/wf/.env"), "TS_VM_AUTH_KEY=from-user\n").unwrap();
    let home = home.display().to_string();
    let layered = emit(&[("HOME", &home), (TAILNET, "from-shell")]);
    // Then none of them is read: the vault's value is written and nothing is called out as ambient
    assert_eq!((layered.exit_code, layered.stdout.as_str()), (0, "TS_VM_AUTH_KEY=\"tskey-12345678\"\n"), "{}", layered.stderr);
    assert!(!layered.stderr.contains("AMBIENT"), "{}", layered.stderr);

    // When no vault token is configured at all (a deployed host), the environment is the source and is called out
    let deployed_out = v.dir.path().join("d.env");
    let deployed = v.wf(&["sync-env", TAILNET, "--out", &deployed_out.display().to_string()], &[(TAILNET, "deployed-value-1")], None);
    // Then the environment's value is written, with the warning
    assert_eq!(std::fs::read_to_string(&deployed_out).unwrap(), "TS_VM_AUTH_KEY=\"deployed-value-1\"\n");
    assert!(deployed.stderr.contains("took their value from the AMBIENT ENVIRONMENT, not the vault: TS_VM_AUTH_KEY"), "{}", deployed.stderr);
}

#[test]
fn every_app_group_gets_its_required_then_its_optional_keys_in_order() {
    // Given every registered key in the vault (the OTLP host left out: wf's own logger refuses a made-up host)
    let source = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/secrets/groups.json")).unwrap();
    let groups: Vec<(String, Value)> = serde_json::from_str::<Vec<Value>>(&source).unwrap().into_iter().map(|g| (g["app"].as_str().unwrap().to_string(), g)).collect();
    let entries = registry();
    let skipped = "BETTERSTACK_OTLP_HOST";
    let all: Vec<(String, String)> = entries.values().map(|e| e["envVar"].as_str().unwrap().to_string()).filter(|v| v != skipped).map(|v| (v.clone(), format!("value-of-{v}"))).collect();
    let deploy_scoped = |key: &str| entries[key].get("vault").and_then(Value::as_str).is_none_or(|v| v == "deploy");
    let v = Vault::new();
    let vault: Vec<Value> = entries.iter().filter(|(_, e)| e["envVar"].as_str().is_some_and(|v| v != skipped && !v.starts_with("BWS_"))).map(|(_, e)| { let key = e.get("bitwardenKey").and_then(Value::as_str).unwrap_or(e["envVar"].as_str().unwrap()); json!({ "id": key, "key": key, "value": format!("value-of-{}", e["envVar"].as_str().unwrap()) }) }).collect();
    v.holds(READ, Value::Array(vault));
    v.holds("value-of-BWS_ADMIN_ACCESS_TOKEN", json!([]));
    let mut env: Vec<(&str, &str)> = all.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    env.push(("BWS_ADMIN_ACCESS_TOKEN", READ));
    for (app, group) in &groups {
        // When the app's env file is revealed
        let r = v.wf(&["sync-env", "--for", app, "--reveal"], &env, None);
        // Then it lists the group's required keys, then its optional ones, deploy-scoped only
        let field = |name: &str| group.get(name).and_then(Value::as_array).cloned().unwrap_or_default();
        let expected: Vec<String> = field("required").into_iter().chain(field("optional")).map(|k| k.as_str().unwrap().to_string()).filter(|k| deploy_scoped(k)).map(|k| entries[&k]["envVar"].as_str().unwrap().to_string()).filter(|v| v != skipped).collect();
        assert_eq!(r.exit_code, 0, "{app}: {}", r.stderr);
        assert_eq!(r.stdout.lines().filter(|l| !l.is_empty()).map(|l| l.split('=').next().unwrap()).collect::<Vec<_>>(), expected, "{app}");
    }
}

#[test]
fn the_retired_metro_runner_has_no_key_group_and_no_group_carries_a_key_the_registry_says_ships_to_none() {
    // Given the key groups, and the registry entries that say they are orphaned or in no deploy group
    let source = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/secrets/groups.json")).unwrap();
    let groups: Vec<Value> = serde_json::from_str(&source).unwrap();
    let carried: BTreeSet<&str> = groups.iter().flat_map(|g| ["required", "optional"].into_iter().flat_map(|f| g[f].as_array().into_iter().flatten().filter_map(Value::as_str))).collect();
    let ships_to_none = Regex::new(r"(?i)^ORPHANED|in NO `sync-env --for`").unwrap();
    let entries = registry();

    // When sync-env is asked for the retired group, and each group is checked for such keys
    let v = Vault::new();
    v.holds(READ, json!([]));
    let retired = v.read(&["sync-env", "--for", "metro-runner"], &[]);
    let strays: Vec<&str> = entries.iter().filter(|(_, e)| ships_to_none.is_match(e["description"].as_str().unwrap())).map(|(k, _)| k.as_str()).filter(|k| carried.contains(k)).collect();

    // Then wf calls it unknown and lists no such group, no group carries one, and the keys that rode with it say they ship to none
    let (unknown, known) = retired.stderr.split_once("Known groups: ").unwrap_or((retired.stderr.as_str(), ""));
    assert!(retired.exit_code == 1 && unknown.contains("Unknown --for group: metro-runner") && !known.contains("metro-runner"), "{}", retired.stderr);
    assert!(strays.is_empty(), "{strays:?}");
    assert!(["PNPM_CACHE_PAR_URL", "EXPO_TOKEN"].iter().all(|k| ships_to_none.is_match(&note(k))));
}

#[test]
fn a_key_created_after_the_sealed_copy_was_taken_is_found_by_one_live_look() {
    // Given a fresh sealed copy of a vault that did not yet hold the tailnet key
    let v = Vault::new();
    v.holds(READ, json!([groq()]));
    let cache = v.dir.path().join("cache").display().to_string();
    let get = |key: &str| v.wf(&["get", key], &[("BWS_ADMIN_ACCESS_TOKEN", READ), ("WF_SECRETS_CACHE", "use"), ("WF_CACHE_DIR", cache.as_str())], None);
    assert_eq!(get(GROQ).exit_code, 0);
    // When the key is created in the vault and then read
    v.holds(READ, json!([groq(), tailnet()]));
    let found = get(TAILNET);
    // Then the copy does not make it "not found": one more live listing finds it
    assert_eq!(found.exit_code, 0, "{}", found.stderr);
    assert_eq!(v.calls().len(), 2);
}

#[test]
fn the_sealed_local_copy_saves_vault_calls_and_stands_in_only_where_it_is_safe() {
    let v = Vault::new();
    v.holds(READ, json!([groq(), tailnet()]));
    let cache = v.dir.path().join("cache").display().to_string();
    let get = |extra: &[(&str, &str)]| {
        let mut env = vec![("WF_SECRETS_CACHE", "use"), ("WF_CACHE_DIR", cache.as_str())];
        env.extend_from_slice(extra);
        v.wf(&["get", GROQ], &[&[("BWS_ADMIN_ACCESS_TOKEN", READ)][..], &env].concat(), None)
    };

    // When the same key is read twice
    let first = get(&[]);
    let second = get(&[]);
    // Then the second answer comes from the copy, which is sealed and owner-only
    assert_eq!((first.stdout.as_str(), second.stdout.as_str()), ("gsk_abcdefghijkl\n", "gsk_abcdefghijkl\n"));
    assert_eq!(v.calls().len(), 1, "the second call came from the copy");
    let copy = std::fs::read_dir(&cache).unwrap().flatten().next().expect("a copy was written").path();
    let bytes = std::fs::read(&copy).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("gsk_abcdefghijkl"), "the secret is not in the file in clear");
    assert_eq!(std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777, 0o600);

    // When the copy is older than its TTL, tampered with, or read against another server
    v.forget();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    get(&[("WF_SECRETS_CACHE_TTL_SEC", "0")]);
    let stale_calls = v.calls().len();
    let mut sealed = std::fs::read(&copy).unwrap();
    *sealed.last_mut().unwrap() ^= 1;
    std::fs::write(&copy, &sealed).unwrap();
    v.forget();
    let tampered = get(&[]);
    let tampered_calls = v.calls().len();
    v.forget();
    get(&[("BWS_SERVER_URL", "https://vault.example")]);
    for other in std::fs::read_dir(&cache).unwrap().flatten().map(|e| e.path()).filter(|p| *p != copy) {
        std::fs::remove_file(other).unwrap();
    }
    // Then each lists the vault again instead of trusting the copy
    assert_eq!((stale_calls, tampered_calls, v.calls().len()), (1, 1, 1));
    assert_eq!(tampered.stdout, "gsk_abcdefghijkl\n");

    // When the vault goes down with a warm copy of the same token, and a rotated token meets the old copy
    let down = get(&[("WF_SECRETS_CACHE", "refresh"), ("STUB_VAULT_DOWN", "1")]);
    let rotated = get(&[("BWS_ADMIN_ACCESS_TOKEN", "bad")]);
    // Then the copy stands in with a warning, but a copy sealed under another token never opens
    assert_eq!((down.exit_code, down.stdout.as_str()), (0, "gsk_abcdefghijkl\n"), "{}", down.stderr);
    assert!(down.stderr.contains("using the vault copy from"), "{}", down.stderr);
    assert!(rotated.exit_code != 0 && rotated.stdout.is_empty() && rotated.stderr.contains("Bitwarden"), "{}", rotated.stderr);

    // When the copy is switched off, sync-env runs, and the copy is cleared
    v.forget();
    get(&[("WF_SECRETS_CACHE", "off")]);
    v.wf(&["sync-env", TAILNET, "--reveal"], &[("WF_SECRETS_CACHE", "use"), ("WF_CACHE_DIR", cache.as_str()), ("BWS_ADMIN_ACCESS_TOKEN", READ)], None);
    let cleared = v.wf(&["cache", "clear"], &[("WF_SECRETS_CACHE", "use"), ("WF_CACHE_DIR", cache.as_str()), ("BWS_ADMIN_ACCESS_TOKEN", READ)], None);
    get(&[]);
    // Then each of those lists the vault: off, sync-env's refresh, and the first read after the clear
    assert_eq!(v.calls().len(), 3);
    assert!(cleared.stdout.contains("Removed 1"), "{}", cleared.stdout);
}

#[test]
fn the_doctor_reports_drift_between_the_registry_and_the_vault_and_registry_sync_rebuilds_wf_first() {
    let v = Vault::new();
    let entries = registry().len();

    // When the doctor runs without any credential, with a stray secret, with a look-alike name, and with a rejected token
    let skipped = v.wf(&["doctor", "--if-available"], &[], None);
    v.holds(READ, json!([groq(), { "id": "9", "key": "STRAY_SECRET_NAME", "value": "x" }]));
    let stray = v.read(&["doctor"], &[]);
    v.holds(READ, json!([{ "id": "8", "key": "GROQ_SOURCE_TOKEN", "value": "x" }]));
    let similar = v.read(&["doctor"], &[]);
    let rejected = v.read(&["doctor", "--if-available"], &[("BWS_ADMIN_ACCESS_TOKEN", "bad")]);
    // Then it skips without a credential, lists the unregistered and the scopes it could not list, flags the likely rename, and fails
    // when no credential can list anything
    assert_eq!((skipped.exit_code, skipped.stdout.as_str()), (0, "secrets doctor SKIPPED: no Bitwarden scope credential in this environment, so registry/vault drift was not checked.\n"));
    assert_eq!(stray.exit_code, 1);
    assert_eq!(stray.stdout.lines().next().unwrap(), format!("registry: {entries} entr(ies)   vault: 2 secret(s)"));
    assert!(stray.stdout.contains("UNREGISTERED — in the vault, reachable by nothing (1):\n  STRAY_SECRET_NAME\n") && stray.stdout.contains("PARTIAL INVENTORY — no credential could list 1 scope(s): gh-ci"), "{}", stray.stdout);
    assert_eq!(similar.exit_code, 1);
    assert!(similar.stdout.contains("NAME MISMATCH (likely)") && similar.stdout.contains("GROQ_API_KEY: wants \"GROQ_API_KEY\", vault has \"GROQ_SOURCE_TOKEN\""), "{}", similar.stdout);
    assert_eq!(rejected.exit_code, 1);
    assert!(rejected.stderr.contains("No Bitwarden credential could list any scope"), "{}", rejected.stderr);

    // Given a repository whose cargo and rebuilt wf are stubs
    let repo = v.dir.path().join("repo");
    std::fs::create_dir_all(repo.join("clis/target/release")).unwrap();
    assert!(Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(&repo).status().unwrap().success());
    std::fs::write(repo.join("clis/Cargo.toml"), "[workspace]\n").unwrap();
    let log = v.dir.path().join("sync.log");
    create_sh_stub(&v.bin.join("cargo"), "echo \"cargo $*\" >> \"$LOG\"; exit \"${CARGO_EXIT:-0}\"");
    create_sh_stub(&repo.join("clis/target/release/wf"), "echo \"wf $*\" >> \"$LOG\"; exit \"${DRIFT_EXIT:-0}\"");
    let sync = |env: &[(&str, &str)]| {
        let _ = std::fs::remove_file(&log);
        let mut opts = RunOpts::default().bin_dir(&v.bin).cwd(&repo).clean_env().env("LOG", &log.display().to_string());
        for (k, val) in env {
            opts = opts.env(k, val);
        }
        let r = run_wf(&["secret", "registry", "sync"], &opts);
        (r.exit_code, std::fs::read_to_string(&log).map(|t| t.trim().lines().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default())
    };
    let top = String::from_utf8_lossy(&Command::new("git").args(["rev-parse", "--show-toplevel"]).current_dir(&repo).output().unwrap().stdout).trim().to_string();
    // When the registry is synced, when the build fails, and when the rebuilt binary finds drift
    let (ok, ok_calls) = sync(&[]);
    let (failed, failed_calls) = sync(&[("CARGO_EXIT", "101")]);
    let (drift, drift_calls) = sync(&[("DRIFT_EXIT", "1")]);
    // Then wf is rebuilt and the drift check runs with the rebuilt binary; a failed build stops there with its exit code
    assert_eq!((ok, ok_calls), (0, vec![format!("cargo build --release -p wf --manifest-path {top}/clis/Cargo.toml"), "wf secret doctor --backend bitwarden --if-available".to_string()]));
    assert_eq!((failed, failed_calls.len()), (101, 1));
    assert_eq!((drift, drift_calls.len()), (1, 2));
}

#[test]
fn the_writers_preview_by_default_and_apply_through_the_edit_token_keeping_the_old_value() {
    let v = Vault::new();
    let both = [("BWS_ADMIN_ACCESS_TOKEN", READ), ("BWS_ADMIN_ACCESS_TOKEN", EDIT)];
    let write = |args: &[&str], extra: &[(&str, &str)], input: Option<&str>| {
        v.forget();
        v.wf(args, &[&both[..], extra].concat(), input)
    };
    let chars = |text: &str| text.chars().count();
    let total = registry().len();

    // Given one note out of date and one current
    v.holds(READ, json!([{ "id": "id-1", "key": GROQ, "note": "old" }, { "id": "id-2", "key": TAILNET, "note": note(TAILNET) }]));
    // When push-notes runs dry, with --apply, without an edit token, without any token, and against a vault that refuses the write
    let dry = write(&["push-notes"], &[], None);
    let dry_calls = v.calls();
    let applied = write(&["push-notes", "--apply"], &[], None);
    let applied_writes = v.writes();
    v.forget();
    let no_token = v.wf(&["push-notes"], &[], None);
    let no_token_calls = v.calls();
    let refused = write(&["push-notes", "--apply"], &[("STUB_WRITE_FAIL", "[403 Forbidden]")], None);
    // Then the dry run plans from the read token and writes nothing, --apply writes the changed note with the edit token, and each
    // missing credential or refusal is named
    assert_eq!(dry.exit_code, 0);
    assert_eq!(
        dry.stdout,
        [format!("would update  {GROQ}  (3 -> {} chars)", chars(&note(GROQ))), String::new(), format!("Would write 1 note(s); 1 already current; {} not in the vault.", total - 2), "Re-run with --verbose to list the keys absent from the vault.".into(), "Dry run — re-run with --apply to write.".into(), String::new()].join("\n")
    );
    assert_eq!(dry_calls, [listed(READ)]);
    assert!(applied.stdout.contains(&format!("updated  {GROQ}  (3 -> {} chars)", chars(&note(GROQ)))) && applied.stdout.contains(&format!("Wrote 1 note(s); 1 already current; {} not in the vault.", total - 2)), "{}", applied.stdout);
    assert_eq!(applied_writes, [format!("secret edit --note {} id-1 --output none | token={EDIT} url=", note(GROQ))]);
    assert_eq!((no_token.exit_code, no_token.stderr.as_str()), (1, "No Bitwarden token available. Set BWS_ADMIN_ACCESS_TOKEN to preview and apply note changes.\n"));
    assert!(no_token_calls.is_empty());
    assert_eq!((refused.exit_code, refused.stderr.as_str()), (1, "Bitwarden secret edit id-1 failed: [403 Forbidden] The token authenticated but was refused — grant its machine account 'Can read, write' on the project.\n"));
    // and --verbose lists every key absent from the vault
    v.holds(READ, json!([{ "id": "id-1", "key": GROQ, "note": note(GROQ) }]));
    let verbose = write(&["push-notes", "--verbose"], &[], None);
    let skipped: Vec<&str> = verbose.stdout.lines().filter(|l| l.starts_with("skipped       ")).collect();
    assert_eq!(skipped.len(), total - 1);
    assert!(Regex::new(r"^skipped {7}\S+ {2}\(registered here, not in the vault\)$").unwrap().is_match(skipped[0]) && !verbose.stdout.contains("Re-run with --verbose"));

    // When a new key is created: previewed, applied with a value that starts like an option, as a placeholder, from stdin, from another secret
    v.holds(READ, json!([{ "id": "id-9", "key": "OTHER", "projectId": "p" }]));
    let preview = write(&["create", GROQ, "proj-1", "--value", "v"], &[], None);
    let preview_writes = v.writes();
    v.holds(READ, json!([{ "id": "id-2", "key": TAILNET, "value": "tskey-copied" }]));
    let created = write(&["create", GROQ, "proj-1", "--value", "-----BEGIN KEY", "--apply"], &[], None);
    let created_writes = v.writes();
    write(&["create", GROQ, "proj-1", "--placeholder", "--apply"], &[], None);
    let placeholder_writes = v.writes();
    write(&["create", GROQ, "proj-1", "--value-stdin", "--apply"], &[], Some("sekret\n"));
    let stdin_writes = v.writes();
    let empty_stdin = write(&["create", GROQ, "proj-1", "--value-stdin", "--apply"], &[], Some("  \n"));
    let copied = write(&["create", GROQ, "proj-1", "--value-from", TAILNET, "--apply"], &[], None);
    let copied_writes = v.writes();
    let from_nothing = write(&["create", GROQ, "proj-1", "--value-from", "GROQ_API_KEY", "--apply"], &[], None);
    // Then the preview writes nothing, the write puts the note options before `--`, and each value source lands as the value
    assert_eq!(preview.stdout, format!("would create  {GROQ}  in project proj-1  (note: {} chars)\nDry run — re-run with --apply to write.\n", chars(&note(GROQ))));
    assert!(preview_writes.is_empty());
    assert_eq!(created.stdout, format!("created  {GROQ}  in project proj-1  (note: {} chars)\n", chars(&note(GROQ))));
    assert_eq!(created_writes, [format!("secret create --note {} --output none -- {GROQ} -----BEGIN KEY proj-1 | token={EDIT} url=", note(GROQ))]);
    assert!(placeholder_writes[0].contains(&format!("-- {GROQ} PLACEHOLDER_FILL_ME proj-1")));
    assert!(stdin_writes[0].contains(&format!("-- {GROQ} sekret proj-1")));
    assert_eq!((empty_stdin.exit_code, empty_stdin.stderr.clone()), (1, format!("{GROQ}: stdin closed with no value provided\n")));
    assert!(copied.exit_code == 0 && copied_writes[0].contains(&format!("-- {GROQ} tskey-copied proj-1")) && !copied.stdout.contains("tskey-copied"), "{}", copied.stdout);
    assert_eq!((from_nothing.exit_code, from_nothing.stderr.clone()), (1, format!("--value-from {GROQ} resolved to nothing; refusing to create an empty secret\n")));
    // and create refuses a duplicate, an unregistered key, no or two value sources, and --apply without an edit token
    v.holds(READ, json!([{ "id": "id-1", "key": GROQ }]));
    let duplicate = write(&["create", GROQ, "proj-1", "--value", "v", "--apply"], &[], None);
    let unregistered = write(&["create", "NOT_REGISTERED", "proj-1", "--value", "v"], &[], None);
    let sources: Vec<RunResult> = [&[][..], &["--value", "a", "--placeholder"][..]].iter().map(|extra| write(&[&["create", GROQ, "proj-1"][..], *extra].concat(), &[], None)).collect();
    v.holds(READ, json!([]));
    assert_eq!((duplicate.exit_code, duplicate.stderr.clone()), (1, format!("Refused: a secret named {GROQ} already exists in the vault — refusing to create a duplicate\n")));
    assert_eq!((unregistered.exit_code, unregistered.stderr.as_str()), (1, "Refused: NOT_REGISTERED is not in registry.json, so it has no reviewed description to use as its note — add the registry entry first\n"));
    for r in &sources {
        assert_eq!((r.exit_code, r.stderr.as_str()), (1, "Pass exactly one of --value, --value-stdin, --value-from <KEY>, or --placeholder\n"));
    }

    // When a value is rotated: previewed (also from stdin), applied, applied to a key the vault lacks, and refused by the vault
    let rotate_preview = write(&["update", GROQ, "--value", "newvalue123"], &[], None);
    let rotate_calls = v.calls();
    let stdin_preview = write(&["update", GROQ, "--value-stdin"], &[], Some("abcdefghij\n"));
    v.holds(READ, json!([{ "id": "id-1", "key": GROQ, "value": "oldvalue" }]));
    v.holds(EDIT, json!([{ "id": "id-1", "key": GROQ, "value": "oldvalue" }]));
    let rotated = write(&["update", GROQ, "--value", "newvalue123", "--apply"], &[], None);
    let rotated_writes = v.writes();
    let refused_rotation = write(&["update", GROQ, "--value", "v", "--apply"], &[("STUB_WRITE_FAIL", "[404 Not Found]")], None);
    // Then the preview masks and counts, the write archives the old value owner-only before editing by id, and the 404 carries its hint
    assert_eq!(rotate_preview.stdout, format!("would update  {GROQ}  to a 11-char value (new•••••123)\nDry run — re-run with --apply to write.\n"));
    assert!(rotate_calls.is_empty());
    assert!(stdin_preview.stdout.contains("to a 10-char value (abc••••hij)"), "{}", stdin_preview.stdout);
    assert_eq!(rotated.stdout, format!("updated  {GROQ}  (11 chars, new•••••123)\nprevious value archived to {} (owner-only file, value never printed — for emergency recovery only)\n", v.history_path().display()));
    assert_eq!(rotated_writes, [format!("secret edit --value newvalue123 --output none -- id-1 | token={EDIT} url=")]);
    let entry = &v.history()[0];
    assert_eq!((entry["key"].as_str(), entry["backend"].as_str(), entry["previousValue"].as_str()), (Some(GROQ), Some("bitwarden"), Some("oldvalue")));
    assert_eq!(entry.as_object().unwrap().keys().collect::<Vec<_>>(), ["timestamp", "key", "backend", "previousValue"]);
    assert_eq!(std::fs::metadata(v.history_path()).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(v.dir.path().join("state/wf-secrets")).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!((refused_rotation.exit_code, refused_rotation.stdout.as_str()), (1, ""));
    assert!(refused_rotation.stderr.contains("Bitwarden secret edit id-1 failed: [404 Not Found]") && refused_rotation.stderr.contains("A 403/404 on a write usually means"), "{}", refused_rotation.stderr);
    // and a rotation never invents a key, and wrong value options or an unknown key are explained
    v.holds(READ, json!([]));
    v.holds(EDIT, json!([]));
    let invented = write(&["update", GROQ, "--value", "v", "--apply"], &[], None);
    assert_eq!((invented.exit_code, invented.stderr.clone()), (1, format!("{GROQ} resolves to vault key {GROQ}, which does not exist — refusing to rotate a secret that is not there. Create it first (`wf secret create`), so a rotation can never silently invent one.\n")));
    assert!(v.writes().is_empty());
    assert_eq!(write(&["update", GROQ, "--value", "a", "--value-stdin"], &[], None).stderr, "Pass exactly one of --value or --value-stdin\n");
    assert_eq!(write(&["update", GROQ], &[], None).stderr, "Pass exactly one of --value or --value-stdin\n");
    let unknown = write(&["update", "NOT_A_KEY", "--value", "a"], &[], None);
    assert!(unknown.exit_code == 1 && unknown.stderr.contains("Unknown key: NOT_A_KEY\nValid keys: "), "{}", unknown.stderr);

    // When a secret is renamed: previewed, in place, additively, from a missing source, onto an existing name, additively off-registry
    v.holds(READ, json!([{ "id": "id-1", "key": "OLD_NAME", "projectId": "p" }]));
    let rename_preview = write(&["rename", "OLD_NAME", "NEW_NAME"], &[], None);
    let rename_preview_writes = v.writes();
    let renamed = write(&["rename", "OLD_NAME", "NEW_NAME", "--apply"], &[], None);
    let renamed_writes = v.writes();
    v.holds(READ, json!([{ "id": "id-2", "key": TAILNET, "value": "tskey-copied", "projectId": "proj-7" }]));
    let additive = write(&["rename", TAILNET, GROQ, "--additive", "--apply"], &[], None);
    let additive_writes = v.writes();
    v.holds(READ, json!([{ "id": "id-1", "key": "A" }, { "id": "id-2", "key": "B" }]));
    let missing = write(&["rename", "NOPE", "C"], &[], None);
    let exists = write(&["rename", "A", "B"], &[], None);
    let off_registry = write(&["rename", "A", "C", "--additive", "--apply"], &[], None);
    // Then the preview warns the in-place rename is a hard cutover, the rename keeps the id, the additive copy keeps the project
    assert_eq!(
        rename_preview.stdout,
        ["would rename  OLD_NAME -> NEW_NAME  (id preserved)", "NOTE: an in-place rename is a hard cutover. `sync-env` resolves a secret BY KEY, so if any", "      running deploy still references the old name, it breaks until the code rename lands.", "      Use --additive for those, then delete the old key once the new name is verified.", "Dry run — re-run with --apply to write.", ""].join("\n")
    );
    assert!(rename_preview_writes.is_empty());
    assert!(renamed.stdout.contains("renamed  OLD_NAME -> NEW_NAME  (id preserved)"), "{}", renamed.stdout);
    assert_eq!(renamed_writes, [format!("secret edit --key NEW_NAME id-1 --output none | token={EDIT} url=")]);
    assert_eq!(additive.stdout, format!("copied  {TAILNET} -> {GROQ}  (both names resolve; delete {TAILNET} after the code rename is verified)\n"));
    assert_eq!(additive_writes, [format!("secret create --note {} --output none -- {GROQ} tskey-copied proj-7 | token={EDIT} url=", note(GROQ))]);
    assert_eq!((missing.exit_code, missing.stderr.as_str()), (1, "Refused: no vault secret named NOPE — nothing to rename\n"));
    assert_eq!(exists.stderr, "Refused: a secret named B already exists in the vault — refusing to create a duplicate\n");
    assert!(off_registry.exit_code == 1 && off_registry.stderr.contains("Unknown key: C\nValid keys: "), "{}", off_registry.stderr);

    // When a secret moves to another project: previewed, applied, already there, missing, and named twice
    v.holds(READ, json!([{ "id": "id-1", "key": "A", "projectId": "admin" }, { "id": "id-2", "key": "B", "projectId": "deploy" }, { "id": "id-3", "key": "B", "projectId": "admin" }]));
    let move_preview = write(&["move", "A", "deploy"], &[], None);
    let move_preview_writes = v.writes();
    let moved = write(&["move", "A", "deploy", "--apply"], &[], None);
    let moved_writes = v.writes();
    let already = write(&["move", "A", "admin", "--apply"], &[], None);
    let already_writes = v.writes();
    let move_missing = write(&["move", "NOPE", "deploy"], &[], None);
    let twice = write(&["move", "B", "deploy", "--apply"], &[], None);
    // Then only the applied move writes, by id with the project alone, and the refusals write nothing
    assert_eq!(move_preview.stdout, "would move  A  admin -> deploy  (id preserved)\nDry run — re-run with --apply to write.\n");
    assert!(move_preview_writes.is_empty());
    assert_eq!(moved.stdout, "moved  A  admin -> deploy  (id preserved)\n");
    assert_eq!(moved_writes, [format!("secret edit --project-id deploy id-1 --output none | token={EDIT} url=")]);
    assert_eq!((already.exit_code, already.stdout.as_str()), (0, "A is already in project admin — nothing to move\n"));
    assert!(already_writes.is_empty());
    assert_eq!((move_missing.exit_code, move_missing.stderr.as_str()), (1, "Refused: no vault secret named NOPE — nothing to move\n"));
    assert_eq!((twice.exit_code, twice.stderr.as_str()), (1, "Refused: 2 vault secrets are named B — move one by hand in the web vault\n"));
    assert!(v.writes().is_empty());

    // When set runs on a key in the vault, on a new key from stdin, on empty stdin, and on an unknown key
    v.holds(READ, json!([{ "id": "id-1", "key": GROQ, "value": "oldvalue" }]));
    v.holds(EDIT, json!([{ "id": "id-1", "key": GROQ, "value": "oldvalue" }]));
    let _ = std::fs::remove_file(v.history_path());
    let set = write(&["set", GROQ, "brandnew"], &[], None);
    let set_writes = v.writes();
    let set_history = v.history();
    v.holds(READ, json!([]));
    v.holds(EDIT, json!([]));
    let _ = std::fs::remove_file(v.history_path());
    write(&["set", GROQ], &[("BWS_DEFAULT_PROJECT_ID", "65501031-2ee6-4d57-9e45-b48e0175233f")], Some("fromstdin\n"));
    let new_writes = v.writes();
    let new_history = v.history();
    let empty = write(&["set", GROQ], &[], Some("\n"));
    let empty_calls = v.calls();
    let unknown_set = write(&["set", "NOT_A_KEY", "v"], &[], None);
    // Then an existing secret is updated in place with its old value archived, a new one is created in the deploy project with its note
    assert_eq!((set.exit_code, set.stdout.clone()), (0, format!("{GROQ}: stored\n")));
    assert_eq!(set_writes, [format!("secret edit --value brandnew --output none -- id-1 | token={EDIT} url=")]);
    assert_eq!((set_history[0]["key"].as_str(), set_history[0]["previousValue"].as_str()), (Some(GROQ), Some("oldvalue")));
    assert_eq!(new_writes, [format!("secret create --note {} --output none -- {GROQ} fromstdin 65501031-2ee6-4d57-9e45-b48e0175233f | token={EDIT} url=", note(GROQ))]);
    assert_eq!((new_history[0]["key"].as_str(), new_history[0]["previousValue"].clone()), (Some(GROQ), Value::Null));
    assert_eq!((empty.exit_code, empty.stderr.clone()), (1, format!("{GROQ}: stdin closed with no value provided\n")));
    assert!(empty_calls.is_empty());
    assert!(unknown_set.exit_code == 1 && unknown_set.stderr.contains("Unknown key: NOT_A_KEY\nValid keys: "), "{}", unknown_set.stderr);

    // When delete runs on the read-only Bitwarden backend, and set and delete run on the macOS backend off Darwin
    let delete = write(&["delete", GROQ], &[], None);
    // Then Bitwarden explains where to remove it, and the macOS backend reports success without touching anything
    assert_eq!(delete.exit_code, 1);
    assert!(format!("{}{}", delete.stdout, delete.stderr).contains("The bitwarden backend is read-only — remove the secret in Bitwarden Secrets Manager, not via this CLI."));
    if !cfg!(target_os = "macos") {
        let macos = [("SECRETS_BACKEND", "macos")];
        let (set, del) = (write(&["set", GROQ, "v"], &macos, None), write(&["delete", GROQ], &macos, None));
        assert_eq!((set.stdout, del.stdout), (format!("{GROQ}: stored\n"), format!("{GROQ}: deleted\n")));
        assert!(v.calls().is_empty());
    }
}

const VALUE_REVEALING_SUBCOMMANDS: [&str; 2] = ["env", "get"];

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap()
}

#[test]
fn the_value_revealing_subcommands_are_refused_to_agents_by_both_guards_and_no_default_file_holds_a_vault_key() {
    // Given the subcommands that print a real secret value: get and env (list is masked, sync-env redacts by default)
    // When the exposure guard script's list is read
    let source = read("scripts/guards/block-secret-exposure.mjs");
    let list = Regex::new(r"WF_SECRETS_VALUE_REVEALING_SUBCOMMANDS\s*=\s*\[([^\]]*)\]").unwrap().captures(&source).map(|c| c[1].to_string()).unwrap_or_default();
    let mut scripted: Vec<String> = Regex::new(r"'([^']+)'").unwrap().captures_iter(&list).map(|m| m[1].to_string()).collect();
    scripted.sort();
    // Then it names exactly those subcommands
    assert_eq!(scripted, VALUE_REVEALING_SUBCOMMANDS.map(String::from));

    // When the guard judges each redirected to a file, and each discarded beside an unrelated dump
    let blocks = |command: &str| Command::new("node").arg(repo_root().join("scripts/guards/block-secret-exposure.mjs")).env("GUARD_COMMAND", command).status().unwrap().success();
    for subcommand in VALUE_REVEALING_SUBCOMMANDS {
        // Then only the redirect is blocked
        assert_eq!((blocks(&format!("wf secret {subcommand} KEY > /tmp/leak.txt")), blocks(&format!("wf secret {subcommand} KEY >/dev/null; head -c 40 README.md"))), (true, false), "{subcommand}");
    }

    // When the approve guard's deny list is read
    let deny: Value = serde_json::from_str(&read("libraries/hook-runtime/src/deny.json")).unwrap();
    let rules = deny["rules"].as_array().cloned().unwrap_or_default();
    let covers = |subcommand: &str| {
        rules.iter().any(|rule| {
            let sequence: Vec<&str> = rule["args_sequential"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let (Some(first), Some(second)) = (sequence.first(), sequence.get(1)) else { return false };
            rule["command"] == "wf" && Regex::new(first).unwrap().is_match("secret") && Regex::new(second).unwrap().is_match(subcommand)
        })
    };
    // Then a wf rule matches every one of them
    assert!(VALUE_REVEALING_SUBCOMMANDS.iter().all(|s| covers(s)), "uncovered: {:?}", VALUE_REVEALING_SUBCOMMANDS.iter().filter(|s| !covers(s)).collect::<Vec<_>>());

    // When the committed .env.defaults files are compared with the vault-managed keys of the registry
    let secrets: BTreeSet<String> = registry().values().filter(|e| e["publicValue"] != true).filter_map(|e| e["envVar"].as_str().map(str::to_string)).collect();
    let listing = Command::new("git").args(["ls-files", "*.env.defaults", ".env.defaults"]).current_dir(repo_root()).output().unwrap();
    let leaks: Vec<String> = String::from_utf8_lossy(&listing.stdout)
        .lines()
        // ls-files still lists a file deleted from the tree and not yet staged
        .filter(|l| !l.is_empty() && repo_root().join(l).exists())
        .flat_map(|file| read(file).lines().map(|line| line.trim().trim_start_matches("export").trim_start().split('=').next().unwrap_or("").trim().to_string()).filter(|key| secrets.contains(key)).map(|key| format!("{file}: {key}")).collect::<Vec<_>>())
        .collect();
    // Then none of them carries a vault-managed secret
    assert!(leaks.is_empty(), "{leaks:?}");
}

fn docker(value: &str) -> Value {
    json!({ "id": "3", "key": "DOCKERHUB_TOKEN", "value": value })
}

fn finding(report: &Value, key: &str) -> Value {
    report["findings"].as_array().unwrap().iter().find(|f| f["key"] == key).cloned().unwrap_or(Value::Null)
}

#[test]
fn verify_checks_that_each_secret_of_an_app_is_usable_and_never_prints_a_value() {
    // Given an app whose secrets are all in the vault, and a Docker Hub that accepts the token
    let v = Vault::new();
    v.holds(READ, json!([tailnet(), docker("dckr_pat_secretvalue1")]));
    let hub = MockServer::start(None);
    hub.queue(vec![MockReply::json(200, json!({ "token": "t" }))], true);
    let origin = format!("http://127.0.0.1:{}", hub.port);

    // When its secrets are verified
    let r = v.read(&["verify", "--for", "agent-shell"], &[("WF_VERIFY_ORIGIN", &origin)]);

    // Then each key reports the checks that ran, the token was asked of the provider, and no value is in the output
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(report["ok"], json!(true));
    assert_eq!(finding(&report, "TS_VM_AUTH_KEY")["checks"], json!(["hygiene", "shape"]));
    assert_eq!(finding(&report, "DOCKERHUB_TOKEN")["checks"], json!(["hygiene", "live"]));
    assert!(hub.requests().iter().any(|q| q.url.starts_with("/token?service=registry.docker.io")), "{:?}", hub.requests().iter().map(|q| q.url.clone()).collect::<Vec<_>>());
    assert!(!format!("{}{}", r.stdout, r.stderr).contains("secretvalue1") && !r.stdout.contains("12345678"));
}

#[test]
fn verify_fails_on_a_rejected_token_a_stray_newline_and_a_missing_secret_with_the_failures_first() {
    // Given a Docker Hub that rejects the token, a tailnet key with a trailing newline, and a vault that lacks nothing else required
    let v = Vault::new();
    v.holds(READ, json!([json!({ "id": "2", "key": TAILNET, "value": "tskey-12345678\n" }), docker("dckr_pat_revoked")]));
    let hub = MockServer::start(None);
    hub.queue(vec![MockReply::json(401, json!({ "details": "incorrect username or password" }))], true);
    let origin = format!("http://127.0.0.1:{}", hub.port);

    // When the app is verified
    let r = v.read(&["verify", "--for", "agent-shell"], &[("WF_VERIFY_ORIGIN", &origin)]);

    // Then it exits 1, each failure names its key and why, and the failures come before the passes
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(report["ok"], json!(false));
    assert_eq!(finding(&report, "TS_VM_AUTH_KEY")["reason"], json!("has leading or trailing whitespace or a newline"));
    assert_eq!(finding(&report, "DOCKERHUB_TOKEN")["reason"], json!("rejected by auth.docker.io (HTTP 401)"));
    assert_eq!(report["findings"][0]["status"], json!("failed"));

    // And a required secret that is not in the vault fails as missing, without a provider being asked
    v.holds(READ, json!([tailnet()]));
    let missing = v.read(&["verify", "--for", "agent-shell"], &[("WF_VERIFY_ORIGIN", &origin)]);
    let report: Value = serde_json::from_str(&missing.stdout).unwrap();
    assert_eq!((missing.exit_code, finding(&report, "DOCKERHUB_TOKEN")["reason"].clone()), (1, json!("is not in the vault")));
}

#[test]
fn verify_reports_a_provider_that_does_not_answer_as_unchecked_and_does_not_fail() {
    // Given a Docker Hub that cannot be reached
    let v = Vault::new();
    v.holds(READ, json!([tailnet(), docker("dckr_pat_secretvalue1")]));

    // When the app is verified
    let r = v.read(&["verify", "--for", "agent-shell"], &[("WF_VERIFY_ORIGIN", "http://127.0.0.1:1")]);

    // Then the token is unchecked, not failed, and the exit is 0: a provider outage must not block a deploy
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(finding(&report, "DOCKERHUB_TOKEN")["status"], json!("unchecked"));
}

#[test]
fn verify_refuses_an_unknown_app() {
    let v = Vault::new();
    v.holds(READ, json!([tailnet()]));
    let r = v.read(&["verify", "--for", "nope"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("Unknown --for group: nope"), "{}", r.stderr);
}

fn scope_keys(scope: &str) -> Vec<(String, String)> {
    registry().iter().filter(|(_, e)| e["scope"] == scope).map(|(k, e)| (k.clone(), e["bitwardenKey"].as_str().unwrap_or_else(|| e["envVar"].as_str().unwrap()).to_string())).collect()
}

fn held(keys: &[(String, String)]) -> Vec<Value> {
    keys.iter().enumerate().map(|(i, (_, name))| json!({ "id": i.to_string(), "key": name, "value": "scopedvalue-1234" })).collect()
}

fn names(report: &Value, field: &str) -> BTreeSet<String> {
    report[field].as_array().unwrap().iter().map(|k| k.as_str().or_else(|| k["key"].as_str()).unwrap().to_string()).collect()
}

#[test]
fn registry_scope_every_key_has_a_known_scope_and_only_the_two_betterstack_values_are_public() {
    // Given the registry compiled into wf
    // When each entry's scope is read
    let scopes: BTreeSet<String> = registry().values().map(|e| e["scope"].as_str().unwrap_or("").to_string()).collect();
    let public: BTreeSet<String> = scope_keys("public").into_iter().map(|(k, _)| k).collect();

    // Then every key is in one of the four known scopes, and the public ones are exactly the repo variables the owner decided on
    assert!(scopes.is_subset(&["ci-open", "ci-deploy", "vm", "public"].iter().map(|s| s.to_string()).collect()), "{scopes:?}");
    assert_eq!(public, ["BETTERSTACK_INGEST_HOST", "EXPO_PUBLIC_BETTERSTACK_TOKEN"].iter().map(|s| s.to_string()).collect());

    // And the registry never lets the vault-only privileged keys into the open scope
    let open: BTreeSet<String> = scope_keys("ci-open").into_iter().map(|(k, _)| k).collect();
    for privileged in ["VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY", "VM_OPS_API_AUTH_TOKEN", "DOCKERHUB_TOKEN", "EXPO_TOKEN", "BUILD_CACHE_WRITE_PAR_URL", "OCI_API_PRIVATE_KEY", "VM_PROVISION_GITHUB_TOKEN"] {
        assert!(!open.contains(privileged), "{privileged}");
    }
}

#[test]
fn verify_scope_reports_the_missing_keys_of_the_scope_and_never_a_value() {
    // Given a vault project that lacks two of the ci-open keys
    let v = Vault::new();
    let open = scope_keys("ci-open");
    v.holds(READ, Value::Array(held(&open[2..])));

    // When the ci-open scope is verified
    let r = v.read(&["verify", "--scope", "ci-open"], &[]);

    // Then it exits 1 naming exactly the two missing keys, the rest present, and no value is printed
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!((report["scope"].clone(), report["ok"].clone()), (json!("ci-open"), json!(false)));
    assert_eq!(names(&report, "missing"), open[..2].iter().map(|(k, _)| k.clone()).collect());
    assert_eq!(report["keys"].as_array().unwrap().iter().filter(|k| k["status"] == "present").count(), open.len() - 2);
    assert!(!format!("{}{}", r.stdout, r.stderr).contains("scopedvalue"));
}

#[test]
fn verify_scope_passes_when_every_key_of_the_scope_is_present() {
    // Given a vault project holding exactly the ci-deploy keys
    let v = Vault::new();
    v.holds(READ, Value::Array(held(&scope_keys("ci-deploy"))));

    // When the ci-deploy scope is verified
    let r = v.read(&["verify", "--scope", "ci-deploy"], &[]);

    // Then it passes with nothing missing and nothing exposed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!((report["ok"].clone(), report["exposureChecked"].clone()), (json!(true), json!(true)));
    assert!(names(&report, "missing").is_empty() && names(&report, "exposed").is_empty());
}

#[test]
fn verify_scope_of_ci_open_never_lists_a_vm_key_as_its_own_and_flags_one_the_token_can_read() {
    // Given a project that holds every ci-open key and also a vm key the ci token must not read
    let v = Vault::new();
    let mut vault = held(&scope_keys("ci-open"));
    vault.push(json!({ "id": "x", "key": "VM_STATE_RESTIC_PASSWORD", "value": "scopedvalue-1234" }));
    v.holds(READ, Value::Array(vault));

    // When the ci-open scope is verified
    let r = v.read(&["verify", "--scope", "ci-open"], &[]);

    // Then the vm key is not among the scope's keys, and is reported as exposed with its scope, failing the check
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert!(!names(&report, "keys").contains("VM_STATE_RESTIC_PASSWORD") && names(&report, "missing").is_empty());
    assert_eq!(report["exposed"], json!([{ "key": "VM_STATE_RESTIC_PASSWORD", "scope": "vm" }]));
}

#[test]
fn verify_scope_refuses_a_scope_that_is_not_a_ci_scope() {
    // Given any vault, when a non-CI scope is asked for, then it is refused naming the known ones
    let v = Vault::new();
    v.holds(READ, json!([groq()]));
    let r = v.read(&["verify", "--scope", "vm"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("Unknown --scope: vm") && r.stderr.contains("ci-open, ci-deploy"), "{}", r.stderr);
}

#[test]
fn verify_of_a_group_skips_the_key_the_app_reads_only_on_the_vm() {
    // Given a vault that holds the vm-ops-api keys the runner reads and VM_PROVISION_GITHUB_TOKEN with a trailing newline, which would fail hygiene
    let v = Vault::new();
    v.holds(READ, json!([tailnet(), json!({ "id": "4", "key": "VM_OPS_API_AUTH_TOKEN", "value": "ops-token-12345678" }), json!({ "id": "6", "key": "VM_OPS_API_CI_AUTH_TOKEN", "value": "ci-ops-token-12345678" }), json!({ "id": "5", "key": "VM_PROVISION_GITHUB_TOKEN", "value": "github_pat_zzzz\n" })]));

    // When the group of vm-ops-api is verified, as a deploy leg does
    let r = v.read(&["verify", "--for", "vm-ops-api"], &[]);

    // Then it passes without a finding for that key, which only the VM reads
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(finding(&report, "VM_PROVISION_GITHUB_TOKEN"), Value::Null, "{report}");
    assert_ne!(finding(&report, "VM_OPS_API_AUTH_TOKEN"), Value::Null, "{report}");
}
