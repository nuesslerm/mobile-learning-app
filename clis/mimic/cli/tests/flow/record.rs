use crate::support::*;
use mimic_core::json::sha256_label;
use mimic_core::rules::RuleSet;
use regex::Regex;
use std::path::Path;

fn corpus_ids(w: &World) -> Vec<String> {
    let spec: Value = serde_json::from_str(&w.read("corpus.json")).unwrap();
    spec["generators"].as_array().unwrap().iter().filter(|g| g["kind"] == "corpus").flat_map(|g| g["cases"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string())).collect()
}

fn uuid() -> Regex {
    Regex::new("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap()
}

#[test]
fn recording_a_reference_that_makes_things_up_hides_them_behind_masks() {
    // Given a toy reference that makes up a uuid, a timestamp, a token and a sequence number on every run
    let w = World::new();
    // When it is recorded, keeping the raw capture
    let r = w.run("record", &["--keep-raw"]);
    // Then the raw capture holds what the reference made up and the cassette holds only masks
    assert_eq!((r.code, r.json["ok"].clone(), r.json["schema"].clone()), (0, json!(true), json!("mimic-record/1")), "{}\n{}", r.json, r.stderr);
    let raw = w.read("raw/capture.jsonl");
    let made_up = uuid().find(&raw).expect("the toy made up a uuid").as_str().to_string();
    let cassette = w.read("interactions.jsonl");
    assert!(!cassette.contains(&made_up) && !uuid().is_match(&cassette), "a uuid survived masking: {cassette}");
    assert!(!Regex::new("[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:").unwrap().is_match(&cassette), "a timestamp survived masking");
    for mask in ["<uuid:1>", "<ts>", "<token>", "<seq>"] {
        assert!(cassette.contains(mask), "{mask} is missing: {cassette}");
    }
    // and the raw directory can never be committed
    assert_eq!(w.read("raw/.gitignore").trim(), "*");
}

#[test]
fn a_recording_has_one_interaction_per_case_of_the_corpus_and_names_its_parity_row() {
    // Given the corpus of the set
    let w = World::new();
    // When it is recorded
    let r = w.record();
    // Then every hand-picked case is an interaction under its id and row, and the seeded ones are stored with their mutations
    let recorded = w.interactions();
    let ids: Vec<&str> = recorded.iter().map(|i| i["id"].as_str().unwrap()).collect();
    for id in corpus_ids(&w) {
        assert!(ids.contains(&id.as_str()), "{id} not in {ids:?}");
    }
    assert_eq!(r.json["interactions"], json!(recorded.len()));
    let seeded: Vec<&Value> = recorded.iter().filter(|i| i["id"].as_str().unwrap().starts_with("drawn-")).collect();
    assert!(seeded.len() > 3 && seeded.iter().any(|i| i["input"]["a"] == ""), "the empty mutation is a case: {seeded:?}");
    assert_eq!(recorded.iter().find(|i| i["id"] == "echo-1").unwrap()["row"], "rpc:echo");
}

#[test]
fn a_re_record_of_equal_behaviour_is_the_same_cassette_and_a_new_seed_is_not() {
    // Given a recorded set
    let w = World::new();
    w.record();
    let first = (serde_json::from_str::<Value>(&w.read("cassette.json")).unwrap(), w.read("interactions.jsonl"));
    // When it is recorded again, the reference having made up other ids
    w.record();
    let second = (serde_json::from_str::<Value>(&w.read("cassette.json")).unwrap(), w.read("interactions.jsonl"));
    // Then the interactions and the content hash are equal (the time it was recorded is no part of it)
    assert_eq!((&first.1, &first.0["content_hash"]), (&second.1, &second.0["content_hash"]));
    // When the seed of the corpus moves
    let r = w.run("record", &["--seed", "8"]);
    assert_eq!(r.code, 0, "{}", r.json);
    // Then the generated inputs and the hash move with it
    let third = (serde_json::from_str::<Value>(&w.read("cassette.json")).unwrap(), w.read("interactions.jsonl"));
    assert!(third.1 != first.1 && third.0["content_hash"] != first.0["content_hash"] && third.0["provenance"]["seed"] == 8);
}

#[test]
fn a_recorded_cassette_names_the_pin_driver_rules_and_corpus_it_came_from() {
    // Given a set recorded against a pin file
    let w = World::new();
    w.record();
    // When its header is read
    let header: Value = serde_json::from_str(&w.read("cassette.json")).unwrap();
    let p = &header["provenance"];
    // Then every hash is that of the file on disk now, and the version is read from the pin by pointer
    let pin: Value = serde_json::from_str(&w.read("pin.json")).unwrap();
    let rules: RuleSet = serde_json::from_str(&w.read("rules.json")).unwrap();
    assert_eq!(p["kind"], "recorded");
    assert_eq!(p["target"]["pin_hash"], json!(sha256_label(w.read("pin.json").as_bytes())));
    assert_eq!(p["target"]["version"], pin["version"]);
    assert_eq!((p["rules_hash"].clone(), header["rules"]["hash"].clone()), (json!(rules.hash()), json!(rules.hash())));
    assert_eq!((p["driver"]["protocol"].clone(), p["driver"]["name"].clone()), (json!("mimic-driver/1"), json!("toy")));
    assert!(p["corpus_hash"].as_str().is_some_and(|h| h.starts_with("sha256:")) && p["recorded_at"].is_string());
}

#[test]
fn output_nothing_masks_is_reported_nondeterministic_and_never_silently_recorded() {
    // Given a corpus with a case whose output is a fresh number on every run
    let w = World::with_corpus(&[json!({"kind": "corpus", "name": "wobble", "op": "wobble", "cases": [{"id": "wobble-1", "input": {}}]})]);
    // When it is recorded
    let r = w.run("record", &[]);
    // Then nothing is written, the case is named, and the raw capture is the evidence
    assert_eq!((r.code, r.json["ok"].clone(), r.json["nondeterministic"].clone()), (1, json!(false), json!(["wobble-1"])), "{}", r.json);
    assert!(!w.path("cassette.json").exists() && !w.path("interactions.jsonl").exists());
    assert!(std::path::Path::new(r.json["evidence"]["raw"].as_str().unwrap()).is_file());
    assert!(r.json["next"].as_str().unwrap().contains("--allow-nondeterministic"), "{}", r.json["next"]);
    // And the differing path is named with the kind of each side, never a value
    let d = r.json["differences"]["wobble-1"].as_array().unwrap();
    assert_eq!(d.len(), 1, "{}", r.json["differences"]);
    assert!(d[0]["path"].as_str().unwrap().starts_with("/steps/") && d[0]["path"].as_str().unwrap().ends_with("/n") && d[0]["first"] == "number" && d[0]["second"] == "number", "{}", d[0]);
    // When the caller decides to record it flagged
    let r = w.run("record", &["--allow-nondeterministic"]);
    // Then the case is tagged and a warning says so
    assert_eq!(r.code, 0, "{}", r.json);
    let wobble = w.interactions().into_iter().find(|i| i["id"] == "wobble-1").unwrap();
    assert_eq!(wobble["tags"], json!(["nondeterministic"]));
    assert!(r.json["warnings"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().contains("nondeterministic")));
}

#[test]
fn a_reference_that_cannot_be_recorded_is_refused_with_the_reason_and_the_next_step() {
    for (what, toml, needle) in [
        ("no pin", format!("[reference]\ncommand = [{:?}]\n", toy().display().to_string()), "pin"),
        ("no such program", "[reference]\ncommand = [\"/nonexistent/driver\"]\npin = \"pin.json\"\n".to_string(), "/nonexistent/driver"),
        ("a program that exits", "[reference]\ncommand = [\"/bin/sh\", \"-c\", \"echo the-driver-said-so >&2; exit 3\"]\npin = \"pin.json\"\n".to_string(), "the-driver-said-so"),
        ("a program that does not speak the protocol", "[reference]\ncommand = [\"/bin/sh\", \"-c\", \"read l; echo not-json\"]\npin = \"pin.json\"\n".to_string(), "not JSON"),
        ("a program that never answers", "[reference]\ncommand = [\"/bin/sh\", \"-c\", \"sleep 30\"]\npin = \"pin.json\"\ntimeout_ms = 300\n".to_string(), "did not answer within 300 ms"),
    ] {
        // Given a set whose reference is broken in one way
        let w = World::new();
        w.write("mimic.toml", &toml);
        // When it is recorded
        let r = w.run("record", &[]);
        // Then it is refused as an environment error, with the cause in the error and a next step, and nothing is written
        assert_eq!((r.code, r.json["ok"].clone()), (2, json!(false)), "{what}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains(needle), "{what}: {}", r.json);
        assert!(!r.json["next"].as_str().unwrap().is_empty() && !w.path("cassette.json").exists(), "{what}: {}", r.json);
    }
}

#[test]
fn a_case_the_reference_fails_on_names_the_interaction() {
    // Given a corpus with a case for an op the toy does not have
    let w = World::with_corpus(&[json!({"kind": "corpus", "name": "bogus", "op": "bogus", "cases": [{"id": "bogus-1", "input": {}}]})]);
    // When it is recorded
    let r = w.run("record", &[]);
    // Then the error names the op and the interaction
    assert_eq!(r.code, 2, "{}", r.json);
    let error = r.json["error"].as_str().unwrap();
    assert!(error.contains("bogus") && error.contains("bogus-1"), "{error}");
}

#[test]
fn a_set_recorded_from_a_stand_in_claims_no_pin_and_cannot_serve_a_parity_row() {
    // Given a set whose reference is a pinned toy, recorded with the toy named as a stand-in
    let w = World::new();
    let r = w.run("record", &["--stand-in", "scripted toy host"]);
    // Then it is written as authored, with no pin hash and the stand-in in its source
    assert_eq!((r.code, r.json["ok"].clone()), (0, json!(true)), "{}\n{}", r.json, r.stderr);
    let p = &r.json["provenance"];
    assert_eq!(p["kind"], "authored");
    assert!(p["target"].get("pin_hash").is_none_or(|h| h.is_null()), "{p}");
    assert_eq!(p["source"], "stand-in: scripted toy host");
    // and verify accepts it as what it is, and the evidence rows of a replay name the provenance, so no row is served
    assert_eq!(w.run("verify", &[]).code, 0);
    let replay = w.run("replay", &["--candidate", "same"]);
    assert_eq!(replay.json["ok"], json!(true), "{}", replay.json);
    assert!(replay.json["evidence_rows"].as_u64().unwrap() > 0);
}

fn counted(n: usize, tag: &str) -> Value {
    let cases: Vec<Value> = (0..n).map(|i| json!({"id": format!("count-{i:02}"), "input": {}, "tags": [tag]})).collect();
    json!({"kind": "corpus", "name": "count", "op": "counter", "cases": cases})
}

fn counts(w: &World) -> Vec<u64> {
    w.interactions().iter().filter(|i| i["id"].as_str().unwrap().starts_with("count-")).map(|i| i["steps"][0]["payload"]["n"].as_u64().unwrap()).collect()
}

#[test]
fn state_changing_cases_are_recorded_in_chunks_on_a_reference_that_is_reset_between_them() {
    // Given a reference that counts the requests since its reset and five cases tagged as changing its state
    let w = World::with_corpus(&[counted(5, "state:write")]);
    // When the set is recorded with two such cases to a state of the reference
    let r = w.run("record", &["--write-chunk", "2", "--stability", "0"]);
    // Then the cases of a chunk saw the reference count on from the chunk's first, and each chunk began from nothing
    assert_eq!((r.code, r.json["done"].clone()), (0, json!(true)), "{}\n{}", r.json, r.stderr);
    assert_eq!(counts(&w), vec![1, 2, 1, 2, 1]);
}

#[test]
fn a_recording_killed_by_the_reference_resumes_from_its_checkpoint_to_the_cassette_an_unbroken_run_gives() {
    let corpus = [counted(6, "state:write")];
    let args = ["--write-chunk", "2", "--batch", "2", "--stability", "0"];
    // Given a recording that was not interrupted
    let whole = World::with_corpus(&corpus);
    assert_eq!(whole.run("record", &args).code, 0);
    // When the reference dies after answering 9 requests (the kill is planted by the toy)
    let w = World::with_corpus(&corpus);
    let set = w.arg();
    let mut with = vec!["record", set.as_str()];
    with.extend(args);
    let killed = mimic(&with, &[("MIMIC_TOY_DIE_AFTER", Path::new("9"))]);
    // Then the run fails naming the case and the way on, and the progress is on disk beside the set
    assert_eq!(killed.code, 2, "{}", killed.json);
    assert!(killed.json["next"].as_str().unwrap().contains("--resume"), "{}", killed.json["next"]);
    assert!(w.path("raw/checkpoint/record.checkpoint.jsonl").is_file() && !w.path("cassette.json").exists());
    // and a recording that does not say it carries on is refused, as is one over another corpus
    let plain = w.run("record", &args);
    assert_eq!(plain.code, 2);
    assert!(plain.json["next"].as_str().unwrap().contains("--fresh"), "{}", plain.json);
    // When it is run again with --resume
    let mut resume = args.to_vec();
    resume.push("--resume");
    let r = w.run("record", &resume);
    // Then it finishes, the checkpoint is gone, and the cassette is the unbroken run's
    assert_eq!((r.code, r.json["done"].clone()), (0, json!(true)), "{}\n{}", r.json, r.stderr);
    assert!(!w.path("raw/checkpoint/record.checkpoint.jsonl").exists());
    assert_eq!(w.read("interactions.jsonl"), whole.read("interactions.jsonl"));
    assert_eq!(serde_json::from_str::<Value>(&w.read("cassette.json")).unwrap()["content_hash"], serde_json::from_str::<Value>(&whole.read("cassette.json")).unwrap()["content_hash"]);
}

#[test]
fn a_chunk_bounds_one_call_and_the_calls_together_give_the_whole_cassette() {
    let w = World::with_corpus(&[counted(6, "state:write")]);
    let whole = World::with_corpus(&[counted(6, "state:write")]);
    let base = ["--write-chunk", "2", "--stability", "0"];
    assert_eq!(whole.run("record", &base).code, 0);
    // When the set is recorded five cases at a time
    let mut args = base.to_vec();
    args.extend(["--chunk", "5", "--resume"]);
    let mut calls = Vec::new();
    loop {
        let r = w.run("record", &args);
        assert_eq!(r.code, 0, "{}\n{}", r.json, r.stderr);
        calls.push((r.json["done"].as_bool().unwrap(), r.json["recorded"].as_u64()));
        if r.json["done"] == json!(true) {
            break;
        }
        assert!(r.json["next"].as_str().unwrap().contains("--resume") && calls.len() < 20, "{}", r.json);
    }
    // Then no call ran to the end of the corpus but the last, and the cassette is the whole run's
    assert!(calls.len() > 1 && calls[..calls.len() - 1].iter().all(|(done, recorded)| !done && recorded.is_some()), "{calls:?}");
    assert_eq!(w.read("interactions.jsonl"), whole.read("interactions.jsonl"));
}

#[test]
fn the_stability_study_runs_a_sample_twice_so_an_unstable_case_outside_it_is_not_flagged() {
    // Given a case whose output is a fresh number on every run
    let w = World::with_corpus(&[json!({"kind": "corpus", "name": "wobble", "op": "wobble", "cases": [{"id": "wobble-1", "input": {}}]})]);
    // When the stability study samples none of the cases, Then the case records as it answered once
    assert_eq!(w.run("record", &["--stability", "0"]).code, 0);
    // and when it samples all of them, the case is found out
    let r = w.run("record", &["--stability", "100"]);
    assert_eq!((r.code, r.json["nondeterministic"].clone()), (1, json!(["wobble-1"])), "{}", r.json);
}

#[test]
fn a_record_over_its_minutes_stops_with_a_checkpoint_and_the_resume_gives_the_uninterrupted_set() {
    let corpus = [counted(6, "state:write")];
    let args = ["--write-chunk", "2", "--stability", "0"];
    // Given a recording that was not interrupted
    let whole = World::with_corpus(&corpus);
    let r = whole.run("record", &args);
    assert_eq!((r.code, r.json["complete"].clone()), (0, json!(true)), "{}", r.json);
    // When a slow reference is recorded with a budget its first batch already spends
    let w = World::with_corpus(&corpus);
    let set = w.arg();
    let slow = [("MIMIC_TOY_DELAY_MS", Path::new("120"))];
    let mut limited = vec!["record", set.as_str(), "--resume", "--max-minutes", "0.001"];
    limited.extend(args);
    let first = mimic(&limited, &slow);
    // Then it stops cleanly after a batch with complete false, the checkpoint under raw/checkpoint and no cassette
    assert_eq!((first.code, first.json["ok"].clone(), first.json["complete"].clone()), (0, json!(true), json!(false)), "{}\n{}", first.json, first.stderr);
    let recorded = first.json["recorded"].as_u64().unwrap();
    assert!(recorded > 0 && recorded < 6, "{}", first.json);
    assert!(w.path("raw/checkpoint/record.checkpoint.jsonl").is_file() && !w.path("cassette.json").exists());
    // When the same call is repeated until it says complete
    let mut last = first;
    for _ in 0..40 {
        if last.json["complete"] == json!(true) {
            break;
        }
        last = mimic(&limited, &slow);
        assert_eq!(last.code, 0, "{}\n{}", last.json, last.stderr);
    }
    // Then the last call wrote the same masked set as the uninterrupted record and removed the checkpoint
    assert_eq!(last.json["complete"], json!(true), "{}", last.json);
    assert!(!w.path("raw/checkpoint/record.checkpoint.jsonl").exists());
    assert_eq!(w.read("interactions.jsonl"), whole.read("interactions.jsonl"));
}

/// A timed record of six state-changing cases that stopped after its first batch, its checkpoint on disk.
fn stopped() -> World {
    let w = World::with_corpus(&[counted(6, "state:write")]);
    let set = w.arg();
    let r = mimic(&["record", set.as_str(), "--resume", "--max-minutes", "0.001", "--write-chunk", "2", "--stability", "0"], &[("MIMIC_TOY_DELAY_MS", Path::new("120"))]);
    assert_eq!(r.json["complete"], json!(false), "{}\n{}", r.json, r.stderr);
    w
}

#[test]
fn a_checkpoint_of_another_corpus_or_rules_refuses_to_resume() {
    let args = ["--resume", "--write-chunk", "2", "--stability", "0"];
    for (what, change) in [("corpus", 0), ("rules", 1)] {
        // Given a timed record that stopped with its checkpoint
        let w = stopped();
        // When the corpus or the rules change and the record is resumed
        if change == 0 {
            w.write("corpus.json", &corpus(&[counted(7, "state:write")]).to_string());
        } else {
            w.write("rules.json", &RULES.replace("\"keys\":[\"sequence\"]", "\"keys\":[\"sequence\",\"extra\"]"));
        }
        let r = w.run("record", &args);
        // Then it refuses naming --fresh, writes no cassette and keeps the checkpoint
        assert_eq!((r.code, r.json["ok"].clone()), (2, json!(false)), "{what}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains("checkpoint"), "{what}: {}", r.json);
        assert!(!w.path("cassette.json").exists() && w.path("raw/checkpoint/record.checkpoint.jsonl").is_file(), "{what}");
    }
}

#[test]
fn a_max_minutes_that_is_no_number_of_minutes_is_refused() {
    let w = World::new();
    for bad in ["0", "-1", "soon"] {
        let r = w.run("record", &["--max-minutes", bad]);
        assert_eq!((r.code, r.json["ok"].clone()), (2, json!(false)), "{bad}: {}", r.json);
    }
}
