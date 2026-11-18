//! The host adapters: the verified provisioning of a pin, the hermetic environment with its stub agent CLIs, and a driver that starts a
//! server for a case, keeps it for the next and starts another after `reset`, against a fake `t3`.
use mimic_core::ports::{Driver, Request};
use mimic_script_core::Policy;
use mimic_script_host::hermetic::{refuse_candidate_only_steps, write_cli_script, Host, STUB_VERSION};
use mimic_script_host::pin::{sha256_hex, Pin};
use mimic_script_host::provision::{ensure_runtime, Get};
use mimic_script_host::target::SpawnLaunch;
use mimic_script_host::ScriptDriver;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fake_t3() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-t3"))
}

/// A release tarball holding the fake `t3`, as the pin's platform would.
fn tarball(dir: &Path) -> Vec<u8> {
    let inner = dir.join("t3-0.1.9");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::copy(fake_t3(), inner.join("t3")).unwrap();
    let out = dir.join("t3.tar.gz");
    assert!(Command::new("tar").arg("czf").arg(&out).arg("-C").arg(dir).arg("t3-0.1.9").status().unwrap().success());
    std::fs::read(out).unwrap()
}

struct FakeRelease {
    sums: String,
    tarball: Vec<u8>,
    gets: RefCell<Vec<String>>,
}

impl Get for FakeRelease {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        self.gets.borrow_mut().push(url.to_string());
        if url.ends_with("SHA256SUMS") { Ok(self.sums.clone().into_bytes()) } else { Ok(self.tarball.clone()) }
    }
}

fn pin_for(bytes: &[u8]) -> Pin {
    Pin::parse(&json!({"version": "0.1.9", "commit": "c", "release": "https://example.org/r", "platforms": {"linux-x64": {"bytes": bytes.len(), "sha256": sha256_hex(bytes)}}}).to_string()).unwrap()
}

#[test]
fn given_a_release_when_provisioned_then_it_is_verified_unpacked_once_and_not_fetched_again() {
    let work = tempfile::tempdir().unwrap();
    let bytes = tarball(work.path());
    let release = FakeRelease { sums: format!("{}  t3-0.1.9-linux-x64.tar.gz\n", sha256_hex(&bytes)), tarball: bytes.clone(), gets: RefCell::default() };
    let cache = work.path().join("cache");

    let first = ensure_runtime(&pin_for(&bytes), "linux-x64", &cache, &release).unwrap();
    let second = ensure_runtime(&pin_for(&bytes), "linux-x64", &cache, &release).unwrap();

    assert!(first.downloaded && !second.downloaded);
    assert!(first.dir.join("t3").exists());
    assert_eq!(release.gets.borrow().len(), 2);
}

#[test]
fn given_bytes_or_sums_that_disagree_with_the_pin_when_provisioned_then_nothing_is_unpacked() {
    let work = tempfile::tempdir().unwrap();
    let bytes = tarball(work.path());
    let wrong_sums = FakeRelease { sums: format!("{}  t3-0.1.9-linux-x64.tar.gz\n", "0".repeat(64)), tarball: bytes.clone(), gets: RefCell::default() };
    let wrong_bytes = FakeRelease { sums: format!("{}  t3-0.1.9-linux-x64.tar.gz\n", sha256_hex(&bytes)), tarball: b"other bytes".to_vec(), gets: RefCell::default() };

    let a = ensure_runtime(&pin_for(&bytes), "linux-x64", &work.path().join("c1"), &wrong_sums).unwrap_err();
    let b = ensure_runtime(&pin_for(&bytes), "linux-x64", &work.path().join("c2"), &wrong_bytes).unwrap_err();

    assert!(a.contains("SHA256SUMS of the release disagrees"), "{a}");
    assert!(b.contains("does not match the pin"), "{b}");
    assert!(!work.path().join("c1/runtime-0.1.9-linux-x64").exists() && !work.path().join("c2/runtime-0.1.9-linux-x64").exists());
}

#[test]
fn given_a_prepared_host_when_the_agent_clis_are_asked_their_version_then_they_answer_the_same_everywhere_and_nothing_else() {
    let root = tempfile::tempdir().unwrap();

    let host = Host::prepare(root.path(), None, None).unwrap();

    for name in ["claude", "codex", "opencode"] {
        let out = Command::new(host.bin.join(name)).arg("--version").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("{name} {STUB_VERSION}"));
        assert!(!Command::new(host.bin.join(name)).arg("chat").status().unwrap().success());
    }
    let env: std::collections::BTreeMap<_, _> = host.env.iter().cloned().collect();
    assert!(env["PATH"].starts_with(&format!("{}:", host.bin.display())));
    assert_eq!((env["TZ"].as_str(), env["LANG"].as_str()), ("UTC", "C.UTF-8"));
}

fn driver(runtime: &Path) -> ScriptDriver {
    ScriptDriver::new(Box::new(SpawnLaunch { runtime: runtime.to_path_buf(), ready_ms: 20_000, cli_dir: None, real_clis: None, env: vec![] }), Policy { settle_ms: 20, ..Policy::default() }, vec![], false, Some("0.1.9".into()))
}

fn ask(driver: &mut ScriptDriver, id: &str, script: Value) -> Vec<Value> {
    let observed = driver.run(&Request { id: id.into(), op: Some("script".into()), input: json!({"script": script}) }).unwrap();
    observed.steps.into_iter().map(|s| s.payload).collect()
}

/// A directory of scripted agent CLIs as a test leaves one: the programs, and a script and a log of an earlier case.
fn scripted_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("codex"), "#!/bin/sh\n").unwrap();
    std::fs::write(dir.path().join("codex.script.json"), r#"{"turns":[{"hang":true}]}"#).unwrap();
    std::fs::write(dir.path().join("codex.invocations.jsonl"), "{}\n").unwrap();
    dir
}

#[test]
fn given_scripted_clis_when_the_host_is_prepared_then_its_own_copy_comes_before_the_stubs_on_the_path_without_the_earlier_script_or_log() {
    let root = tempfile::tempdir().unwrap();
    let scripted = scripted_dir();

    let host = Host::prepare(root.path(), Some(scripted.path()), None).unwrap();

    let cli = host.cli.clone().unwrap();
    let path = host.env.iter().find(|(k, _)| k == "PATH").unwrap().1.clone();
    assert!(path.starts_with(&format!("{}:{}:", cli.display(), host.bin.display())), "{path}");
    let mut files: Vec<String> = std::fs::read_dir(&cli).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files, ["codex"]);
}

/// A directory of real agent CLIs as an install leaves it: one program, `opencode`.
fn real_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("opencode"), "#!/bin/sh\necho real-opencode\n").unwrap();
    dir
}

#[test]
fn given_a_real_opencode_when_the_host_is_prepared_then_opencode_is_neither_stubbed_nor_scripted_and_claude_and_codex_still_are() {
    let root = tempfile::tempdir().unwrap();
    let scripted = scripted_dir();
    std::fs::write(scripted.path().join("opencode"), "#!/bin/sh\necho scripted\n").unwrap();
    let real = real_dir();

    let host = Host::prepare(root.path(), Some(scripted.path()), Some(real.path())).unwrap();

    let path = host.env.iter().find(|(k, _)| k == "PATH").unwrap().1.clone();
    assert!(path.starts_with(&format!("{}:{}:{}:", host.cli.as_ref().unwrap().display(), host.bin.display(), real.path().display())), "{path}");
    assert!(!host.bin.join("opencode").exists() && !host.cli.as_ref().unwrap().join("opencode").exists());
    assert!(host.bin.join("claude").exists() && host.bin.join("codex").exists());
    assert!(host.cli.as_ref().unwrap().join("codex").exists());
}

#[test]
fn given_a_real_clis_directory_that_is_missing_when_the_host_is_prepared_then_it_fails_naming_the_directory() {
    let root = tempfile::tempdir().unwrap();

    let err = Host::prepare(root.path(), None, Some(Path::new("/nonexistent/real-clis"))).unwrap_err();

    assert!(err.contains("/nonexistent/real-clis"), "{err}");
}

#[test]
fn given_two_hosts_from_one_directory_when_one_is_scripted_then_the_other_does_not_see_the_script_and_the_version_line_is_forced() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let scripted = scripted_dir();
    let (first, second) = (Host::prepare(a.path(), Some(scripted.path()), None).unwrap(), Host::prepare(b.path(), Some(scripted.path()), None).unwrap());

    write_cli_script(first.cli.as_ref().unwrap(), "codex", &json!([{"hang": true}])).unwrap();

    let script: Value = serde_json::from_str(&std::fs::read_to_string(first.cli.as_ref().unwrap().join("codex.script.json")).unwrap()).unwrap();
    assert_eq!((script["turns"].clone(), script["version"].as_str()), (json!([{"hang": true}]), Some(format!("codex {STUB_VERSION}").as_str())));
    assert!(!second.cli.as_ref().unwrap().join("codex.script.json").exists());
    assert_eq!(std::fs::read_to_string(scripted.path().join("codex.script.json")).unwrap(), r#"{"turns":[{"hang":true}]}"#);
}

#[test]
fn given_a_reference_host_when_a_turn_calls_a_tool_or_writes_a_credential_then_it_is_refused_and_a_protocol_free_turn_is_not() {
    let hang = json!([{"hang": true}, {"exit_code": 1, "stderr": "no"}]);
    let tool = json!([{"steps": [{"step": "mcp_call", "id": "1", "tool": "write_file", "args": {}}]}]);
    let login = json!([{"steps": [{"step": "credential", "env": "X", "file": "f", "content": "c"}]}]);

    assert_eq!(refuse_candidate_only_steps(&hang), Ok(()));
    assert!(refuse_candidate_only_steps(&tool).unwrap_err().contains("`mcp_call`"));
    assert!(refuse_candidate_only_steps(&login).unwrap_err().contains("`credential`"));
}

#[test]
fn given_a_driver_when_cases_run_then_one_server_serves_them_in_a_hermetic_environment_and_reset_starts_another() {
    let work = tempfile::tempdir().unwrap();
    let runtime = work.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    // a symlink, not a copy: a copy still being written when another test forks cannot then be executed (ETXTBSY)
    std::os::unix::fs::symlink(fake_t3(), runtime.join("t3")).unwrap();
    let mut driver = driver(&runtime);
    let show = json!([{"http": {"path": "/environment-of-server"}}]);

    let first = ask(&mut driver, "a", show.clone());
    let again = ask(&mut driver, "b", show.clone());
    driver.reset().unwrap();
    let after = ask(&mut driver, "c", show);

    let seen = |steps: &[Value]| steps[1]["body"].as_str().unwrap().to_string();
    assert_eq!(seen(&first), seen(&again));
    assert_ne!(seen(&first), seen(&after));
    let environment = seen(&first);
    assert!(environment.contains("TZ=UTC") && environment.contains("ARGS=serve --host 127.0.0.1 --port") && environment.contains("--no-browser <workspace>"), "{environment}");
    assert!(environment.contains("/bin:/usr/bin:/bin"), "{environment}");
}

#[test]
fn given_an_agent_key_when_a_server_is_spawned_then_the_server_holds_it_and_a_server_without_one_does_not() {
    let work = tempfile::tempdir().unwrap();
    let runtime = work.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    std::os::unix::fs::symlink(fake_t3(), runtime.join("t3")).unwrap();
    let real = real_dir();
    let show = json!([{"http": {"path": "/environment-of-server"}}]);
    let spawn = |env: Vec<(String, String)>| {
        let launch = SpawnLaunch { runtime: runtime.clone(), ready_ms: 20_000, cli_dir: None, real_clis: Some(real.path().to_path_buf()), env };
        ScriptDriver::new(Box::new(launch), Policy { settle_ms: 20, ..Policy::default() }, vec![], false, None)
    };

    let with = ask(&mut spawn(vec![("OPENCODE_API_KEY".into(), "k-1234567".into())]), "a", show.clone());
    let without = ask(&mut spawn(vec![]), "b", show);

    let body = |steps: &[Value]| steps[1]["body"].as_str().unwrap().to_string();
    assert!(body(&with).contains("AGENT_KEY=k-1234567") && body(&with).contains(&real.path().display().to_string()), "{}", body(&with));
    assert!(body(&without).contains("AGENT_KEY=unset"), "{}", body(&without));
}

#[test]
fn given_a_cli_script_when_a_case_runs_on_a_spawned_server_then_the_script_is_written_into_its_host_and_no_step_is_recorded() {
    let work = tempfile::tempdir().unwrap();
    let runtime = work.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    std::os::unix::fs::symlink(fake_t3(), runtime.join("t3")).unwrap();
    let scripted = scripted_dir();
    let launch = SpawnLaunch { runtime, ready_ms: 20_000, cli_dir: Some(scripted.path().to_path_buf()), real_clis: None, env: vec![] };
    let mut driver = ScriptDriver::new(Box::new(launch), Policy { settle_ms: 20, ..Policy::default() }, vec![], false, None);

    let steps = ask(&mut driver, "a", json!([{"cli.script": {"cli": "codex", "turns": [{"hang": true}]}}, {"http": {"path": "/environment-of-server"}}]));
    let refused = ask(&mut driver, "b", json!([{"cli.script": {"cli": "codex", "turns": [{"steps": [{"step": "mcp_call", "id": "1", "tool": "t", "args": {}}]}]}}]));

    assert_eq!(steps.len(), 2, "only the http call and its answer are steps: {steps:?}");
    assert!(refused[0]["transport_error"].as_str().is_some_and(|e| e.contains("`mcp_call`")), "{refused:?}");
}

#[test]
fn given_a_script_that_names_a_credential_when_run_then_the_server_mints_one_and_it_is_filled() {
    let work = tempfile::tempdir().unwrap();
    let runtime = work.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    // a symlink, not a copy: a copy still being written when another test forks cannot then be executed (ETXTBSY)
    std::os::unix::fs::symlink(fake_t3(), runtime.join("t3")).unwrap();
    let mut driver = driver(&runtime);

    let steps = ask(&mut driver, "a", json!([{"http": {"method": "POST", "path": "/oauth/token", "body": "{credential}"}}]));

    assert!(steps[0]["body"].as_str().unwrap().starts_with("cred-"), "{}", steps[0]);
    assert!(steps[1]["body"].as_str().unwrap().contains("tok-for-cred-"), "{}", steps[1]);
}

mod phone_server {
    use mimic_core::cassette::{Dir, Step, StepKind};
    use mimic_core::ports::{Driver, Request};
    use mimic_script_core::testing::CassettePhone;
    use mimic_script_core::{Phone, Policy};
    use mimic_script_host::driver::{Launch, Server};
    use mimic_script_host::ScriptDriver;
    use serde_json::{json, Value};
    use std::net::SocketAddr;
    use std::rc::Rc;

    /// A server reached through a phone, as Orca's is: no address anyone connects to, repositories under a scratch folder.
    struct Phoned;

    impl Server for Phoned {
        fn addr(&self) -> SocketAddr {
            "127.0.0.1:0".parse().unwrap()
        }
        fn workspace(&self) -> String {
            String::new()
        }
        fn mint(&self) -> Result<String, String> {
            Err("no credential".into())
        }
        fn stop(&self) {}
        fn phone(&self) -> Option<Box<dyn Phone>> {
            let answer = Step { dir: Dir::S2c, channel: None, kind: StepKind::Json, rid: Some("0".into()), len: None, payload: json!({"ok": true, "result": {"seen": 1}}) };
            Some(Box::new(CassettePhone::new(&[answer])))
        }
        fn make_git(&self, name: &str) -> Result<String, String> {
            Ok(format!("/scratch-7/{name}"))
        }
        fn masks(&self) -> Vec<(String, String)> {
            vec![("/scratch-7".into(), "<scratch>".into())]
        }
    }

    struct Once;

    impl Launch for Once {
        fn start(&mut self) -> Result<Rc<dyn Server>, String> {
            Ok(Rc::new(Phoned))
        }
    }

    #[test]
    fn given_a_server_with_a_phone_when_an_orca_case_runs_then_the_request_goes_through_the_phone_and_the_scratch_path_is_masked() {
        let mut driver = ScriptDriver::new(Box::new(Once), Policy::default(), Vec::new(), false, None);
        let input = json!({"requests": [{"method": "repo.add", "params": {"path": "{git.main}"}}], "setup": [{"git": "main"}]});

        let observed = driver.run(&Request { id: "c1".into(), op: Some("script".into()), input }).unwrap();

        let payloads: Vec<Value> = observed.steps.iter().map(|s| s.payload.clone()).collect();
        assert_eq!(payloads, [json!({"method": "repo.add", "params": {"path": "<scratch>/main"}}), json!({"ok": true, "result": {"seen": 1}})]);
    }
}

#[test]
fn given_a_credential_named_variable_in_the_caller_when_a_url_target_mints_then_the_helper_prints_only_allowed_names() {
    // Given a caller holding a fake credential and a url target whose mint command prints its environment
    std::env::set_var("MIMIC_FLOW_FAKE_API_KEY", "must-not-leak");
    let mut launch = mimic_script_host::target::UrlLaunch {
        addr: "127.0.0.1:1".parse().unwrap(),
        workspace: "w".into(),
        mint: vec!["env".into()],
        cli_dir: None,
    };
    let server = mimic_script_host::Launch::start(&mut launch).unwrap();

    // When it mints
    let printed = server.mint().unwrap();

    // Then every name printed is allowed and the credential is not among them
    assert!(!printed.contains("MIMIC_FLOW_FAKE_API_KEY") && !printed.contains("must-not-leak"), "{printed}");
    let allowed = ["PATH", "HOME", "USER", "LOGNAME", "TERM", "TZ", "LANG", "LANGUAGE", "TMPDIR", "TEMP", "TMP", "SSL_CERT_FILE", "SSL_CERT_DIR", "CURL_CA_BUNDLE", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy", "_", "PWD", "SHLVL", "OLDPWD"];
    for line in printed.lines() {
        let name = line.split('=').next().unwrap();
        assert!(allowed.contains(&name) || name.starts_with("LC_"), "unexpected {name}");
    }
}

#[test]
fn given_a_curl_and_a_tar_helper_when_started_then_they_get_the_allowed_part_of_the_environment_only() {
    // Given an environment with a credential, a path and a proxy
    let vars = [("PATH", "/bin"), ("OPENCODE_API_KEY", "x"), ("BWS_ACCESS_TOKEN", "y"), ("HTTPS_PROXY", "p"), ("LC_ALL", "C")].map(|(k, v)| (k.to_string(), v.to_string()));

    // When the helper environment is built
    let got: Vec<String> = mimic_script_host::child_env::minimal_from(vars).into_iter().map(|(k, _)| k).collect();

    // Then only the allowed names remain
    assert_eq!(got, ["PATH", "HTTPS_PROXY", "LC_ALL"]);
}

fn seen_by(host: &Host, cli: &str) -> String {
    let out = std::process::Command::new("sh").arg("-c").arg(cli).env_clear().envs(host.env.iter().cloned()).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn given_a_credential_per_real_cli_when_each_cli_runs_through_the_host_then_it_sees_its_own_variable_and_no_other_cli_credential() {
    let root = tempfile::tempdir().unwrap();
    let real = tempfile::tempdir().unwrap();
    for cli in ["claude", "opencode"] {
        std::fs::write(real.path().join(cli), "#!/bin/sh\nenv | sort\n").unwrap();
        set_exec(&real.path().join(cli));
    }
    let mut host = Host::prepare(root.path(), None, Some(real.path())).unwrap();
    host.env.push(("MIMIC_CRED_CLAUDE__CLAUDE_CODE_OAUTH_TOKEN".into(), "claude-secret-value".into()));
    host.env.push(("MIMIC_CRED_OPENCODE__OPENCODE_API_KEY".into(), "opencode-secret-value".into()));

    host.wrap_credentials().unwrap();

    let claude = seen_by(&host, "claude");
    assert!(claude.contains("CLAUDE_CODE_OAUTH_TOKEN=claude-secret-value"), "{claude}");
    assert!(!claude.contains("opencode-secret-value") && !claude.contains("MIMIC_CRED_"), "claude saw another credential");
    let opencode = seen_by(&host, "opencode");
    assert!(opencode.contains("OPENCODE_API_KEY=opencode-secret-value"), "{opencode}");
    assert!(!opencode.contains("claude-secret-value") && !opencode.contains("MIMIC_CRED_") && !opencode.contains("CLAUDE_CODE_OAUTH_TOKEN"), "opencode saw another credential");
}

#[test]
fn given_a_credential_for_a_cli_that_is_not_real_when_the_wrappers_are_made_then_it_fails_and_no_wrapper_holds_the_value() {
    let root = tempfile::tempdir().unwrap();
    let real = real_dir();
    let mut host = Host::prepare(root.path(), None, Some(real.path())).unwrap();
    host.env.push(("MIMIC_CRED_CLAUDE__CLAUDE_CODE_OAUTH_TOKEN".into(), "claude-secret-value".into()));

    let err = host.wrap_credentials().unwrap_err();

    assert!(err.contains("claude") && !err.contains("claude-secret-value"), "{err}");
}

fn set_exec(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn given_a_codex_login_when_codex_runs_through_the_host_then_it_reads_a_private_codex_home_that_is_gone_afterwards_and_no_credential_variable() {
    let root = tempfile::tempdir().unwrap();
    let real = tempfile::tempdir().unwrap();
    std::fs::write(real.path().join("codex"), "#!/bin/sh\necho \"home=$CODEX_HOME\"\necho \"login=$(cat \"$CODEX_HOME/auth.json\")\"\nstat -c 'mode=%a' \"$CODEX_HOME/auth.json\"\nenv | sort\n").unwrap();
    set_exec(&real.path().join("codex"));
    let mut host = Host::prepare(root.path(), None, Some(real.path())).unwrap();
    let login = base64_of("{\"tokens\":{\"refresh_token\":\"rt-secret-value\"}}");
    host.env.push(("MIMIC_CRED_CODEX__CODEX_OAUTH_JSON_B64".into(), login.clone()));
    host.wrap_credentials().unwrap();

    let seen = seen_by(&host, "codex");

    assert!(seen.contains("login={\"tokens\":{\"refresh_token\":\"rt-secret-value\"}}"), "{seen}");
    assert!(seen.contains("mode=600"), "{seen}");
    assert!(!seen.contains("MIMIC_CRED_") && !seen.contains("CODEX_OAUTH_JSON_B64"), "codex kept the carrier variable");
    let home = seen.lines().find_map(|l| l.strip_prefix("home=")).unwrap();
    assert!(!Path::new(home).exists(), "the login directory {home} outlived the process");
}

fn base64_of(text: &str) -> String {
    let out = std::process::Command::new("sh").arg("-c").arg(format!("printf %s '{text}' | base64 -w0")).output().unwrap();
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn given_a_real_cli_when_the_host_is_prepared_then_its_workspace_is_a_repository_with_one_commit_and_a_scripted_host_keeps_an_empty_workspace() {
    let real = real_dir();
    let real_root = tempfile::tempdir().unwrap();
    let scripted_root = tempfile::tempdir().unwrap();

    let real_host = Host::prepare(real_root.path(), None, Some(real.path())).unwrap();
    let scripted_host = Host::prepare(scripted_root.path(), None, None).unwrap();

    let log = std::process::Command::new("git").args(["log", "--oneline"]).current_dir(&real_host.workspace).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).lines().count(), 1, "{}", String::from_utf8_lossy(&log.stderr));
    assert!(std::fs::read_dir(&scripted_host.workspace).unwrap().next().is_none(), "a scripted host's workspace must stay empty");
}
