use super::*;
use crate::cassette::{Outcome, Step, StepKind};
use serde_json::json;

fn step(dir: Dir, rid: Option<&str>, payload: Value) -> Step {
    Step { dir, channel: Some("ws".into()), kind: StepKind::Json, rid: rid.map(String::from), len: None, payload }
}

fn case(id: &str, steps: Vec<Step>) -> Interaction {
    Interaction { id: id.into(), row: Some(format!("row:{id}")), op: None, input: json!({}), steps, effects: vec![], outcome: Outcome::default(), tags: vec![] }
}

fn reply(id: &str, body: Value) -> Interaction {
    case(id, vec![step(Dir::S2c, Some("1"), body)])
}

fn hex(seed: u8, len: usize) -> String {
    (0..len).map(|i| format!("{:x}", (seed as usize * 7 + i * 5) % 16)).collect()
}

fn base() -> RuleSet {
    RuleSet::new(vec![])
}

fn run(rules: &RuleSet, recordings: &[Vec<Interaction>]) -> Stabilization {
    stabilize(rules, recordings).expect("rules compile")
}

fn after(s: &Stabilization, recordings: &[Vec<Interaction>]) -> Vec<Vec<Interaction>> {
    let k = s.rules.compile().expect("compiles");
    recordings.iter().map(|r| r.iter().map(|i| k.interaction(i)).collect()).collect()
}

#[test]
fn given_a_trace_id_that_differs_every_run_when_stabilized_then_one_shape_rule_makes_all_runs_equal() {
    let runs: Vec<Vec<Interaction>> = (1..=3).map(|n| vec![reply("a", json!({"log": format!("span {} done", hex(n, 32))}))]).collect();
    let s = run(&base(), &runs);
    assert_eq!(s.stable_before(), 0);
    assert_eq!(s.stable_after(), 1, "{:?}", s.cases);
    assert!(matches!(&s.proposals[0].rule, Rule::Token { pattern, .. } if pattern == r"\b[0-9a-f]{32}\b"), "{:?}", s.proposals);
    let masked = after(&s, &runs);
    assert_eq!(masked[0], masked[2]);
}

#[test]
fn given_a_stable_answer_with_the_volatile_shape_when_stabilized_then_it_is_not_masked_and_the_volatile_one_is_scoped() {
    let stable = hex(9, 32);
    let runs: Vec<Vec<Interaction>> =
        (1..=2).map(|n| vec![reply("volatile", json!({"id": hex(n, 32)})), reply("stable", json!({"id": stable}))]).collect();
    let s = run(&base(), &runs);
    assert_eq!(s.stable_after(), 2);
    assert!(s.proposals.iter().all(|p| matches!(&p.rule, Rule::Pointer { interaction: Some(id), .. } if id == "volatile")), "{:?}", s.proposals);
    let masked = after(&s, &runs);
    assert_eq!(masked[0][1].steps[0].payload, json!({"id": stable}));
    assert_eq!(masked[0][0].steps[0].payload, json!({"id": "<volatile>"}));
}

#[test]
fn given_replies_to_two_requests_interleaved_differently_when_stabilized_then_a_canonical_order_makes_them_equal() {
    let frames = |order: [&str; 4]| {
        vec![case("s", order.iter().map(|r| step(Dir::S2c, Some(r), json!({"requestId": r, "n": r}))).collect())]
    };
    let runs = vec![frames(["1", "1", "2", "2"]), frames(["2", "1", "2", "1"])];
    let s = run(&base(), &runs);
    assert_eq!(s.stable_after(), 1);
    assert_eq!(s.proposals[0].rule, Rule::CanonicalOrder { channel: "ws".into() });
}

#[test]
fn given_a_different_number_of_frames_when_stabilized_then_it_stays_unstable_with_the_array_cause() {
    let runs = vec![
        vec![case("s", vec![step(Dir::S2c, Some("1"), json!({"a": 1}))])],
        vec![case("s", vec![step(Dir::S2c, Some("1"), json!({"a": 1})), step(Dir::S2c, Some("1"), json!({"a": 2}))])],
    ];
    let s = run(&base(), &runs);
    assert_eq!(s.stable_after(), 0);
    assert_eq!(s.cases[0].cause, Some(Cause::ArrayLength));
    assert!(s.proposals.is_empty());
}

#[test]
fn given_a_counter_that_differs_by_one_when_stabilized_then_it_is_not_masked() {
    let runs: Vec<Vec<Interaction>> = [1, 2].iter().map(|n| vec![reply("c", json!({"count": n}))]).collect();
    let s = run(&base(), &runs);
    assert_eq!(s.cases[0].cause, Some(Cause::ShortValue));
    assert!(s.proposals.is_empty());
}

#[test]
fn given_a_timing_key_that_always_differs_when_stabilized_then_the_key_is_masked_everywhere_and_a_stable_key_is_not() {
    let runs: Vec<Vec<Interaction>> =
        [4120u64, 5237].iter().map(|ms| vec![reply("a", json!({"elapsedMs": ms, "total": 3000})), reply("b", json!({"elapsedMs": ms + 1000}))]).collect();
    let s = run(&base(), &runs);
    assert_eq!(s.stable_after(), 2);
    assert!(matches!(&s.proposals[0].rule, Rule::VolatileNumber { keys } if keys.contains_key("elapsedMs")), "{:?}", s.proposals);
    assert_eq!(after(&s, &runs)[0][0].steps[0].payload["total"], json!(3000));
}

#[test]
fn given_a_session_cookie_name_with_a_port_and_a_hash_when_stabilized_then_the_pattern_keeps_the_literal_and_generalises_both_parts() {
    let runs: Vec<Vec<Interaction>> = [(5001, "9f8e7d6c5b4a"), (5002, "0a1b2c3d4e5f")].iter().map(|(p, h)| vec![reply("c", json!({"cookie": format!("t3_session_{p}_{h}=on")}))]).collect();
    let s = run(&base(), &runs);
    assert_eq!(s.stable_after(), 1);
    assert!(matches!(&s.proposals[0].rule, Rule::Token { pattern, .. } if pattern.starts_with(r"\bt3_session_[0-9]{4}_[0-9a-f]{12}")), "{:?}", s.proposals);
}

#[test]
fn given_an_iso_timestamp_in_a_stringified_body_when_stabilized_then_it_is_masked_as_a_timestamp() {
    let runs: Vec<Vec<Interaction>> = ["2026-11-06T10:00:01.123Z", "2026-11-06T10:00:09.456Z"].iter().map(|t| vec![reply("t", json!({"body": format!("{{\"at\":\"{t}\"}}")}))]).collect();
    let s = run(&base(), &runs);
    assert_eq!(s.stable_after(), 1);
    assert!(matches!(&s.proposals[0].rule, Rule::Token { placeholder, .. } if placeholder == "<ts>"));
}

#[test]
fn given_free_text_that_differs_when_stabilized_then_it_stays_unstable_as_text() {
    let runs: Vec<Vec<Interaction>> = ["sunny today", "raining again"].iter().map(|t| vec![reply("t", json!({"answer": t}))]).collect();
    let s = run(&base(), &runs);
    assert_eq!(s.cases[0].cause, Some(Cause::TextChange));
}

#[test]
fn given_rules_already_masking_the_difference_when_stabilized_then_nothing_is_proposed() {
    let rules = RuleSet::new(vec![Rule::VolatileString { keys: BTreeMap::from([("traceId".to_string(), "<trace>".to_string())]) }]);
    let runs: Vec<Vec<Interaction>> = (1..=3).map(|n| vec![reply("a", json!({"traceId": hex(n, 32)}))]).collect();
    let s = run(&rules, &runs);
    assert!(s.proposals.is_empty());
    assert_eq!((s.stable_before(), s.stable_after()), (1, 1));
}

#[test]
fn given_a_case_missing_from_one_recording_when_stabilized_then_it_is_unobserved_not_judged() {
    let runs = vec![vec![reply("a", json!(1)), reply("b", json!(1))], vec![reply("a", json!(1))]];
    let s = run(&base(), &runs);
    assert_eq!(s.cases.len(), 1);
    assert_eq!(s.unobserved, vec!["b".to_string()]);
}

#[test]
fn given_an_allowlist_when_accounted_then_unlisted_stale_and_wrong_cause_entries_are_named() {
    let runs: Vec<Vec<Interaction>> = ["sunny today", "raining again"].iter().map(|t| vec![reply("t", json!({"a": t})), reply("ok", json!({"a": 1}))]).collect();
    let s = run(&base(), &runs);
    let entry = |id: &str, cause| AllowEntry { id: id.into(), cause, reason: "why".into() };
    let list = |entries| StabilityAllowlist { schema: ALLOWLIST_SCHEMA.into(), entries };
    assert!(account(&s, &list(vec![entry("t", Cause::TextChange)])).clean());
    assert_eq!(account(&s, &list(vec![])).unlisted, vec!["t".to_string()]);
    assert_eq!(account(&s, &list(vec![entry("t", Cause::TextChange), entry("ok", Cause::TextChange)])).stale, vec!["ok".to_string()]);
    assert_eq!(account(&s, &list(vec![entry("t", Cause::ArrayLength)])).wrong_cause, vec!["t".to_string()]);
}
