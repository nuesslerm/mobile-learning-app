//! `wf parity track` through the thin binary: the pins and cassette sets are the repository's own, the newest release is a local server.
use super::support::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// The repository's pins and cassette sets in a temp root, so a bump rewrites a copy.
fn root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for pin in ["tooling/orca-conformance/orca-pin.json", "tooling/t3-conformance/t3-pin.json"] {
        let to = dir.path().join(pin);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(repo().join(pin), to).unwrap();
    }
    for crate_dir in ["t3-serve", "orca-serve"] {
        let cassettes = repo().join("clis").join(crate_dir).join("cassettes");
        for set in std::fs::read_dir(cassettes).unwrap().flatten() {
            let toml = set.path().join("mimic.toml");
            if let Ok(text) = std::fs::read_to_string(&toml) {
                let to = dir.path().join("clis").join(crate_dir).join("cassettes").join(set.file_name());
                std::fs::create_dir_all(&to).unwrap();
                std::fs::write(to.join("mimic.toml"), text).unwrap();
            }
        }
    }
    dir
}

fn pin(root: &Path, app: &str) -> Value {
    let file = if app == "t3" { "tooling/t3-conformance/t3-pin.json" } else { "tooling/orca-conformance/orca-pin.json" };
    serde_json::from_str(&std::fs::read_to_string(root.join(file)).unwrap()).unwrap()
}

/// The version after `version`: the last number plus one.
fn next_version(version: &str) -> String {
    let mut parts: Vec<u64> = version.split('.').map(|p| p.parse().unwrap()).collect();
    *parts.last_mut().unwrap() += 1;
    parts.iter().map(u64::to_string).collect::<Vec<_>>().join(".")
}

/// A server answering fixed bodies by path; a path it does not know is 404. Every path asked is kept.
struct Upstream {
    base: String,
    asked: Arc<std::sync::Mutex<Vec<String>>>,
}

fn serve(answers: BTreeMap<String, String>) -> Upstream {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let asked = Arc::new(std::sync::Mutex::new(vec![]));
    let log = asked.clone();
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let path = request.url().to_string();
            log.lock().unwrap().push(path.clone());
            let _ = match answers.get(&path) {
                Some(body) => request.respond(tiny_http::Response::from_string(body.clone())),
                None => request.respond(tiny_http::Response::from_string("not found").with_status_code(404)),
            };
        }
    });
    Upstream { base, asked }
}

/// Like `serve`, but the first `fail` requests of every path answer 503.
fn serve_flaky(answers: BTreeMap<String, String>, fail: usize) -> Upstream {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let asked = Arc::new(std::sync::Mutex::new(vec![]));
    let log = asked.clone();
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let path = request.url().to_string();
            let seen = log.lock().unwrap().iter().filter(|p| **p == path).count();
            log.lock().unwrap().push(path.clone());
            let _ = match answers.get(&path) {
                Some(_) if seen < fail => request.respond(tiny_http::Response::from_string("busy").with_status_code(503)),
                Some(body) => request.respond(tiny_http::Response::from_string(body.clone())),
                None => request.respond(tiny_http::Response::from_string("not found").with_status_code(404)),
            };
        }
    });
    Upstream { base, asked }
}

fn orca_release(from: &Value) -> (String, BTreeMap<String, String>) {
    let next = next_version(from["version"].as_str().unwrap());
    let repo = from["repository"].as_str().unwrap().trim_start_matches("https://github.com/").to_string();
    let answers = [(format!("/repos/{repo}/releases/latest"), format!(r#"{{"tag_name": "v{next}"}}"#)), (format!("/repos/{repo}/commits/v{next}"), r#"{"sha": "0123456789abcdef0123456789abcdef01234567"}"#.to_string())];
    (next, answers.into())
}

fn t3_release(from: &Value) -> (String, BTreeMap<String, String>) {
    let next = next_version(from["version"].as_str().unwrap());
    let mut sums = String::new();
    let mut assets = vec![];
    for (i, platform) in from["platforms"].as_object().unwrap().keys().enumerate() {
        sums += &format!("{:064x}  t3-{next}-{platform}.tar.gz\n", i + 1);
        assets.push(json!({"name": format!("t3-{next}-{platform}.tar.gz"), "size": 1000 + i}));
    }
    let answers = [
        ("/t3/latest".to_string(), json!({"version": next}).to_string()),
        (format!("/t3/{next}"), json!({"dist": {"integrity": "sha512-next"}}).to_string()),
        (format!("/repos/pingdotgg/t3code/commits/v{next}"), r#"{"sha": "fedcba9876543210fedcba9876543210fedcba98"}"#.to_string()),
        (format!("/repos/pingdotgg/t3code/releases/tags/v{next}"), json!({"assets": assets}).to_string()),
        (format!("/pingdotgg/t3code/releases/download/v{next}/SHA256SUMS"), sums),
    ];
    (next, answers.into())
}

fn track(root: &Path, upstream: &str, args: &[&str]) -> Run {
    let mut cmd = Command::new(binary());
    cmd.args(["parity", "track"]).args(args).arg("--root").arg(root).current_dir(root);
    for var in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY", "all_proxy", "GITHUB_TOKEN"] {
        cmd.env_remove(var);
    }
    cmd.env("WF_TRACK_RETRY_MS", "1").env("NO_PROXY", "127.0.0.1").env("no_proxy", "127.0.0.1").env("WF_TRACK_NPM", upstream).env("WF_TRACK_GITHUB_API", upstream).env("WF_TRACK_GITHUB_WEB", upstream);
    let out = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = stdout.lines().rev().find(|l| l.starts_with('{')).unwrap_or("null");
    let json = serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {stdout}\n{}", String::from_utf8_lossy(&out.stderr)));
    Run { code: out.status.code().unwrap_or(-1), json, stdout }
}

#[test]
fn a_new_orca_release_is_drift_until_the_pin_is_bumped_to_it() {
    // Given the repository's Orca pin and a release one patch version newer
    let dir = root();
    let before = pin(dir.path(), "orca");
    let (next, answers) = orca_release(&before);
    let upstream = serve(answers);
    // When it is checked
    let drifted = track(dir.path(), &upstream.base, &["check", "--app", "orca"]);
    // Then the verdict is drifted, from the pinned version to the new one
    assert_eq!(drifted.code, 0, "{}", drifted.stdout);
    assert_eq!((drifted.json["app"].as_str(), drifted.json["pinned"].as_str(), drifted.json["latest"].as_str(), drifted.json["drifted"].as_bool()), (Some("orca"), before["version"].as_str(), Some(next.as_str()), Some(true)));
    // When the pin is bumped
    let bumped = track(dir.path(), &upstream.base, &["bump", "--app", "orca"]);
    // Then the pin file names the new version and commit and keeps its repository, and the report lists what to re-record
    assert_eq!(bumped.code, 0, "{}", bumped.stdout);
    let after = pin(dir.path(), "orca");
    assert_eq!((after["version"].as_str(), after["commit"].as_str(), &after["repository"]), (Some(next.as_str()), Some("0123456789abcdef0123456789abcdef01234567"), &before["repository"]));
    assert!(bumped.json["rerecord"].as_array().unwrap().iter().any(|s| s == "orca-recorded"), "{}", bumped.stdout);
    assert!(bumped.json["stale"].as_array().is_some_and(|s| !s.is_empty()));
    // And a check after it is no longer drift
    let level = track(dir.path(), &upstream.base, &["check", "--app", "orca"]);
    assert_eq!(level.json["drifted"], false, "{}", level.stdout);
}

#[test]
fn a_new_t3_release_is_bumped_with_the_hashes_its_sums_file_gives() {
    // Given the repository's T3 pin and a newer npm release whose sums list each platform the pin names
    let dir = root();
    let before = pin(dir.path(), "t3");
    let (next, answers) = t3_release(&before);
    let upstream = serve(answers);
    // When it is bumped
    let bumped = track(dir.path(), &upstream.base, &["bump", "--app", "t3"]);
    // Then each platform of the pin has the size and sha256 the release gives, and the version, commit and integrity moved
    assert_eq!(bumped.code, 0, "{}", bumped.stdout);
    let after = pin(dir.path(), "t3");
    assert_eq!((after["version"].as_str(), after["commit"].as_str(), after["npm"]["integrity"].as_str()), (Some(next.as_str()), Some("fedcba9876543210fedcba9876543210fedcba98"), Some("sha512-next")));
    for (i, platform) in before["platforms"].as_object().unwrap().keys().enumerate() {
        assert_eq!(after["platforms"][platform], json!({"bytes": 1000 + i, "sha256": format!("{:064x}", i + 1)}), "{platform}");
    }
    // And what only a person can set is untouched and listed for review
    assert_eq!((after["protocol"].clone(), after["conformedStable"].clone()), (before["protocol"].clone(), before["conformedStable"].clone()));
    assert!(bumped.json["review"].as_array().is_some_and(|r| !r.is_empty()));
    assert!(upstream.asked.lock().unwrap().iter().any(|p| p == &format!("/t3/{next}")));
}

#[test]
fn an_unreachable_service_is_a_failure_with_its_url_and_leaves_the_pin_alone() {
    // Given a service that is not listening
    let dir = root();
    let before = std::fs::read_to_string(dir.path().join("tooling/orca-conformance/orca-pin.json")).unwrap();
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    // When the pin is bumped
    let run = track(dir.path(), &dead, &["bump", "--app", "orca"]);
    // Then it fails with the URL it asked, and the pin file is as it was
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert_eq!(run.json["ok"], false);
    assert!(run.json["error"].as_str().unwrap().contains(&dead), "{}", run.stdout);
    assert_eq!(std::fs::read_to_string(dir.path().join("tooling/orca-conformance/orca-pin.json")).unwrap(), before);
}

#[test]
fn the_plan_of_a_marker_records_the_sets_of_the_drifted_app_only() {
    // Given a service with a newer Orca and the pinned T3
    let dir = root();
    let (_, mut answers) = orca_release(&pin(dir.path(), "orca"));
    let t3 = pin(dir.path(), "t3");
    answers.insert("/t3/latest".into(), json!({"version": t3["version"]}).to_string());
    let upstream = serve(answers);
    // When the plan of a commit message with a track marker for both is read
    let message = "chore: track\n\n[track t3 orca chunk=20]\n";
    let run = track(dir.path(), &upstream.base, &["plan", "--message", message]);
    // Then Orca is the drifted one and the record marker names sets whose mimic.toml takes Orca's provisioning, with the chunk
    assert_eq!(run.code, 0, "{}", run.stdout);
    assert_eq!(run.json["drifted"], json!(["orca"]));
    let marker = run.json["record_marker"].as_str().unwrap();
    let words: Vec<&str> = marker.trim_matches(|c| c == '[' || c == ']').split(' ').collect();
    assert_eq!((words[0], *words.last().unwrap()), ("record", "chunk=20"), "{marker}");
    for set in &words[1..words.len() - 1] {
        let toml = std::fs::read_to_string(dir.path().join("clis/orca-serve/cassettes").join(set).join("mimic.toml")).unwrap();
        assert!(toml.contains("provision/orca.toml"), "{set}");
    }
    assert!(words.len() > 2);
    // And a message without the marker starts nothing and asks nothing
    let before = upstream.asked.lock().unwrap().len();
    let quiet = track(dir.path(), &upstream.base, &["plan", "--message", "fix: a mention of [track orca] mid-line"]);
    assert_eq!((quiet.json["go"].as_bool(), quiet.json["record_marker"].is_null()), (Some(false), true));
    assert_eq!(upstream.asked.lock().unwrap().len(), before);
}

#[test]
fn a_malformed_marker_is_refused_with_the_reason() {
    // Given a marker that names an app that does not exist
    let dir = root();
    let run = track(dir.path(), "http://127.0.0.1:9", &["plan", "--message", "[track t4]"]);
    // Then the plan fails and says which word it could not read
    assert_eq!(run.code, 1, "{}", run.stdout);
    assert!(run.json["error"].as_str().unwrap().contains("t4"), "{}", run.stdout);
}

#[test]
fn a_tracker_run_folds_into_one_drift_report_and_a_publishable_run_dir() {
    // Given the plan of a drifted Orca (the plan the binary printed), its sets recorded, and one replay that differs
    let dir = root();
    let (_, answers) = orca_release(&pin(dir.path(), "orca"));
    let upstream = serve(answers);
    let plan = track(dir.path(), &upstream.base, &["plan", "--message", "[track orca]"]);
    let sets: Vec<String> = plan.json["sets"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    assert!(!sets.is_empty(), "{}", plan.stdout);
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("plan.json"), plan.json.to_string()).unwrap();
    let inputs = work.path().join("inputs");
    std::fs::create_dir_all(&inputs).unwrap();
    for set in &sets {
        std::fs::write(inputs.join(format!("{set}.record.json")), json!({"ok": true, "complete": true}).to_string()).unwrap();
    }
    let replay = json!({"ok": false, "candidates": [{"candidate": "orca-serve", "severity": "drift", "summary": {"total": 9, "clean": 8, "allowlisted": 0, "drift": 1, "breaking": 0}}]});
    std::fs::write(inputs.join(format!("{}.replay.json", sets[0])), replay.to_string()).unwrap();
    // When the report is built
    let out = work.path().join("report");
    let report = track(dir.path(), &upstream.base, &["report", "--plan", work.path().join("plan.json").to_str().unwrap(), "--inputs", inputs.to_str().unwrap(), "--out", out.to_str().unwrap(), "--run", "7", "--sha", "abc123"]);
    // Then Orca is a list of discrepancies naming the replayed set, and the run is ok
    assert_eq!(report.code, 0, "{}", report.stdout);
    assert_eq!((report.json["apps"][0]["app"].as_str(), report.json["apps"][0]["status"].as_str(), report.json["ok"].as_bool()), (Some("orca"), Some("discrepancies"), Some(true)));
    assert_eq!(report.json["apps"][0]["sets"][0]["replay"]["candidates"][0]["drift"], 1);
    // And the out dir is a flow report dir a run bundle can be made of, with its page and summary
    let flow: Value = serde_json::from_str(&std::fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    assert_eq!((flow["schema"].as_str(), flow["verdict"].as_str()), (Some("flow-report/1"), Some("passed")));
    for file in ["report.html", "summary.md", "drift-report.json"] {
        assert!(out.join(file).is_file(), "{file}");
    }
}

#[test]
fn a_service_that_answers_503_a_few_times_is_asked_again_and_the_check_still_answers() {
    // Given the Orca release endpoints that answer 503 to each path's first two requests
    let dir = root();
    let before = pin(dir.path(), "orca");
    let (next, answers) = orca_release(&before);
    let upstream = serve_flaky(answers, 2);
    // When the pin is checked
    let run = track(dir.path(), &upstream.base, &["check", "--app", "orca"]);
    // Then the verdict is the new release, not a failure, and the first path was asked three times
    assert_eq!(run.code, 0, "{}", run.stdout);
    assert_eq!((run.json["latest"].as_str(), run.json["drifted"].as_bool()), (Some(next.as_str()), Some(true)), "{}", run.stdout);
    let asked = upstream.asked.lock().unwrap().clone();
    assert!(asked.iter().filter(|p| **p == asked[0]).count() >= 3, "{asked:?}");
}
