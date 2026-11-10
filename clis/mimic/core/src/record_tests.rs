//! Given a corpus and a reference that keeps state, When it is recorded under a plan, Then the cases run in the groups and chunks the plan
//! names, a killed recording resumes without asking again what it has, and the second run is the stability study of a sample.
use super::*;
use crate::cassette::{Dir, Kind, Outcome, Status, Step, StepKind, ToolInfo};
use crate::ports::{DriverError, Hello};
use crate::testing::driver_info;
use serde_json::json;

/// A target that counts the `w` cases since its last reset and writes what it is asked into `log`.
#[derive(Default)]
struct Counting {
    log: Vec<String>,
    asked: Vec<String>,
    counter: u64,
    served: u64,
    die_after: Option<u64>,
    wobble: bool,
}

impl Driver for Counting {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        Ok(Hello { driver: driver_info("counting"), target_version: Some("1".into()), coverage: false })
    }
    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        if self.die_after == Some(self.served) {
            return Err(DriverError("the reference died".into()));
        }
        self.served += 1;
        self.asked.push(request.id.clone());
        self.log.push(format!("run {}", request.id));
        if request.id.starts_with('w') {
            self.counter += 1;
        }
        let n = if self.wobble { self.served } else { self.counter };
        let step = Step { dir: Dir::Ret, channel: None, kind: StepKind::Json, rid: None, len: None, payload: json!({"id": request.id, "n": n}) };
        Ok(Observed { steps: vec![step], effects: Vec::new(), outcome: Outcome { status: Status::Ok, code: None, reference_accepts: Some(true) } })
    }
    fn reset(&mut self) -> Result<(), DriverError> {
        self.log.push("reset".into());
        self.counter = 0;
        Ok(())
    }
    fn run_many(&mut self, requests: &[Request]) -> Vec<Result<Observed, DriverError>> {
        let out = requests.iter().map(|r| self.run(r)).collect();
        let at = self.log.len() - requests.len();
        self.log.splice(at.., [format!("many {}", requests.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().join(","))]);
        out
    }
}

fn case(id: &str, tags: &[&str]) -> Case {
    Case { id: id.into(), op: None, row: None, input: json!({"id": id}), tags: tags.iter().map(|t| (*t).to_string()).collect() }
}

fn meta() -> Meta {
    Meta { kind: Kind::Recorded, target: None, tool: ToolInfo { mimic: "t".into(), git: None }, seed: Some(1), recorded_at: None, recorded_on: None, source: None, rules_path: "rules.json".into(), corpus_path: None }
}

fn plan(batch: usize, write_chunk: usize, double_run: DoubleRun) -> Plan {
    Plan { double_run, batch, write_chunk, chunk: None, until: None, keep_raw: false }
}

fn record_all(driver: &mut Counting, cases: &[Case], plan: &Plan, ck: &mut MemCheckpoint) -> Result<Progress, UseCaseError> {
    record_planned(driver, cases, &RuleSet::new(vec![]), None, meta(), plan, ck)
}

fn corpus() -> Vec<Case> {
    let mut cases: Vec<Case> = (0..6).map(|i| case(&format!("f{i}"), &[])).collect();
    cases.extend((0..5).map(|i| case(&format!("w{i}"), &[TAG_WRITE])));
    cases.insert(3, case("w-early", &[TAG_WRITE]));
    cases
}

const NEVER: DoubleRun = DoubleRun::Sample { pct: 0, seed: 1 };

fn seen(done: &Recording) -> Vec<u64> {
    done.set.cassette.interactions.iter().map(|i| i.steps[0].payload["n"].as_u64().unwrap()).collect()
}

#[test]
fn state_free_cases_run_together_first_and_state_changing_ones_in_chunks_with_a_reset_between() {
    // Given six state-free and six state-changing cases (one of them listed among the free ones)
    let (mut driver, mut ck) = (Counting::default(), MemCheckpoint::default());
    // When they are recorded with batches of 3 and chunks of 4, no case run twice
    let Progress::Done(done) = record_all(&mut driver, &corpus(), &plan(3, 4, NEVER), &mut ck).unwrap() else { panic!("not finished") };
    // Then the free ones went first, three at a time, and the others in chunks of four with one reset between and none before the first
    let log = driver.log.join(" | ");
    assert_eq!(log, "many f0,f1,f2 | many f3,f4,f5 | run w-early | run w0 | run w1 | run w2 | reset | run w3 | run w4", "{log}");
    // and the cassette lists the cases in the order of the corpus whatever order they ran in
    let ids: Vec<&str> = done.set.cassette.interactions.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, corpus().iter().map(|c| c.id.as_str()).collect::<Vec<_>>());
}

#[test]
fn the_first_case_of_a_chunk_sees_the_target_as_the_first_chunk_did() {
    // Given a target whose answer counts the state-changing requests since its reset
    let (mut driver, mut ck) = (Counting::default(), MemCheckpoint::default());
    let cases = vec![case("w0", &[TAG_WRITE]), case("w1", &[TAG_WRITE]), case("w2", &[TAG_WRITE])];
    // When the cases are recorded two to a chunk
    let Progress::Done(done) = record_all(&mut driver, &cases, &plan(1, 2, NEVER), &mut ck).unwrap() else { panic!() };
    // Then the third case, first of its chunk, saw the target as the first did
    assert_eq!(seen(&done), vec![1, 2, 1]);
}

#[test]
fn a_fresh_case_gets_a_pristine_target_of_its_own() {
    let (mut driver, mut ck) = (Counting::default(), MemCheckpoint::default());
    let cases = vec![case("w0", &[TAG_WRITE]), case("w-own", &[TAG_FRESH]), case("w1", &[TAG_WRITE])];
    let Progress::Done(done) = record_all(&mut driver, &cases, &plan(1, 0, NEVER), &mut ck).unwrap() else { panic!() };
    assert_eq!(seen(&done), vec![1, 1, 1], "{}", driver.log.join(" | "));
}

#[test]
fn only_the_sample_of_cases_is_run_twice_and_the_sample_does_not_depend_on_the_batching() {
    // Given 40 state-free cases
    let cases: Vec<Case> = (0..40).map(|i| case(&format!("c{i}"), &[])).collect();
    let runs = |batch: usize, double_run: DoubleRun| {
        let (mut driver, mut ck) = (Counting::default(), MemCheckpoint::default());
        record_all(&mut driver, &cases, &plan(batch, 0, double_run), &mut ck).unwrap();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for id in driver.asked {
            *counts.entry(id).or_default() += 1;
        }
        counts
    };
    // When a quarter is sampled, at two batch sizes
    let (a, b) = (runs(1, DoubleRun::Sample { pct: 25, seed: 9 }), runs(8, DoubleRun::Sample { pct: 25, seed: 9 }));
    // Then the same ones ran twice, a minority of them, and every case ran
    let twice = |m: &BTreeMap<String, usize>| m.iter().filter(|(_, n)| **n == 2).map(|(k, _)| k.clone()).collect::<Vec<_>>();
    assert_eq!(twice(&a), twice(&b));
    assert!(!twice(&a).is_empty() && twice(&a).len() < 20, "{:?}", twice(&a));
    assert_eq!(a.len(), 40);
    // and every case twice when the whole corpus is the sample
    assert!(runs(4, DoubleRun::All).values().all(|n| *n == 2));
}

#[test]
fn an_unstable_case_is_found_only_when_it_is_in_the_sample() {
    // Given a target whose answer changes on every request
    let cases = vec![case("c0", &[])];
    let found = |double_run| {
        let mut driver = Counting { wobble: true, ..Counting::default() };
        let Progress::Done(done) = record_all(&mut driver, &cases, &plan(1, 0, double_run), &mut MemCheckpoint::default()).unwrap() else { panic!() };
        done.nondeterministic
    };
    // When it is run twice, Then it is named; when it is not in the sample, it is not
    assert_eq!(found(DoubleRun::All), vec!["c0".to_string()]);
    assert!(found(NEVER).is_empty());
}

#[test]
fn a_recording_killed_half_way_resumes_with_only_the_cases_it_does_not_have_and_equals_one_that_was_not_killed() {
    let cases = corpus();
    let wanted = plan(3, 4, NEVER);
    // Given a recording that was not killed
    let Progress::Done(whole) = record_all(&mut Counting::default(), &cases, &wanted, &mut MemCheckpoint::default()).unwrap() else { panic!() };
    // When the reference dies after seven answers (the batches before it are saved)
    let mut ck = MemCheckpoint::default();
    let mut dying = Counting { die_after: Some(7), ..Counting::default() };
    let killed = record_all(&mut dying, &cases, &wanted, &mut ck);
    assert!(matches!(killed, Err(UseCaseError::Driver { .. })), "{killed:?}");
    let saved = ck.0.iter().filter(|e| matches!(e, CheckpointEntry::Done { .. })).count();
    assert!(saved >= 6 && saved < cases.len(), "{saved}");
    // and the recording is called again with a healthy reference
    let mut healthy = Counting::default();
    let Progress::Done(resumed) = record_all(&mut healthy, &cases, &wanted, &mut ck).unwrap() else { panic!() };
    // Then it asked only for the cases the checkpoint did not have, and the cassette is the one that was not killed
    assert_eq!(healthy.asked.len(), cases.len() - saved, "{}", healthy.log.join(" | "));
    assert_eq!(resumed.set.cassette.header.content_hash, whole.set.cassette.header.content_hash);
    assert!(ck.0.is_empty(), "the checkpoint is gone once the cassette is whole");
}

#[test]
fn a_chunk_stops_after_that_many_cases_and_the_next_call_carries_on_to_the_same_cassette() {
    let cases = corpus();
    let wanted = Plan { chunk: Some(5), ..plan(2, 3, NEVER) };
    let Progress::Done(whole) = record_all(&mut Counting::default(), &cases, &Plan { chunk: None, ..wanted.clone() }, &mut MemCheckpoint::default()).unwrap() else { panic!() };
    let mut ck = MemCheckpoint::default();
    let mut asked = Vec::new();
    let done = loop {
        let mut driver = Counting::default();
        let progress = record_all(&mut driver, &cases, &wanted, &mut ck).unwrap();
        asked.push(driver.asked.len());
        match progress {
            Progress::Partial { total, .. } => assert_eq!(total, cases.len()),
            Progress::Done(d) => break d,
        }
        assert!(asked.len() < 10, "{asked:?}");
    };
    assert!(asked.iter().all(|n| *n < 5 + 3), "a call stops at the first chunk boundary after 5 cases: {asked:?}");
    assert_eq!(asked.iter().sum::<usize>(), cases.len());
    assert_eq!(done.set.cassette.header.content_hash, whole.set.cassette.header.content_hash, "chunked and whole recordings differ");
}

#[test]
fn a_checkpoint_of_other_cases_is_refused_not_mixed_in() {
    let mut ck = MemCheckpoint::default();
    record_all(&mut Counting::default(), &corpus(), &Plan { chunk: Some(2), ..plan(2, 3, NEVER) }, &mut ck).unwrap();
    let mut other = corpus();
    other.push(case("extra", &[]));
    let e = record_all(&mut Counting::default(), &other, &plan(2, 3, NEVER), &mut ck).unwrap_err();
    assert!(matches!(&e, UseCaseError::Checkpoint(m) if m.contains("other cases")), "{e}");
}

#[test]
fn a_state_changing_case_is_recorded_on_the_same_state_whether_or_not_the_sample_picks_it() {
    // Given state-changing cases whose answer counts what the target already saw
    let cases: Vec<Case> = (0..12).map(|i| case(&format!("w{i}"), &[TAG_WRITE])).collect();
    let answers = |double_run: DoubleRun| {
        let (mut driver, mut ck) = (Counting::default(), MemCheckpoint::default());
        let Progress::Done(done) = record_all(&mut driver, &cases, &plan(1, 4, double_run), &mut ck).unwrap() else { panic!() };
        seen(&done)
    };
    // When every case is in the stability sample, and when none is, Then the recorded answers are the same
    assert_eq!(answers(DoubleRun::All), answers(DoubleRun::Sample { pct: 0, seed: 1 }));
}
