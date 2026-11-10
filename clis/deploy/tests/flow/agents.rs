//! The VM's dependency manifest and its installs (`wf vm deps`): apt, npm, mise and mkdir are argv-recording stubs.
use crate::support::{node_path, run_wf, RunOpts, RunResult};
use wf_vmops::testing::{answers, FakeVm};
use regex::Regex;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// dpkg-query, apt-get, npm, mise and mkdir in one: logs `<name> <argv>` and answers from `table.json` by full line, then by name.
const TOOL: &str = "const fs = require('fs');
const dir = process.env.STUB_DIR;
const args = process.argv.slice(2);
const name = require('path').basename(process.argv[1]);
fs.appendFileSync(dir + '/calls.log', name + ' ' + args.join(' ') + '\\n');
let table = {};
try { table = JSON.parse(fs.readFileSync(dir + '/table.json', 'utf8')); } catch {}
const entry = table[name + ' ' + args.join(' ')] || table[name] || {};
if (entry.exit) { process.stderr.write(entry.stderr || ''); process.exit(entry.exit); }
process.stdout.write(entry.stdout || '');
";

/// A stand-in for npm that really installs: `install -g <name>` writes a package.json under the configured prefix.
const REAL_SHAPED_NPM: &str = "#!/bin/sh\n[ -n \"$FAKE_NPM_LOG\" ] && echo \"$*\" >> \"$FAKE_NPM_LOG\"\nprefix=\"${npm_config_prefix:-$NPM_CONFIG_PREFIX}\"\nif [ \"$1\" = install ] && [ \"$2\" = -g ]; then\n  [ -z \"$prefix\" ] && { echo 'fake npm: no prefix configured' >&2; exit 1; }\n  mkdir -p \"$prefix/lib/node_modules/$3\"\n  printf '{\"name\":\"%s\",\"version\":\"0.0.0-fake\"}' \"$3\" > \"$prefix/lib/node_modules/$3/package.json\"\n  exit 0\nfi\necho \"fake npm: unhandled args $*\" >&2\nexit 1\n";

struct Deps {
    dir: tempfile::TempDir,
    manifest: PathBuf,
    prefix: PathBuf,
}

impl Deps {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("wf-flow-deps-").tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        for name in ["dpkg-query", "apt-get", "npm", "mise", "mkdir"] {
            let path = bin.join(name);
            std::fs::write(&path, format!("#!{}\n{TOOL}", node_path().expect("node on PATH").display())).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { manifest: dir.path().join("state/packages.json"), prefix: dir.path().join("npm-prefix"), dir }
    }

    /// `wf vm deps <args>` with the tools answering from `table`; the calls of earlier steps are forgotten.
    fn deps(&self, args: &[&str], table: Value) -> RunResult {
        let _ = std::fs::remove_file(self.dir.path().join("calls.log"));
        std::fs::write(self.dir.path().join("table.json"), table.to_string()).unwrap();
        let mut full = vec!["vm", "deps"];
        full.extend_from_slice(args);
        run_wf(
            &full,
            &RunOpts::default()
                .bin_dir(&self.dir.path().join("bin"))
                .cwd(self.dir.path())
                .clean_env()
                .env("STUB_DIR", &self.dir.path().display().to_string())
                .env("AGENT_DEPS_MANIFEST_PATH", &self.manifest.display().to_string())
                .env("NPM_CONFIG_PREFIX", &self.prefix.display().to_string()),
        )
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("calls.log")).map(|log| log.trim().lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
    }

    fn seed(&self, entries: &[(&str, &str)]) {
        std::fs::create_dir_all(self.dir.path().join("state")).unwrap();
        let entries: Vec<Value> = entries.iter().map(|(manager, name)| json!({ "manager": manager, "name": name, "addedAt": "2026-01-01T00:00:00.000Z" })).collect();
        std::fs::write(&self.manifest, json!({ "entries": entries }).to_string()).unwrap();
    }
}

fn parsed(r: &RunResult) -> Value {
    serde_json::from_str(&r.stdout).unwrap_or_else(|e| panic!("stdout {:?} (stderr {:?}): {e}", r.stdout, r.stderr))
}

fn outcomes(r: &RunResult) -> Vec<String> {
    parsed(r)["results"].as_array().unwrap().iter().map(|x| x["outcome"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_vm_deps_manifest_records_what_an_agent_installed_and_replay_brings_it_back() {
    let d = Deps::new();

    // Given no manifest, or a corrupt or foreign one, Then the list is empty
    assert_eq!(parsed(&d.deps(&["list"], json!({}))), json!({ "entries": [] }));
    std::fs::create_dir(d.dir.path().join("state")).unwrap();
    std::fs::write(&d.manifest, "{not json").unwrap();
    assert_eq!(parsed(&d.deps(&["list"], json!({}))), json!({ "entries": [] }));
    std::fs::write(&d.manifest, json!({ "entries": [{ "manager": "pip", "name": "x", "addedAt": "a" }] }).to_string()).unwrap();
    assert_eq!(parsed(&d.deps(&["list"], json!({}))), json!({ "entries": [] }));
    std::fs::remove_file(&d.manifest).unwrap();

    // When a dependency is added, Then it is stored with an ISO timestamp as two-space JSON; adding it again changes nothing
    let r = d.deps(&["add", "apt", "jq"], json!({}));
    assert_eq!(r.exit_code, 0);
    let out = parsed(&r);
    let entries = out["manifest"]["entries"].as_array().unwrap();
    assert_eq!((out["added"].clone(), entries.len(), entries[0]["manager"].as_str(), entries[0]["name"].as_str()), (json!(true), 1, Some("apt"), Some("jq")));
    assert!(Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$").unwrap().is_match(entries[0]["addedAt"].as_str().unwrap()));
    let stored = std::fs::read_to_string(&d.manifest).unwrap();
    assert!(serde_json::from_str::<Value>(&stored).unwrap() == out["manifest"] && stored.contains("\n  \"entries\": ["));
    assert_eq!(parsed(&d.deps(&["add", "apt", "jq"], json!({})))["added"], json!(false));
    assert_eq!(std::fs::read_to_string(&d.manifest).unwrap(), stored);

    // When one of two entries is removed, Then only it goes; removing it again reports false
    d.seed(&[("apt", "jq"), ("npm", "tsx")]);
    let gone = parsed(&d.deps(&["remove", "apt", "jq"], json!({})));
    assert_eq!((gone["removed"].clone(), gone["manifest"]["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect::<Vec<_>>()), (json!(true), vec!["tsx"]));
    assert_eq!(parsed(&d.deps(&["remove", "apt", "jq"], json!({})))["removed"], json!(false));
    assert_eq!(parsed(&d.deps(&["list"], json!({})))["entries"].as_array().unwrap().len(), 1);

    // Given an unknown manager, Then add and replay fail naming the valid ones before touching anything
    std::fs::remove_file(&d.manifest).unwrap();
    let r = d.deps(&["add", "pip", "requests"], json!({}));
    assert!(r.exit_code == 1 && r.stderr.contains("unknown manager \"pip\" -- expected one of: apt, npm, mise") && !d.manifest.exists(), "{}", r.stderr);
    d.seed(&[("npm", "tsx")]);
    let r = d.deps(&["replay", "--managers", "npm,pip"], json!({}));
    assert!(r.exit_code == 1 && r.stderr.contains("unknown manager \"pip\"") && d.calls().is_empty(), "{}", r.stderr);

    // Given every entry installed, Then replay installs nothing
    d.seed(&[("apt", "jq"), ("npm", "tsx"), ("mise", "node")]);
    std::fs::create_dir_all(d.prefix.join("lib/node_modules/tsx")).unwrap();
    let r = d.deps(&["replay"], json!({ "dpkg-query": { "stdout": "install ok installed" } }));
    assert_eq!((r.exit_code, outcomes(&r)), (0, vec!["already-present".to_string(), "already-present".into(), "already-present".into()]));
    assert!(d.calls().iter().all(|c| !Regex::new(r"^(apt-get|npm|mkdir)").unwrap().is_match(c)), "{:?}", d.calls());

    // Given every entry missing, Then each is installed with its own manager's command
    std::fs::remove_dir_all(&d.prefix).unwrap();
    let r = d.deps(&["replay"], json!({ "dpkg-query": { "exit": 1, "stderr": "no packages found" }, "mise": { "stdout": "" }, "mise which node": { "exit": 1, "stderr": "not found" } }));
    assert_eq!((r.exit_code, outcomes(&r)), (0, vec!["installed".to_string(), "installed".into(), "installed".into()]));
    assert_eq!(d.calls(), ["dpkg-query -W -f=${Status} jq", "mkdir -p /workspace/cache/apt", "apt-get install -y -o Dir::Cache::Archives=/workspace/cache/apt jq", "npm install -g tsx", "mise which node", "mise install node"]);
    // and --managers skips the other managers' entries entirely
    let r = d.deps(&["replay", "--managers", "npm"], json!({}));
    assert_eq!((r.exit_code, parsed(&r)["results"].as_array().unwrap().len(), d.calls()), (0, 1, vec!["npm install -g tsx".to_string()]));

    // Given one install fails, Then replay reports it, carries on and exits 1; a failure without stderr names the command
    d.seed(&[("npm", "broken"), ("npm", "fine")]);
    let r = d.deps(&["replay"], json!({ "npm install -g broken": { "exit": 1, "stderr": "E404 not found\n" } }));
    let results = parsed(&r)["results"].clone();
    assert_eq!((r.exit_code, results[0]["outcome"].as_str(), results[0]["detail"].as_str(), results[1]["outcome"].as_str()), (1, Some("failed"), Some("E404 not found"), Some("installed")));
    d.seed(&[("mise", "node")]);
    let r = d.deps(&["replay"], json!({ "mise": { "exit": 3 } }));
    assert_eq!((r.exit_code, parsed(&r)["results"][0]["detail"].clone()), (1, json!("Command failed: mise install node")));

    // Given an npm that really installs under NPM_CONFIG_PREFIX, Then a replay installs there and a second one finds it present
    let real = tempfile::Builder::new().prefix("wf-flow-deps-npm-").tempdir().unwrap();
    let bin = real.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("npm"), REAL_SHAPED_NPM).unwrap();
    std::fs::set_permissions(bin.join("npm"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let second_log = real.path().join("second-npm-calls.log");
    let run = |args: &[&str], log: Option<&Path>| {
        let mut full = vec!["vm", "deps"];
        full.extend_from_slice(args);
        // npm exports npm_config_prefix, which the stand-in would prefer over NPM_CONFIG_PREFIX
        let mut opts = RunOpts::default().bin_dir(&bin).env("AGENT_DEPS_MANIFEST_PATH", &real.path().join("packages.json").display().to_string()).env("NPM_CONFIG_PREFIX", &real.path().join("npm-prefix").display().to_string()).env("npm_config_prefix", "");
        if let Some(log) = log {
            opts = opts.env("FAKE_NPM_LOG", &log.display().to_string());
        }
        let r = run_wf(&full, &opts);
        assert_eq!(r.exit_code, 0, "wf {full:?}: {}{}", r.stdout, r.stderr);
    };
    run(&["add", "npm", "left-pad"], None);
    run(&["replay", "--managers", "npm"], None);
    assert!(real.path().join("npm-prefix/lib/node_modules/left-pad/package.json").exists());
    run(&["replay", "--managers", "npm"], Some(&second_log));
    assert!(!second_log.exists(), "the second replay installed again");
}

#[test]
fn vm_deps_install_installs_on_the_vm_and_records_what_it_installed() {
    let vm = FakeVm::start();
    let install = |args: &[&str], replies: &[String]| {
        vm.replies(replies);
        let mut full = vec!["vm", "deps", "install"];
        full.extend_from_slice(args);
        run_wf(&full, &vm.opts())
    };
    let script = || vm.requests()[0].body.clone();
    let ran = |n: u16, exit_code: i64, output: &str| {
        let job = answers::job_id(n);
        [answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(exit_code), output))]
    };

    // Given an apt package, Then one exec job installs it into the cache and records it, its output streamed
    let r = install(&["apt", "jq"], &ran(1, 0, "Setting up jq\n"));
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "Setting up jq\n"));
    assert!(r.stderr.contains(&format!("jobId: {}\n", answers::job_id(1))));
    assert_eq!(script(), "mkdir -p /workspace/cache/apt && apt-get install -y -o Dir::Cache::Archives=/workspace/cache/apt jq && wf vm deps add apt jq");
    assert_eq!(vm.requests()[1].url, format!("/services/agent-shell/exec-script/{}", answers::job_id(1)));

    // Given an npm package or a mise tool, Then the job installs it the manager's way and records it
    install(&["npm", "prettier@3"], &ran(2, 0, ""));
    assert_eq!(script(), "npm install -g prettier@3 && wf vm deps add npm prettier@3");
    install(&["mise", "node@22"], &ran(3, 0, ""));
    assert_eq!(script(), "mise install node@22 && wf vm deps add mise node@22");

    // Given an install that fails, Then its exit code is wf's
    let r = install(&["apt", "nope"], &ran(4, 100, "E: Unable to locate package nope\n"));
    assert_eq!((r.exit_code, r.stdout.as_str()), (100, "E: Unable to locate package nope\n"));

    // Given an unknown manager, a scoped npm name or a name with a space, Then nothing is sent
    for (args, message) in [(["brew", "jq"], "unknown manager \"brew\" -- expected one of: apt, npm, mise"), (["npm", "@scope/tool"], "invalid package/tool name: \"@scope/tool\""), (["apt", "jq vim"], "invalid package/tool name: \"jq vim\"")] {
        let r = install(&args, &[]);
        assert!(r.exit_code == 1 && r.stderr.contains(message), "{args:?}: {}", r.stderr);
        assert!(vm.requests().is_empty());
    }
}
