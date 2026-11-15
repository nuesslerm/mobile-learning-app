//! The host adapters: the verified provisioning of a pin, the hermetic environment with its stub agent CLIs, and a driver that starts a
//! server for a case, keeps it for the next and starts another after `reset`, against a fake `t3`.
use mimic_core::ports::{Driver, Request};
use mimic_script_core::Policy;
use mimic_script_host::hermetic::{Host, STUB_VERSION};
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

    let host = Host::prepare(root.path(), None).unwrap();

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
    ScriptDriver::new(Box::new(SpawnLaunch { runtime: runtime.to_path_buf(), ready_ms: 20_000, cli_dir: None }), Policy { settle_ms: 20, ..Policy::default() }, vec![], false, Some("0.1.9".into()))
}

fn ask(driver: &mut ScriptDriver, id: &str, script: Value) -> Vec<Value> {
    let observed = driver.run(&Request { id: id.into(), op: Some("script".into()), input: json!({"script": script}) }).unwrap();
    observed.steps.into_iter().map(|s| s.payload).collect()
}

#[test]
fn given_scripted_clis_when_the_host_is_prepared_then_they_come_before_the_stubs_on_the_path() {
    let root = tempfile::tempdir().unwrap();
    let scripted = tempfile::tempdir().unwrap();

    let host = Host::prepare(root.path(), Some(scripted.path())).unwrap();

    let path = host.env.iter().find(|(k, _)| k == "PATH").unwrap().1.clone();
    assert!(path.starts_with(&format!("{}:{}:", scripted.path().display(), host.bin.display())), "{path}");
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
