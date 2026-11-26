//! The canary test of `wf mimic proxy`: credentials planted at every place one travels (a pairing grant, a cookie, an
//! Authorization header, a session token, a socket ticket, a declared vault value, a token in a failing request) and then every
//! artifact a run leaves is scanned for them.
use crate::proxy_support::*;

const DECLARED: &str = "CANARY-DECLARED-VAULT-VALUE-77";
const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJjYW5hcnkifQ.c2lnbmF0dXJlLWNhbmFyeQ";

#[test]
fn given_credentials_at_every_place_one_travels_when_a_journey_is_recorded_and_a_failing_replay_is_published_then_no_artifact_holds_one() {
    // Given an upstream whose every credential is a canary, one answer that echoes a declared vault value and a token nobody named
    let dir = tempfile::tempdir().unwrap();
    let upstream = Upstream::start();
    *upstream.extra.lock().unwrap() = (String::new(), json!({"vaultEcho": DECLARED, "blob": JWT, "line": format!("upstream said Bearer {}", "canarybearer0123456789")}).to_string().into_bytes());
    let (set, trace, out) = (dir.path().join("set"), dir.path().join("trace.jsonl"), dir.path().join("out"));
    let path = |p: &std::path::Path| p.display().to_string();

    // When the journey is recorded with its shape trace
    let upstream_url = format!("http://{}", upstream.addr);
    let mut record = Tool::start(dir.path(), &["record", "--set", &path(&set), "--upstream", &upstream_url, "--trace", &path(&trace), "--secret-env", "MIMIC_TEST_DECLARED"], &[("MIMIC_TEST_DECLARED", DECLARED)]);
    journey(record.addr, ["1", "2", "6f1e6a2e-9d0b-4d7c-8a51-0c2a4b9f1e11"]);
    http(record.addr, "GET", "/api/extra", &[("cookie", &format!("sid={COOKIE}"))], "");
    let recorded = record.stop();
    upstream.stop();
    // and a replay fails on requests that carry live credentials, and writes its report
    let mut replay = Tool::start(dir.path(), &["replay", &path(&set), "--suite", "wf-mimic-proxy-canary"], &[("WF_TEST_OUTPUT_DIR", &path(&out))]);
    journey(replay.addr, ["7", "8", "0b7c2f4d-1a3e-4f5a-9b6c-7d8e9f0a1b2c"]);
    let live = format!("Bearer {TOKEN}");
    http(replay.addr, "GET", &format!("/api/unknown?token={TICKET}"), &[("authorization", &live), ("cookie", &format!("sid={COOKIE}"))], "");
    http(replay.addr, "POST", "/api/auth/websocket-ticket/rpc", &[("authorization", &live), ("content-type", "application/json")], &json!({"password": SESSION, "note": format!("retry with Bearer {}", "canarylivebearer012345"), "blob": JWT}).to_string());
    let replayed = replay.stop();

    // Then the set was written and the replay failed as planned
    assert_eq!((recorded.code, replayed.code, &replayed.json["mismatch_count"]), (0, 1, &json!(2)), "{}\n{}", recorded.stdout, replayed.stdout);
    // and the fixture values keep the relationships a replay needs
    let cassette = std::fs::read_to_string(set.join("interactions.jsonl")).unwrap();
    for kept in ["subject_token=fx-subject-token-1", "Bearer fx-access-token-1", "sid=fx-cookie-1", "wsTicket=fx-ticket-1", "fx-session-token-1", "fx-declared-1", "fx-jwt-1", "fx-bearer-1"] {
        assert!(cassette.contains(kept), "the cassette lacks {kept}");
    }
    // and nothing a run left holds a canary: the set, the trace, the report, the ready files, what the verbs printed
    let mut artifacts = files(dir.path());
    assert!(artifacts.iter().any(|(p, _)| p.ends_with("out/wf-mimic-proxy-canary/report.json")) && artifacts.iter().any(|(p, _)| p.ends_with("trace.jsonl")));
    for (name, printed) in [("record stdout", &recorded.stdout), ("record stderr", &recorded.stderr), ("replay stdout", &replayed.stdout), ("replay stderr", &replayed.stderr)] {
        artifacts.push((name.into(), printed.clone()));
    }
    for (file, text) in &artifacts {
        for canary in ["CANARY", "canarybearer", "canarylivebearer", "eyJ"] {
            assert!(!text.contains(canary), "{} holds {canary}", file.display());
        }
    }
    // and no raw capture was kept beside the set
    assert!(!set.join("raw").exists());
}
