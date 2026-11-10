//! Unit tests of the pure pieces; the use cases and the T3 oracle are in tests/flow.rs.
use crate::allowlist::{Allowlist, Entry};
use crate::cassette::{Dir, Interaction, Step, StepKind};
use crate::differ::{diff_json, DiffKind, DiffOptions, Severity};
use crate::presets;
use crate::rules::{Rule, RuleSet};
use serde_json::json;

fn entry(path: &str, pin: Option<&str>) -> Entry {
    Entry { path: path.into(), reason: "because".into(), phase: "1".into(), pin_hash: pin.map(String::from) }
}

mod rules {
    use super::*;

    #[test]
    fn given_uuids_when_masked_then_ordinals_follow_first_appearance_and_repeat_for_the_same_uuid() {
        let c = presets::t3().compile().unwrap();
        let mut nz = c.normalizer();
        let (a, b) = ("11111111-1111-4111-8111-111111111111", "22222222-2222-4222-8222-222222222222");
        assert_eq!(nz.text(&format!("{b} {a} {b}")), "<uuid:1> <uuid:2> <uuid:1>");
    }

    #[test]
    fn given_a_secret_under_a_key_when_it_appears_elsewhere_then_it_is_masked_there_too_but_a_short_value_is_not_learned() {
        let c = presets::t3().compile().unwrap();
        let mut nz = c.normalizer();
        let masked = nz.json(&json!({"access_token": "tok-abcdef", "echo": "Bearer tok-abcdef", "ticket": "ab"}), None);
        assert_eq!(masked, json!({"access_token": "<access-token>", "echo": "Bearer <access-token>", "ticket": "<ws-ticket>"}));
        assert_eq!(nz.text("ab"), "ab");
    }

    #[test]
    fn given_headers_when_masked_then_dropped_ones_vanish_names_sort_and_credentials_hide() {
        let c = presets::t3().compile().unwrap();
        let mut nz = c.normalizer();
        let h = vec![
            ("Host".to_string(), "x".to_string()),
            ("Authorization".to_string(), "Bearer s3cr3t-value".to_string()),
            ("Set-Cookie".to_string(), "sid=1".to_string()),
            ("Accept".to_string(), "*/*".to_string()),
        ];
        let out = nz.headers(&h);
        assert_eq!(out.keys().collect::<Vec<_>>(), ["accept", "authorization", "set-cookie"]);
        assert_eq!(out["authorization"], "Bearer <access-token>");
        assert_eq!(out["set-cookie"], "<cookie>");
    }

    #[test]
    fn given_a_query_when_masked_then_secret_parameters_are_replaced_and_percent_encoding_is_read() {
        let c = presets::t3().compile().unwrap();
        let mut nz = c.normalizer();
        assert_eq!(nz.path("/ws?orchestrationProtocol=2&wsTicket=abc%20def123"), "/ws?orchestrationProtocol=2&wsTicket=<ws-ticket>");
        assert_eq!(nz.text("abc def123"), "<ws-ticket>");
        assert_eq!(nz.path("/plain"), "/plain");
    }

    #[test]
    fn given_bodies_when_masked_then_json_and_form_are_parsed_and_long_text_is_truncated() {
        let c = presets::t3().compile().unwrap();
        let mut nz = c.normalizer();
        assert_eq!(nz.body("", "text/plain"), json!(null));
        assert_eq!(nz.body("{\"pid\": 41}", "application/json"), json!({"pid": "<pid>"}));
        assert_eq!(nz.body("a=1&b=x%2Fy", "application/x-www-form-urlencoded"), json!({"a": "1", "b": "x/y"}));
        let long = "z".repeat(2500);
        assert_eq!(nz.body(&long, "text/plain"), json!(format!("{}...<truncated 500>", "z".repeat(2000))));
        assert_eq!(nz.body("{not json", "application/json"), json!("{not json"));
    }

    #[test]
    fn given_path_and_pointer_rules_when_an_interaction_is_masked_then_only_the_observed_side_changes() {
        use crate::cassette::{Dir, Interaction, Outcome, Step, StepKind};
        let rules = RuleSet::new(vec![
            Rule::Path { placeholder: "<world>".into(), value: "/tmp/run-123".into() },
            Rule::Pointer { pointer: "/steps/0/payload/volatile".into(), placeholder: "<volatile>".into(), interaction: Some("only-me".into()) },
        ]);
        let c = rules.compile().unwrap();
        let it = |id: &str| Interaction {
            id: id.into(),
            row: None,
            op: None,
            input: json!({"cwd": "/tmp/run-123"}),
            steps: vec![Step { dir: Dir::Ret, channel: None, kind: StepKind::Json, rid: None, len: None, payload: json!({"file": "/tmp/run-123/a", "volatile": 5}) }],
            effects: vec![],
            outcome: Outcome::default(),
            tags: vec![],
        };
        let masked = c.interaction(&it("only-me"));
        assert_eq!(masked.input, json!({"cwd": "/tmp/run-123"}), "the input is what the driver is given and stays verbatim");
        assert_eq!(masked.steps[0].payload, json!({"file": "<world>/a", "volatile": "<volatile>"}));
        assert_eq!(c.interaction(&it("other")).steps[0].payload["volatile"], 5);
    }

    #[test]
    fn given_a_bad_pattern_or_schema_when_compiled_then_the_error_names_it() {
        let bad = RuleSet::new(vec![Rule::Token { pattern: "(".into(), placeholder: "<x>".into() }]);
        assert!(bad.compile().err().unwrap().to_string().contains("\"(\""));
        let mut other = presets::t3();
        other.schema = "mimic-rules/2".into();
        assert!(other.compile().err().unwrap().to_string().contains("mimic-rules/2"));
    }

    #[test]
    fn given_a_rule_set_when_one_rule_changes_then_its_hash_changes_and_key_order_does_not_matter() {
        let a = presets::t3();
        let mut b = a.clone();
        assert_eq!(a.hash(), b.hash());
        b.rules.push(Rule::UuidOrdinal);
        assert_ne!(a.hash(), b.hash());
        let one: RuleSet = serde_json::from_str(r#"{"schema":"mimic-rules/1","rules":[{"kind":"sequence","keys":["a"],"placeholder":"<seq>"}]}"#).unwrap();
        let two: RuleSet = serde_json::from_str(r#"{"rules":[{"placeholder":"<seq>","keys":["a"],"kind":"sequence"}],"schema":"mimic-rules/1"}"#).unwrap();
        assert_eq!(one.hash(), two.hash());
    }
}

mod allowlist {
    use super::*;

    #[test]
    fn given_the_t3_file_shape_when_parsed_then_groups_and_reasons_load_and_an_entry_without_a_reason_is_refused() {
        let ok = Allowlist::parse(r#"{"ignored":[{"path":"label","reason":"host name","phase":"1"}],"shapeOnly":[{"path":"a.b","reason":"r","phase":"1"}]}"#).unwrap();
        assert_eq!((ok.ignored.len(), ok.omitted.len(), ok.shape_only.len()), (1, 0, 1));
        let err = Allowlist::parse(r#"{"omitted":[{"path":"x","reason":"","phase":"1"}]}"#).unwrap_err().to_string();
        assert!(err.contains("omitted") && err.contains("reason"), "{err}");
    }

    #[test]
    fn given_an_entry_of_any_group_without_its_reason_or_phase_when_parsed_then_the_refusal_names_its_path_and_the_missing_field() {
        for group in ["ignored", "omitted", "shapeOnly"] {
            for field in ["reason", "phase"] {
                let mut entry = json!({"path": "some.path", "reason": "why", "phase": "1"});
                entry.as_object_mut().unwrap().remove(field);
                let err = Allowlist::parse(&json!({ group: [entry] }).to_string()).unwrap_err().to_string();
                assert!(err.contains(group) && err.contains("some.path") && err.contains(field), "{err}");
            }
        }
    }

    #[test]
    fn given_a_pin_bound_entry_when_the_pin_moves_then_it_expires_and_unbound_entries_stay() {
        let list = Allowlist { ignored: vec![entry("a", Some("sha256:old")), entry("b", None), entry("c", Some("sha256:new"))], ..Allowlist::default() };
        let (live, expired) = list.active(Some("sha256:new"));
        assert_eq!(live.ignored.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["b", "c"]);
        assert_eq!(expired.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn given_an_entry_when_matched_then_array_indices_do_not_count_and_a_suffix_must_start_at_a_segment() {
        let list = [entry("platform.os", None)];
        assert!(Allowlist::find(&list, "frame.[3].platform.os").is_some());
        assert!(Allowlist::find(&list, "platform.os").is_some());
        assert!(Allowlist::find(&list, "xplatform.os").is_none());
    }
}

mod differ {
    use super::*;

    fn run(expected: serde_json::Value, actual: serde_json::Value, allow: &Allowlist) -> Vec<crate::differ::Diff> {
        diff_json(&expected, &actual, allow, &DiffOptions::default())
    }

    #[test]
    fn given_two_values_when_diffed_then_missing_and_type_and_length_are_breaking_and_extra_and_value_are_drift() {
        let d = run(json!({"a": 1, "gone": 1, "t": "x", "arr": [1, 2]}), json!({"a": 2, "t": 5, "arr": [1], "new": 1}), &Allowlist::default());
        let by = |p: &str| d.iter().find(|d| d.path == p).unwrap_or_else(|| panic!("no diff at {p}: {d:?}"));
        assert_eq!((by("a").kind, by("a").severity), (DiffKind::Changed, Severity::Drift));
        assert_eq!((by("gone").kind, by("gone").severity), (DiffKind::Missing, Severity::Breaking));
        assert_eq!((by("t").kind, by("t").severity), (DiffKind::Kind, Severity::Breaking));
        assert_eq!((by("arr").kind, by("arr").severity), (DiffKind::Changed, Severity::Breaking));
        assert_eq!((by("new").kind, by("new").severity), (DiffKind::Extra, Severity::Drift));
    }

    #[test]
    fn given_equal_values_when_diffed_then_nothing_is_reported() {
        let v = json!({"a": [1, {"b": null}], "c": "x"});
        assert!(run(v.clone(), v, &Allowlist::default()).is_empty());
    }

    #[test]
    fn given_shape_only_when_only_the_value_differs_then_it_is_allowlisted_and_a_type_change_still_breaks() {
        let allow = Allowlist { shape_only: vec![entry("id", None)], ..Allowlist::default() };
        let same_type = run(json!({"id": "a"}), json!({"id": "b"}), &allow);
        assert_eq!(crate::differ::severity_of(&same_type), Severity::None);
        assert_eq!(same_type.len(), 1);
        assert_eq!(crate::differ::severity_of(&run(json!({"id": "a"}), json!({"id": 1}), &allow)), Severity::Breaking);
    }

    #[test]
    fn given_omitted_when_the_candidate_drops_that_key_then_it_is_allowed_but_not_for_another_key() {
        let allow = Allowlist { omitted: vec![entry("optional", None)], ..Allowlist::default() };
        assert_eq!(crate::differ::severity_of(&run(json!({"optional": 1}), json!({}), &allow)), Severity::None);
        assert_eq!(crate::differ::severity_of(&run(json!({"required": 1}), json!({}), &allow)), Severity::Breaking);
    }

    #[test]
    fn given_the_t3_options_when_a_status_changes_or_a_capability_flag_flips_then_status_breaks_and_the_flag_is_only_reported() {
        let opts = presets::t3_diff_options();
        let ret = |payload: serde_json::Value| Interaction { id: "x".into(), row: None, op: None, input: json!({}), steps: vec![Step { dir: Dir::Ret, channel: Some("http".into()), kind: StepKind::Json, rid: None, len: None, payload }], effects: Vec::new(), outcome: Default::default(), tags: Vec::new() };
        let status = crate::differ::diff_interaction(&ret(json!({"status": 200, "body": {"status": "a"}})), &ret(json!({"status": 500, "body": {"status": "a"}})), &Allowlist::default(), &opts);
        assert_eq!(status[0].severity, Severity::Breaking);
        let flag = diff_json(&json!({"capabilities": {"x": true}}), &json!({"capabilities": {"x": false}}), &Allowlist::default(), &opts);
        assert_eq!(crate::differ::severity_of(&flag), Severity::None);
        assert!(flag[0].allowlisted.is_some());
        let nested = crate::differ::diff_interaction(&ret(json!({"status": 200, "body": {"status": "a"}})), &ret(json!({"status": 200, "body": {"status": "b"}})), &Allowlist::default(), &opts);
        assert_eq!(nested[0].severity, Severity::Drift, "only the status of the answer is behaviour");
    }
}

mod fold {
    use crate::allowlist::Allowlist;
    use crate::cassette::{Dir, Interaction, Step, StepKind};
    use crate::differ::{diff_interaction, severity_of, Severity};
    use crate::presets;
    use serde_json::{json, Value};

    fn frame(dir: Dir, rid: &str, payload: Value) -> Step {
        Step { dir, channel: Some("ws".into()), kind: StepKind::Json, rid: Some(rid.into()), len: None, payload }
    }

    fn chunk(values: Value) -> Step {
        frame(Dir::S2c, "4", json!({"_tag": "Chunk", "requestId": "4", "values": values}))
    }

    fn exit(causes: usize) -> Step {
        frame(Dir::S2c, "4", json!({"_tag": "Exit", "requestId": "4", "exit": {"_tag": "Failure", "cause": vec![json!({"_tag": "Interrupt"}); causes]}}))
    }

    fn ack() -> Step {
        frame(Dir::C2s, "4", json!({"_tag": "Ack", "requestId": "4"}))
    }

    fn it(steps: Vec<Step>) -> Interaction {
        Interaction { id: "x".into(), row: None, op: None, input: json!({}), steps, effects: Vec::new(), outcome: Default::default(), tags: Vec::new() }
    }

    fn severity(expected: Vec<Step>, actual: Vec<Step>) -> Severity {
        severity_of(&diff_interaction(&it(expected), &it(actual), &Allowlist::default(), &presets::t3_diff_options()))
    }

    fn snapshot() -> Value {
        json!({"kind": "snapshot", "n": 1})
    }

    fn update(n: u64) -> Value {
        json!({"kind": "project.updated", "n": n})
    }

    #[test]
    fn given_a_subscription_split_into_other_chunks_when_diffed_then_only_the_values_and_their_order_count() {
        let recorded = vec![chunk(json!([snapshot()])), chunk(json!([update(1)])), chunk(json!([update(2)])), exit(1)];
        // one chunk for all values, or another split: clean
        assert_eq!(severity(recorded.clone(), vec![chunk(json!([snapshot(), update(1), update(2)])), exit(1)]), Severity::None);
        assert_eq!(severity(recorded.clone(), vec![chunk(json!([snapshot(), update(1)])), chunk(json!([update(2)])), exit(1)]), Severity::None);
        // a changed value is a drift, a lost value breaking, a swapped order a drift in each place
        assert_eq!(severity(recorded.clone(), vec![chunk(json!([snapshot(), update(1), update(9)])), exit(1)]), Severity::Drift);
        assert_eq!(severity(recorded.clone(), vec![chunk(json!([snapshot(), update(1)])), exit(1)]), Severity::Breaking);
        assert_eq!(severity(recorded, vec![chunk(json!([snapshot(), update(2), update(1)])), exit(1)]), Severity::Drift);
    }

    #[test]
    fn given_a_snapshot_the_real_server_sent_twice_when_diffed_then_the_second_is_extra_replies_that_do_not_count() {
        let once = vec![chunk(json!([snapshot()])), chunk(json!([update(1)])), exit(1)];
        let twice = vec![chunk(json!([snapshot()])), chunk(json!([update(1)])), chunk(json!([snapshot()])), exit(1)];
        assert_eq!(severity(twice.clone(), once.clone()), Severity::None);
        assert_eq!(severity(once, twice), Severity::None);
    }

    #[test]
    fn given_one_interrupt_per_fiber_when_diffed_then_runs_of_equal_causes_count_once_and_a_different_cause_does_not() {
        assert_eq!(severity(vec![exit(7)], vec![exit(2)]), Severity::None);
        let other = frame(Dir::S2c, "4", json!({"_tag": "Exit", "requestId": "4", "exit": {"_tag": "Failure", "cause": [{"_tag": "Fail"}]}}));
        assert_eq!(severity(vec![exit(2)], vec![other]), Severity::Drift);
    }

    #[test]
    fn given_a_capability_flag_the_real_server_advertised_when_this_server_does_not_then_it_is_allowed_and_any_other_boolean_is_not() {
        let diff = |expected: Value, actual: Value| severity_of(&crate::differ::diff_json(&expected, &actual, &Allowlist::default(), &presets::t3_diff_options()));
        assert_eq!(diff(json!({"capabilities": {"sessions": true}}), json!({"capabilities": {"sessions": false}})), Severity::None);
        assert_eq!(diff(json!({"capabilities": {"limit": "a"}}), json!({"capabilities": {"limit": "b"}})), Severity::Drift);
        assert_eq!(diff(json!({"settings": {"enabled": true}}), json!({"settings": {"enabled": false}})), Severity::Drift);
    }

    #[test]
    fn given_acknowledgements_when_their_number_differs_then_the_client_side_is_not_compared() {
        assert_eq!(severity(vec![ack(), ack(), chunk(json!([snapshot()]))], vec![ack(), chunk(json!([snapshot()]))]), Severity::None);
    }
}

mod import {
    use crate::cassette::{Dir, Kind, StepKind, ToolInfo};
    use crate::import::{check, export, import, Legacy, EXEC_HOSTED, EXEC_RECORDING};
    use serde_json::json;

    fn tool() -> ToolInfo {
        ToolInfo { mimic: "0".into(), git: None }
    }

    fn legacy(path: &str, text: &str) -> Legacy {
        Legacy { path: path.into(), bytes: text.as_bytes().to_vec() }
    }

    fn hosted_file() -> Legacy {
        let doc = json!({"action": "hosted_x", "exchanges": [
            {"name": "a", "cli": [{"program": "gh", "args": ["pr", "view"], "stdout": "o\n", "stderr": "", "exit_code": 0, "stdin": "body"}, {"program": "gh", "args": ["api"], "stdout": "", "stderr": "e", "exit_code": 4}],
             "request": {"id": "h1", "tool": "hosted_x", "args": {"z": 1, "a": [1, 2]}}, "starts": [["gh", "pr", "view"]], "response": {"id": "h1", "ok": true}},
            {"name": "b", "files": {"f.txt": "x"}, "cli": [], "request": {"id": "h2"}, "starts": [], "response": {"id": "h2", "ok": false}}]});
        legacy("fx/hosted_x.json", &doc.to_string())
    }

    fn recordings() -> Vec<Legacy> {
        let index = json!({"note": "n", "recordings": [{"name": "r1", "driver": "codex", "file": "r1.jsonl", "scenario": "s", "provenance": "documented-format", "source": "src", "expect": ["text"]}]});
        vec![legacy("d/recordings.json", &index.to_string()), legacy("d/recordings/r1.jsonl", "{\"a\":1}\n{\"b\":2}\n")]
    }

    #[test]
    fn given_hosted_fixtures_when_imported_then_cli_calls_become_steps_and_the_set_is_imported_never_recorded() {
        // Given a fixture with a two-call exchange (one with stdin) and a call-less one with files
        let set = import(EXEC_HOSTED, &[hosted_file()], tool()).unwrap();
        // Then the interactions are per exchange, the calls are steps in order and the provenance is imported with its source file
        let c = &set.cassette;
        assert_eq!(c.header.provenance.kind, Kind::Imported);
        assert!(c.problems().is_empty(), "{:?}", c.problems());
        assert_eq!(c.interactions.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["hosted_x/00", "hosted_x/01"]);
        let dirs: Vec<Dir> = c.interactions[0].steps.iter().map(|s| s.dir).collect();
        assert_eq!(dirs, [Dir::Call, Dir::Stdin, Dir::Stdout, Dir::Stderr, Dir::Exit, Dir::Call, Dir::Stdout, Dir::Stderr, Dir::Exit, Dir::S2c]);
        assert_eq!(c.interactions[0].steps[4].kind, StepKind::Exit);
        assert_eq!(c.header.provenance.imported_from.len(), 1);
    }

    #[test]
    fn given_an_imported_set_when_checked_then_it_rebuilds_losslessly_and_an_edited_interaction_is_found() {
        for (format, files) in [(EXEC_HOSTED, vec![hosted_file()]), (EXEC_RECORDING, recordings())] {
            // Given an imported set
            let mut set = import(format, &files, tool()).unwrap();
            // Then every legacy file rebuilds to the hash recorded at import
            assert!(check(format, &set).unwrap().iter().all(|f| f.ok), "{format}");
            assert_eq!(export(format, &set).unwrap().len(), files.len());
            // When one interaction is changed and the set resealed
            set.cassette.interactions[0].steps[1].payload = json!("changed");
            set.cassette.seal();
            // Then the check names the file that no longer rebuilds
            let lost: Vec<_> = check(format, &set).unwrap().into_iter().filter(|f| !f.ok).collect();
            assert_eq!(lost.len(), 1, "{format}: {lost:?}");
        }
    }

    #[test]
    fn given_legacy_files_that_cannot_round_trip_or_claim_recorded_when_imported_then_they_are_refused() {
        // Given a recording that claims to be recorded, one without a final newline and a hosted file whose action is not its name
        let mut claimed = recordings();
        claimed[0] = legacy("d/recordings.json", &String::from_utf8_lossy(&claimed[0].bytes).replace("documented-format", "recorded"));
        let mut loose = recordings();
        loose[1] = legacy("d/recordings/r1.jsonl", "{\"a\":1}");
        let renamed = legacy("fx/other.json", &String::from_utf8_lossy(&hosted_file().bytes));
        // Then each import fails and says why
        assert!(import(EXEC_RECORDING, &claimed, tool()).unwrap_err().0.contains("not importable"));
        assert!(import(EXEC_RECORDING, &loose, tool()).unwrap_err().0.contains("round-trip"));
        assert!(import(EXEC_HOSTED, &[renamed], tool()).unwrap_err().0.contains("not the file's name"));
    }
}

mod fixtures {
    use crate::fixtures::glob;

    #[test]
    fn given_globs_when_matched_then_star_stays_in_a_segment_and_double_star_spans_segments_or_none() {
        for (pattern, path, want) in [
            ("*/tests/fixtures/**", "a/tests/fixtures/x.json", true),
            ("*/tests/fixtures/**", "a/tests/fixtures/d/e/x.json", true),
            ("*/tests/fixtures/**", "a/b/tests/fixtures/x.json", false),
            ("a/**/x.json", "a/x.json", true),
            ("a/**/x.json", "a/b/c/x.json", true),
            ("a/*.json", "a/b/x.json", false),
            ("a/x?.json", "a/x1.json", true),
            ("a/x?.json", "a/x.json", false),
            ("a/*", "a", false),
        ] {
            assert_eq!(glob(pattern, path), want, "{pattern} vs {path}");
        }
    }
}

mod wire {
    use crate::cassette::{Dir, StepKind};
    use crate::presets;
    use crate::wire::{close_info, encode_frame, steps, FrameParser, WireEvent, OP_BINARY, OP_CLOSE, OP_TEXT};

    #[test]
    fn given_frames_of_every_length_class_when_the_bytes_arrive_in_pieces_then_each_message_comes_out_whole_once() {
        // Given a short, a 16-bit and a 64-bit length frame, masked and not, and a fragmented text message, as one byte stream
        let sizes = [5usize, 300, 70_000];
        let mut stream: Vec<u8> = sizes.iter().enumerate().flat_map(|(i, n)| encode_frame(OP_BINARY, &vec![i as u8; *n], (i % 2 == 0).then_some([9, 8, 7, 6]))).collect();
        let mut first = encode_frame(OP_TEXT, b"hel", None);
        first[0] &= 0x7f;
        stream.extend(first);
        stream.extend(encode_frame(0, b"lo", None));
        // When it is fed in uneven pieces
        let mut parser = FrameParser::default();
        let out: Vec<_> = stream.chunks(997).flat_map(|c| parser.push(c)).collect();
        // Then every message is whole and in order
        let lens: Vec<usize> = out.iter().map(|m| m.payload.len()).collect();
        assert_eq!((lens, out.last().map(|m| (m.opcode, m.payload.clone()))), (vec![5, 300, 70_000, 5], Some((OP_TEXT, b"hello".to_vec()))));
    }

    #[test]
    fn given_a_close_payload_when_read_then_code_and_reason_come_out_and_an_empty_one_has_neither() {
        let payload = [&1001u16.to_be_bytes()[..], b"going away"].concat();
        assert_eq!(close_info(&payload), (Some(1001), "going away".to_string()));
        assert_eq!(close_info(&[]), (None, String::new()));
        assert_eq!(FrameParser::default().push(&encode_frame(OP_CLOSE, &payload, None))[0].opcode, OP_CLOSE);
    }

    #[test]
    fn given_wire_events_when_made_into_steps_then_secrets_learned_from_bodies_mask_the_paths_and_binary_stays_whole() {
        // Given an exchange whose body carries a ticket the path repeats, a text frame that is not JSON, a binary frame and a dropped connection
        let events = vec![
            WireEvent::Http {
                method: "POST".into(),
                path: "/pair?ticket=abcdefgh".into(),
                req_headers: vec![("Content-Type".into(), "application/json".into()), ("Host".into(), "x".into())],
                req_body: r#"{"ticket":"abcdefgh"}"#.into(),
                status: 200,
                res_headers: Vec::new(),
                res_body: String::new(),
            },
            WireEvent::Ws { dir: Dir::C2s, opcode: OP_TEXT, data: b"plain words".to_vec() },
            WireEvent::Ws { dir: Dir::S2c, opcode: OP_BINARY, data: vec![0, 1, 2, 3] },
            WireEvent::WsClose { dir: None, code: None, reason: String::new() },
        ];
        // When they are made into steps under the T3 rules
        let compiled = presets::t3().compile().unwrap();
        let made = steps(&events, &mut compiled.normalizer());
        // Then the ticket is a mask everywhere, the header the rules drop is gone, and the binary frame keeps its length and bytes
        assert_eq!(made[0].payload, serde_json::json!({"method": "POST", "path": "/pair?ticket=<ws-ticket>", "headers": {"content-type": "application/json"}, "body": {"ticket": "<ws-ticket>"}}));
        assert_eq!(made[2].kind, StepKind::Text);
        assert_eq!((made[3].kind, made[3].len, made[3].payload.clone()), (StepKind::Binary, Some(4), serde_json::json!("AAECAw==")));
        assert_eq!(made[4].payload["tcp"], serde_json::json!(true));
    }
}
