//! `wf mimic proxy check` through the thin binary: a recorded journey is played to a candidate server, and the first thing the
//! candidate answers differently is named.
use crate::proxy_support::*;
use std::path::{Path, PathBuf};

fn text(path: &Path) -> String {
    path.display().to_string()
}

/// The journey recorded into `<dir>/set` against an upstream whose session names a fixture home.
fn recorded(dir: &Path) -> PathBuf {
    let upstream = Upstream::start();
    upstream.wording.lock().unwrap().1 = "/fixture/home".to_string();
    let set = dir.join("set");
    let mut tool = Tool::start(dir, &["record", "--set", &text(&set), "--upstream", &format!("http://{}", upstream.addr)], &[]);
    journey(tool.addr, ["1", "2", "6f1e6a2e-9d0b-4d7c-8a51-0c2a4b9f1e11"]);
    assert_eq!(tool.stop().code, 0);
    set
}

#[test]
fn given_a_candidate_that_answers_as_recorded_when_checked_then_nothing_differs() {
    // Given a recorded journey and a candidate that serves the same answers: a replay of the set
    let dir = tempfile::tempdir().unwrap();
    let set = recorded(dir.path());
    let mut candidate = Tool::start(dir.path(), &["replay", &text(&set)], &[]);

    // When the set's client is played to it
    let checked = run(&["check", &text(&set), "--addr", &candidate.addr.to_string(), "--settle-ms", "100"]);
    let served = candidate.stop();

    // Then nothing differs, and the candidate was asked everything the recording has
    assert_eq!((checked.code, &checked.json["ok"], &checked.json["summary"]["clean"], &checked.json["summary"]["total"]), (0, &json!(true), &json!(5), &json!(5)), "{}", checked.stdout);
    assert!(checked.json["first"].is_null(), "{}", checked.stdout);
    assert_eq!((&served.json["mismatch_count"], &served.json["unused"]), (&json!(0), &json!([])), "{}", served.stdout);
}

#[test]
fn given_a_candidate_with_credentials_of_its_own_and_one_changed_answer_when_checked_then_the_first_difference_names_the_method_and_the_place() {
    // Given a recorded journey, and a candidate that is a server of its own: it issues its own token, cookie and ticket, and words one reply differently
    let dir = tempfile::tempdir().unwrap();
    let set = recorded(dir.path());
    let candidate = Upstream::start();
    *candidate.wording.lock().unwrap() = ("changed: ".to_string(), "/fixture/home".to_string());

    // When the set's client is played to it, with the pairing grant the candidate accepts bound to the recorded one
    std::env::set_var("MIMIC_TEST_PAIRING", PAIRING);
    let checked = run(&["check", &text(&set), "--addr", &candidate.addr.to_string(), "--bind-env", "fx-subject-token-1=MIMIC_TEST_PAIRING", "--settle-ms", "100"]);

    // Then the candidate's own credentials carried the journey to the socket: only the changed reply differs
    let first = &checked.json["first"];
    assert_eq!((checked.code, &checked.json["ok"], &checked.json["summary"]["clean"]), (1, &json!(false), &json!(4)), "{}", checked.stdout);
    assert_eq!((&first["interaction"], &first["what"], &first["cassette"], &first["evidence"]), (&json!("c01"), &json!("orchestration.subscribeThread"), &json!("interactions.jsonl:5"), &json!("authored")), "{first}");
    assert_eq!((&first["expected"], &first["actual"]), (&json!("reply to: say hello"), &json!("changed: say hello")), "{first}");
    // and what is printed holds none of the candidate's credentials
    assert!(!checked.stdout.contains("CANARY") && !checked.stderr.contains("CANARY"), "{}", checked.stdout);
}
