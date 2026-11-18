//! The environment of every child `wf mimic capture` starts: one allowlist, no inheritance, and a key reaches a process only when the set declares it by
//! name (`[reference] env_from_vault`). Each launch path dumps its own environment to a file; the tests read what each one saw.
use crate::support::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const KEY: &str = "OPENCODE_GO_API_KEY";
/// The variable the reference's program reads the key under.
const ENV_NAME: &str = "OPENCODE_API_KEY";
const DECLARE: &str = "{ OPENCODE_API_KEY = \"OPENCODE_GO_API_KEY\" }";
const VALUE: &str = "sk-oc-4f9a1b7c2d3e";

/// What a caller of the capture holds: a vault token, GitHub's context and token, a key nobody declared and a deploy token.
const CALLER: [(&str, &str); 7] = [("SECRETS_BACKEND", "bitwarden"), ("BWS_ADMIN_ACCESS_TOKEN", "bws-0.aaaa"), ("GITHUB_TOKEN", "ghs_bbbb"), ("GITHUB_ENV", "/tmp/github-env"), ("MODEL_API_KEY", "k-cccc"), ("DEPLOY_TOKEN", "dddd"), ("MIMIC_FAKE_VISIBLE", "yes")];

fn sidecar(prelude: &str, echo: &str) -> String {
    format!(
        r#"{prelude}
while read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":"\([0-9]*\)".*/\1/p')
  case "$line" in
    *'"op":"connect"'*) echo "{{\"id\":\"$id\",\"ok\":{{\"state\":\"connected\"}}}}" ;;
    *'"op":"request"'*) echo "{{\"id\":\"$id\",\"ok\":{{\"response\":{{\"ok\":true,\"result\":{{\"echo\":\"{echo}\"}}}}}}}}" ;;
    *'"op":"quit"'*) echo "{{\"id\":\"$id\",\"ok\":{{}}}}"; exit 0 ;;
    *) echo "{{\"id\":\"$id\",\"ok\":{{}}}}" ;;
  esac
done"#
    )
}

/// A stand-in for `wf-mimic` that dumps the environment of each child the capture starts, and answers the way the real one does.
const FAKE: &str = r#"#!/bin/sh
[ "$1" = mimic ] || exit 2
dir="$MIMIC_FAKE_ENV_DIR"
case "$2" in
  record)
    if [ "$3" = --help ]; then echo "  --resume  --max-minutes <n>"; exit 0; fi
    env > "$dir/record.env"
    echo '{"schema":"mimic-cassette/1"}' > "$3/cassette.json"
    echo '{"id":"a"}' > "$3/interactions.jsonl"
    echo '{"ok":true,"set":"s","interactions":1,"complete":true,"nondeterministic":[],"warnings":[]}' ;;
  verify) env > "$dir/verify.env"; echo '{"ok":true,"warnings":[]}' ;;
  replay) env > "$dir/replay.env"; echo '{"ok":true,"severity":"none","summary":{"total":0,"clean":0,"allowlisted":0,"drift":0,"breaking":0},"results":[]}' ;;
  *) exit 2 ;;
esac
"#;

/// The vault as `bws` serves it: `secret list` answers the listing in `$STUB_DIR/list.json`, and every call is logged with the token it was given.
const STUB_BWS: &str = r#"#!/bin/sh
echo "$* | token=$BWS_ACCESS_TOKEN" >> "$STUB_DIR/bws.log"
[ "$1 $2" = "secret list" ] && { cat "$STUB_DIR/list.json"; exit 0; }
exit 0
"#;

struct Rig {
    dir: tempfile::TempDir,
    set: PathBuf,
}

fn write_script(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

impl Rig {
    /// A set whose reference is an orca sidecar that dumps its environment and declares the key, one provision step that dumps its own, and a chain
    /// candidate whose launcher and sidecar dump theirs.
    fn orca() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let (set, dump) = (dir.path().join("s"), dir.path().join("dump"));
        std::fs::create_dir_all(&set).unwrap();
        std::fs::create_dir_all(&dump).unwrap();
        let w = |name: &str, text: &str| std::fs::write(set.join(name), text).unwrap();
        w("pin.json", r#"{"version":"fake-1.0"}"#);
        w("rules.json", RULES);
        let case = json!({"kind": "corpus", "name": "files-list", "op": "script", "cases": [{"id": "files-list", "row": "rpc:files.list", "input": {"requests": [{"method": "files.list", "params": {}}]}}]});
        w("corpus.json", &json!({"schema": "mimic-corpus/1", "seed": 1, "generators": [case]}).to_string());
        w("reference.sh", &sidecar("env > \"$1\"", "hello"));
        w("candidate.sh", &sidecar("env > \"$1\"", "hello"));
        w("launcher.sh", "env > \"$1\"\necho '{\"ok\":true,\"event\":\"listening\",\"pairing\":\"orca://chain\"}'\ncat > /dev/null\n");
        let d = |name: &str| dump.join(name).display().to_string();
        w(
            "mimic.toml",
            &format!(
                "[provision]\ndrivers = false\n[[provision.step]]\nname = \"dump\"\nrun = [\"sh\", \"-c\", \"env > {{work}}/provision.env\"]\n[[provision.step]]\nname = \"dump-for-replay\"\nwhen = \"replay\"\nrun = [\"sh\", \"-c\", \"env > {{work}}/provision-replay.env\"]\n\n\
                 [reference]\npin = \"pin.json\"\nversion_pointer = \"/version\"\nenv_from_vault = {DECLARE}\ndriver = \"script\"\n[reference.script]\ntarget = \"orca\"\nsidecar = [\"sh\", \"reference.sh\", \"{}\"]\n\n\
                 [candidate.chain]\ndriver = \"script\"\n[candidate.chain.script]\ntarget = \"orca\"\nsidecar = [\"sh\", \"candidate.sh\", \"{}\"]\nlaunch = [\"sh\", \"{}\", \"{}\"]\nready_ms = 5000\n",
                d("reference.env"),
                d("candidate.env"),
                set.join("launcher.sh").display(),
                d("launcher.env")
            ),
        );
        write_script(&dir.path().join("fake-wf-mimic"), FAKE);
        std::fs::create_dir_all(dir.path().join("vault/bin")).unwrap();
        write_script(&dir.path().join("vault/bin/bws"), STUB_BWS);
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        let rig = Rig { dir, set };
        rig.vault_holds(Some(VALUE));
        rig
    }

    /// What the vault lists for the declared key: the value, or nothing.
    fn vault_holds(&self, value: Option<&str>) {
        let entries = value.map(|v| json!({"id": "1", "key": KEY, "value": v, "note": "", "projectId": "p"})).into_iter().collect::<Vec<_>>();
        std::fs::write(self.dir.path().join("vault/list.json"), json!(entries).to_string()).unwrap();
    }

    /// The calls the capture made to the vault.
    fn vault_calls(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("vault/bws.log")).unwrap_or_default()
    }

    /// A set whose reference is a program speaking `mimic-driver/1` over stdin and stdout that dumps its environment and declares the key.
    fn stdio() -> Rig {
        let rig = Rig::orca();
        let dump = rig.dump("driver.env");
        std::fs::write(rig.set.join("driver.sh"), "env > \"$1\"\nwhile read -r line; do\n  case \"$line\" in\n    *'\"hello\"'*) echo '{\"hello\":{\"driver\":{\"name\":\"d\",\"version\":\"1\",\"protocol\":\"mimic-driver/1\"},\"coverage\":false}}' ;;\n    *) echo '{\"id\":\"x\",\"error\":\"not a case\"}' ;;\n  esac\ndone\n").unwrap();
        std::fs::write(rig.set.join("mimic.toml"), format!("[reference]\ncommand = [\"sh\", \"driver.sh\", \"{}\"]\npin = \"pin.json\"\nenv_from_vault = {DECLARE}\n[candidate.same]\ncommand = [\"true\"]\n", dump.display())).unwrap();
        rig
    }

    fn dump(&self, name: &str) -> PathBuf {
        self.dir.path().join("dump").join(name)
    }

    fn work(&self, name: &str) -> PathBuf {
        self.dir.path().join("work/s").join(name)
    }

    /// `capture <action> <set> <args>` with the caller's credentials in its environment, the vault stub on its PATH, the stand-in `wf-mimic` when `fake`.
    fn capture(&self, action: &str, args: &[&str], fake: bool) -> Run {
        let (set, work, dump, summary) = (self.set.display().to_string(), self.dir.path().join("work").display().to_string(), self.dir.path().join("dump"), self.dir.path().join("summary.md"));
        let mut all = vec!["capture", action, set.as_str(), "--work-root", work.as_str(), "--summary", summary.to_str().unwrap()];
        all.extend_from_slice(args);
        let fake_bin = self.dir.path().join("fake-wf-mimic");
        let mut env: Vec<(&str, &Path)> = CALLER.iter().map(|(k, v)| (*k, Path::new(*v))).collect();
        let beside = Path::new(env!("CARGO_BIN_EXE_wf-mimic")).parent().expect("a directory").display().to_string();
        let path = PathBuf::from(format!("{}:{beside}:{}", self.dir.path().join("vault/bin").display(), std::env::var("PATH").unwrap_or_default()));
        let (home, vault) = (self.dir.path().join("home"), self.dir.path().join("vault"));
        env.extend([("MIMIC_FAKE_ENV_DIR", dump.as_path()), ("PATH", path.as_path()), ("HOME", home.as_path()), ("STUB_DIR", vault.as_path())]);
        if fake {
            env.push(("WF_MIMIC_BIN", &fake_bin));
        }
        mimic(&all, &env)
    }
}

fn names(path: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines().filter_map(|l| l.split_once('=')).map(|(k, _)| k.to_string()).collect()
}

/// The names a dump holds, and the dump itself.
fn snap(path: &Path) -> (BTreeSet<String>, String) {
    (names(path), text(path))
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn undeclared_secrets(seen: &BTreeSet<String>) -> Vec<&String> {
    let shaped = |n: &str| ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "API_KEY"].iter().any(|k| n.contains(k)) || n.ends_with("_KEY");
    seen.iter().filter(|n| n.starts_with("BWS_") || n.starts_with("GITHUB_") || (shaped(n) && n.as_str() != ENV_NAME)).collect()
}

#[test]
fn no_launch_path_gives_a_child_the_vault_token_a_github_variable_or_an_undeclared_key_and_the_declared_key_reaches_the_reference_alone() {
    // Given a caller holding a vault token, GitHub's token and context, a key nobody declared, and the key the set's reference declares
    let rig = Rig::orca();
    // When a record runs over the stand-in wf-mimic (provisioning, the record child and the verify child), then the real one (the reference's sidecar)
    let staged = rig.capture("record", &[], true);
    let provision = snap(&rig.work("provision.env"));
    let real = rig.capture("record", &[], false);
    // And a replay runs over the stand-in (a replay's provisioning and the replay child), then the real one (the chain launcher and the candidate's sidecar)
    let replay_child = rig.capture("replay", &["--candidate", "chain"], true);
    let provision_replay = snap(&rig.work("provision-replay.env"));
    let replay = rig.capture("replay", &["--candidate", "chain"], false);
    // And a set whose reference is a stdio driver is recorded
    let stdio = Rig::stdio();
    let driver = stdio.capture("record", &["--provision", "skip"], false);
    for (what, r) in [("staged record", &staged), ("replay over the stand-in", &replay_child), ("replay", &replay)] {
        assert!(r.json["captured"] == json!(true), "{what}: {}\n{}", r.json, r.stderr);
    }
    assert!(real.json["action"] == json!("record"), "{}\n{}", real.json, real.stderr);
    assert!(driver.json["action"] == json!("record") || driver.json["error"].is_string(), "{}\n{}", driver.json, driver.stderr);

    // Then every child saw its own variables and none of the credentials
    let dumps = [
        (provision, "a provisioning step"),
        (provision_replay, "a replay's provisioning step"),
        (snap(&rig.dump("verify.env")), "the verify child"),
        (snap(&rig.dump("replay.env")), "the replay child"),
        (snap(&rig.dump("launcher.env")), "the chain launcher"),
        (snap(&rig.dump("candidate.env")), "the candidate's sidecar"),
        (snap(&rig.dump("reference.env")), "the reference's sidecar"),
        (snap(&stdio.dump("driver.env")), "the stdio reference"),
        (snap(&rig.dump("record.env")), "the record child"),
    ];
    for ((seen, raw), what) in &dumps {
        assert!(seen.contains("PATH"), "{what} has no PATH: {seen:?}");
        assert_eq!(undeclared_secrets(seen), Vec::<&String>::new(), "{what}");
        for value in ["bws-0.aaaa", "ghs_bbbb", "k-cccc", "dddd"] {
            assert!(!raw.contains(value), "{what} holds a credential value");
        }
    }
    // And the variables the repository's own knobs name (MIMIC_*) still pass
    assert!(dumps[4].0 .0.contains("MIMIC_FAKE_VISIBLE"));

    // And the declared key is in the reference's processes and the record child that starts them, nowhere else
    let holders: Vec<&str> = dumps.iter().filter(|((seen, _), _)| seen.contains(ENV_NAME)).map(|(_, what)| *what).collect();
    assert_eq!(holders, vec!["the reference's sidecar", "the stdio reference", "the record child"], "who holds the key");

    // And nothing the capture printed or wrote to its summary holds the value
    for r in [&staged, &real, &replay_child, &replay, &driver] {
        assert!(!r.json.to_string().contains(VALUE) && !r.stderr.contains(VALUE), "{}\n{}", r.json, r.stderr);
    }
    assert!(!text(&rig.dir.path().join("summary.md")).contains(VALUE));
}

#[test]
fn a_declared_key_the_vault_does_not_hold_fails_naming_the_key_never_a_value_and_only_a_record_asks_for_it() {
    // Given a set whose reference declares a key and a caller that has none
    let rig = Rig::orca();
    // When it is recorded with a vault that holds no value for the key
    rig.vault_holds(None);
    let r = rig.capture("record", &[], true);
    // Then it fails naming the key and the way to supply it, before any record child ran
    assert_eq!(r.code, 2, "{}", r.json);
    let error = r.json["error"].as_str().unwrap();
    assert!(error.contains(KEY) && error.contains("env_from_vault"), "{error}");
    assert!(!rig.dump("record.env").exists());
    // When the same set is replayed, or only checked, without the key
    let calls_before = rig.vault_calls();
    let replay = rig.capture("replay", &["--candidate", "chain"], true);
    let check = rig.capture("record", &["--check"], true);
    // Then neither asks for it, and the vault was not read for either
    assert_eq!((replay.code, check.code), (0, 0), "{}\n{}", replay.json, check.json);
    assert!(!names(&rig.dump("replay.env")).contains(ENV_NAME));
    assert_eq!(rig.vault_calls(), calls_before, "a replay or a check reads no vault");
}

#[test]
fn a_recording_or_a_cassette_holding_a_declared_keys_value_is_held_back_naming_the_key_and_a_clean_one_goes_on() {
    // Given a reference whose answer repeats the key it was given, and one that does not
    let rig = Rig::orca();
    std::fs::write(rig.set.join("reference.sh"), sidecar("env > \"$1\"", &format!("${ENV_NAME}"))).unwrap();
    // When it is recorded for real with the key
    let held = rig.capture("record", &[], false);
    // Then the record is held back (exit 1), the finding names the key and the file, and the value is nowhere in what was printed
    assert_eq!((held.code, held.json["ok"].clone()), (1, json!(false)), "{}\n{}", held.json, held.stderr);
    let leaks = held.json["leaks"].to_string();
    assert!(leaks.contains(&format!("holds the value of {KEY}")) && leaks.contains("interactions.jsonl"), "{leaks}");
    assert!(!held.json.to_string().contains(VALUE) && !held.stderr.contains(VALUE), "{}\n{}", held.json, held.stderr);
    // When the reference does not repeat it
    std::fs::write(rig.set.join("reference.sh"), sidecar("env > \"$1\"", "hello")).unwrap();
    let clean = rig.capture("record", &["--fresh"], false);
    // Then the leak list is empty
    assert_eq!(clean.json["leaks"], json!([]), "{}\n{}", clean.json, clean.stderr);
}

#[test]
fn a_set_may_declare_a_key_only_on_its_reference_by_an_upper_snake_name_and_never_a_vault_or_github_variable() {
    let cases = [
        ("[reference]\nenv_from_vault = { OPENCODE_API_KEY = \"BWS_ACCESS_TOKEN\" }\ncommand = [\"true\"]\n", "never a BWS_* or GITHUB_*"),
        ("[reference]\nenv_from_vault = { GITHUB_TOKEN = \"OPENCODE_GO_API_KEY\" }\ncommand = [\"true\"]\n", "never a BWS_* or GITHUB_*"),
        ("[reference]\nenv_from_vault = { lower_case = \"OPENCODE_GO_API_KEY\" }\ncommand = [\"true\"]\n", "UPPER_SNAKE"),
        ("[reference]\nenv_from_vault = [\"OPENCODE_GO_API_KEY\"]\ncommand = [\"true\"]\n", "invalid type"),
        ("[reference]\ncommand = [\"true\"]\n[candidate.main]\nenv_from_vault = { OPENCODE_API_KEY = \"OPENCODE_GO_API_KEY\" }\ncommand = [\"true\"]\n", "only [reference] may declare a vault key"),
    ];
    for (toml, reason) in cases {
        // Given a mimic.toml with the declaration
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mimic.toml"), toml).unwrap();
        std::fs::write(dir.path().join("pin.json"), "{}").unwrap();
        // When the set is checked
        let r = mimic(&["capture", "record", &dir.path().display().to_string(), "--check"], &[]);
        // Then it is refused for that reason
        assert_eq!(r.code, 2, "{toml}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains(reason), "{toml}: {}", r.json["error"]);
    }
}

fn platform() -> &'static str {
    if std::env::consts::ARCH == "aarch64" {
        "linux-arm64"
    } else {
        "linux-x64"
    }
}

/// A fake release asset of `opencode` that reports `version`, as the pinned release would be: the asset's bytes and the pin file that names them.
fn release_asset(dir: &Path, version: &str) -> (Vec<u8>, String) {
    let stage = dir.join("stage");
    std::fs::create_dir_all(&stage).unwrap();
    write_script(&stage.join("opencode"), &format!("#!/bin/sh\necho {version}\n"));
    let asset = dir.join(format!("opencode-{platform}.tar.gz", platform = platform()));
    assert!(std::process::Command::new("tar").arg("czf").arg(&asset).arg("-C").arg(&stage).arg("opencode").status().unwrap().success());
    let bytes = std::fs::read(&asset).unwrap();
    let sum = mimic_core::json::sha256_label(&bytes).trim_start_matches("sha256:").to_string();
    let platforms: serde_json::Map<String, Value> = ["linux-x64", "linux-arm64"].iter().map(|p| (p.to_string(), json!({"bytes": bytes.len(), "sha256": sum}))).collect();
    (bytes, json!({"opencode": {"version": version, "release": "https://example.org/opencode/releases/download/v1.19.0", "platforms": platforms}}).to_string())
}

impl Rig {
    /// The orca rig whose provision block runs the real opencode: the pin file is beside the set, the asset is in the cache a capture is pointed at.
    fn real() -> Rig {
        let rig = Rig::orca();
        let (_, pin) = release_asset(&rig.dir.path().join("agents-cache"), "1.19.0");
        std::fs::write(rig.set.join("real-agents.json"), pin).unwrap();
        rig.with_provision("real_agents = [\"opencode\"]\nreal_agents_pin = \"real-agents.json\"\n");
        rig
    }

    /// The set's `[provision]` table with `extra` lines added to it.
    fn with_provision(&self, extra: &str) {
        let toml = std::fs::read_to_string(self.set.join("mimic.toml")).unwrap();
        std::fs::write(self.set.join("mimic.toml"), toml.replacen("[provision]\ndrivers = false\n", &format!("[provision]\ndrivers = false\n{extra}"), 1)).unwrap();
    }

    fn cache(&self) -> String {
        self.dir.path().join("agents-cache").display().to_string()
    }
}

#[test]
fn a_provision_block_with_real_agents_and_a_pin_is_accepted_and_names_what_it_installs() {
    // Given a set whose provision block runs opencode for real, with a pin file
    let rig = Rig::real();
    // When the block is checked
    let r = rig.capture("record", &["--check"], true);
    // Then it is accepted: the real agents step is in the plan with the pinned version and the directory the host puts on PATH
    assert_eq!(r.code, 0, "{}\n{}", r.json, r.stderr);
    let provision = &r.json["provision"];
    assert!(provision["steps"].as_array().unwrap().iter().any(|s| s == "real-agents"), "{provision}");
    assert_eq!(provision["real_agents"][0]["name"], json!("opencode"));
    assert_eq!(provision["real_agents"][0]["version"], json!("1.19.0"));
    assert!(provision["env"]["MIMIC_REAL_CLI_DIR"].as_str().is_some_and(|d| d.ends_with("/real-clis")), "{provision}");
    // And claude and codex are not in it: they stay scripted
    assert_eq!(provision["real_agents"].as_array().unwrap().len(), 1);
}

#[test]
fn a_real_agents_block_that_is_unknown_unpinned_or_pinned_loosely_is_refused_naming_the_rule() {
    let sum = "0".repeat(64);
    let pin = |entry: Value| json!({"opencode": entry}).to_string();
    let platforms = json!({"linux-x64": {"sha256": sum}, "linux-arm64": {"sha256": sum}});
    let good = json!({"version": "1.19.0", "release": "https://example.org/r", "platforms": platforms});
    let cases = [
        ("real_agents = [\"gemini\"]\nreal_agents_pin = \"p.json\"\n", pin(good.clone()), "is not an agent CLI a set can run for real"),
        ("real_agents = [\"claude\"]\nreal_agents_pin = \"p.json\"\n", pin(good.clone()), "claude and codex stay scripted"),
        ("real_agents = [\"codex\"]\nreal_agents_pin = \"p.json\"\n", pin(good.clone()), "claude and codex stay scripted"),
        ("real_agents = [\"opencode\"]\n", pin(good.clone()), "needs real_agents_pin"),
        ("real_agents_pin = \"p.json\"\n", pin(good.clone()), "real_agents_pin without real_agents"),
        ("real_agents = [\"opencode\", \"opencode\"]\nreal_agents_pin = \"p.json\"\n", pin(good.clone()), "twice"),
        ("real_agents = [\"opencode\"]\nreal_agents_pin = \"p.json\"\n", json!({}).to_string(), "is not pinned"),
        ("real_agents = [\"opencode\"]\nreal_agents_pin = \"p.json\"\n", pin(json!({"version": "latest", "release": "https://example.org/r", "platforms": platforms})), "no full version"),
        ("real_agents = [\"opencode\"]\nreal_agents_pin = \"p.json\"\n", pin(json!({"version": "1.19.0", "release": "https://example.org/r", "platforms": {"linux-x64": {"sha256": sum}}})), "64-hex sha256 for linux-arm64"),
        ("real_agents = [\"opencode\"]\nreal_agents_pin = \"p.json\"\n", pin(json!({"version": "1.19.0", "release": "https://example.org/r", "platforms": {"linux-x64": {"sha256": "abc"}, "linux-arm64": {"sha256": sum}}})), "64-hex sha256 for linux-x64"),
        ("real_agents = [\"opencode\"]\nreal_agents_pin = \"p.json\"\n", pin(json!({"version": "1.19.0", "release": "http://example.org/r", "platforms": platforms})), "https release URL"),
        ("real_agents = [\"opencode\"]\nreal_agents_pin = \"p.json\"\n", "not json".to_string(), "is not JSON"),
    ];
    for (block, pin_file, reason) in cases {
        // Given a provision block with the real agents setting and a pin file
        let rig = Rig::orca();
        std::fs::write(rig.set.join("p.json"), &pin_file).unwrap();
        rig.with_provision(block);
        // When the set is checked
        let r = rig.capture("record", &["--check"], true);
        // Then it is refused for that rule
        assert_eq!(r.code, 2, "{block}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains(reason), "{block} wants {reason:?}: {}", r.json["error"]);
    }
}

#[test]
fn a_set_with_real_agents_still_refuses_a_credential_shaped_name_in_its_env() {
    // Given a real-agents block that also puts the key in its [env]
    let rig = Rig::real();
    let toml = std::fs::read_to_string(rig.set.join("mimic.toml")).unwrap();
    std::fs::write(rig.set.join("mimic.toml"), toml.replacen("[[provision.step]]\nname = \"dump\"", "[provision.env]\nOPENCODE_API_KEY = \"sk-literal-value\"\n[[provision.step]]\nname = \"dump\"", 1)).unwrap();
    // When the set is checked
    let r = rig.capture("record", &["--check"], true);
    // Then the env guard refuses it, naming the variable and never the value
    assert_eq!(r.code, 2, "{}", r.json);
    let error = r.json["error"].as_str().unwrap();
    assert!(error.contains("OPENCODE_API_KEY") && error.contains("looks like a credential") && !error.contains("sk-literal-value"), "{error}");
}

#[test]
fn the_pinned_opencode_is_installed_only_when_its_download_hashes_to_the_pin() {
    // Given a cache holding the release asset the pin names, and one holding another
    let rig = Rig::real();
    // When the provision runs
    let ok = rig.capture("record", &["--provision", "only", "--agents-cache", &rig.cache()], false);
    // Then the program is in real-clis and reports the pinned version
    assert_eq!(ok.code, 0, "{}\n{}", ok.json, ok.stderr);
    let program = rig.work("real-clis/opencode");
    assert_eq!(String::from_utf8_lossy(&std::process::Command::new(&program).arg("--version").output().unwrap().stdout).trim(), "1.19.0");
    assert!(!rig.work("real-clis/.opencode-linux-x64.tar.gz").exists());
    // When the asset in the cache is another build than the pin names
    let other = tempfile::tempdir().unwrap();
    release_asset(other.path(), "9.9.9");
    let swapped = rig.dir.path().join("agents-cache/stage");
    std::fs::remove_dir_all(&swapped).ok();
    let bad = format!("opencode-{}.tar.gz", platform());
    std::fs::copy(other.path().join(&bad), rig.dir.path().join("agents-cache").join(&bad)).unwrap();
    let refused = rig.capture("record", &["--provision", "only", "--force", "--agents-cache", &rig.cache()], false);
    // Then nothing is unpacked and the error names the asset, the hash it has and the pin
    assert_eq!(refused.code, 2, "{}", refused.json);
    let error = refused.json["error"].as_str().unwrap();
    assert!(error.contains(&bad) && error.contains("the pin says") && error.contains("nothing was unpacked"), "{error}");
    assert!(!rig.work("real-clis/opencode").exists());
}

#[test]
fn a_real_agents_record_puts_the_real_clis_first_on_the_references_path_with_the_key_in_the_reference_alone() {
    // Given a set that runs opencode for real, declares the key, and a caller that holds the vault token and GitHub's
    let rig = Rig::real();
    // When it is recorded for real (provision, then the reference's sidecar)
    let r = rig.capture("record", &["--agents-cache", &rig.cache()], false);
    // Then the reference's PATH starts with the directory of the real CLIs and the reference holds the key
    assert_eq!(r.json["action"], json!("record"), "{}\n{}", r.json, r.stderr);
    let path = text(&rig.dump("reference.env")).lines().find_map(|l| l.strip_prefix("PATH=")).unwrap().to_string();
    assert!(path.starts_with(&rig.work("real-clis").display().to_string()), "{path}");
    assert!(names(&rig.dump("reference.env")).contains(ENV_NAME));
    // And the provisioning steps (which installed it) never saw the key, the vault token or GitHub's variables
    let (seen, raw) = snap(&rig.work("provision.env"));
    assert!(!seen.contains(ENV_NAME) && undeclared_secrets(&seen).is_empty() && !raw.contains(VALUE), "{seen:?}");
    // And the value is nowhere in what the capture printed
    assert!(!r.json.to_string().contains(VALUE) && !r.stderr.contains(VALUE));
}

#[test]
fn a_spawned_reference_that_needs_real_agents_and_a_key_is_recorded_with_the_key_in_the_record_child_alone() {
    // Given a set whose reference is a spawned server and whose provision runs opencode for real and declares the key
    let rig = Rig::real();
    let toml = std::fs::read_to_string(rig.set.join("mimic.toml")).unwrap();
    std::fs::write(rig.set.join("mimic.toml"), toml.replace("target = \"orca\"\nsidecar = [\"sh\", \"reference.sh\"", "target = \"spawn\"\nsidecar = [\"sh\", \"reference.sh\"")).unwrap();
    // When it is recorded
    let r = rig.capture("record", &["--agents-cache", &rig.cache()], true);
    // Then it is not refused: the record child ran holding the key, the provisioning steps did not
    assert_eq!(r.code, 0, "{}\n{}", r.json, r.stderr);
    assert!(names(&rig.dump("record.env")).contains(ENV_NAME));
    let (seen, raw) = snap(&rig.work("provision.env"));
    assert!(!seen.contains(ENV_NAME) && !raw.contains(VALUE), "{seen:?}");
    assert!(!r.json.to_string().contains(VALUE) && !r.stderr.contains(VALUE));
}

#[test]
fn a_url_reference_that_declares_a_key_is_recorded_and_its_mint_command_is_left_to_the_host_which_starts_it_without_the_key() {
    // Given a set whose reference is a url script target and declares the key
    let rig = Rig::real();
    let toml = std::fs::read_to_string(rig.set.join("mimic.toml")).unwrap();
    std::fs::write(rig.set.join("mimic.toml"), toml.replace("target = \"orca\"\nsidecar = [\"sh\", \"reference.sh\"", "target = \"url\"\naddr = \"127.0.0.1:1\"\nworkspace = \"/w\"\nmint = [\"true\"]\nsidecar = [\"sh\", \"reference.sh\"")).unwrap();
    // When it is recorded
    let r = rig.capture("record", &["--agents-cache", &rig.cache()], true);
    // Then it is not refused: the record child ran holding the key, and the value is nowhere in what the capture printed
    assert_eq!(r.code, 0, "{}\n{}", r.json, r.stderr);
    assert!(names(&rig.dump("record.env")).contains(ENV_NAME));
    assert!(!r.json.to_string().contains(VALUE) && !r.stderr.contains(VALUE));
}
