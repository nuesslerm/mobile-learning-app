//! The recordings, as the interactions of their cassette set, and the provenance the set's header gives them.
use exec_testkit::recordings;
use serde_json::Value;
use std::collections::BTreeSet;

#[test]
fn every_interaction_of_the_set_is_a_recording_once_with_a_provenance_and_each_line_is_a_json_object() {
    // Given the recordings of the set and the interactions of its cassette
    let (all, set) = (recordings::all(), recordings::cassette());
    // Then they are the same, each name once
    let names: BTreeSet<String> = all.iter().map(|r| r.name.clone()).collect();
    let ids: BTreeSet<String> = set.interactions.iter().map(|i| i.id.clone()).collect();
    assert_eq!((names.len(), &names), (all.len(), &ids));
    for recording in &all {
        // And each says how it was made, in words, and every line is a JSON object
        assert!(["authored", "imported", "recorded"].contains(&recording.provenance.as_str()), "{}: {}", recording.name, recording.provenance);
        assert!(!recording.source.is_empty() && !recording.scenario.is_empty() && !recording.expect.is_empty(), "{}", recording.name);
        for line in &recording.lines {
            assert!(serde_json::from_str::<Value>(line).is_ok_and(|v| v.is_object()), "{}: {line}", recording.name);
        }
    }
}

#[test]
fn every_driver_has_a_recording_that_answers_in_text_one_that_calls_a_tool_and_one_that_fails() {
    // Given the recordings by driver
    let all = recordings::all();
    for driver in ["claudeAgent", "codex", "opencode"] {
        let expects: Vec<&Vec<String>> = all.iter().filter(|r| r.driver == driver).map(|r| &r.expect).collect();
        // Then the driver's recordings between them expect a text, a tool call and an error
        for kind in ["text", "tool.started", "error"] {
            assert!(expects.iter().any(|e| e.iter().any(|k| k == kind)), "{driver} has no recording that expects {kind}");
        }
    }
}

#[test]
fn a_set_that_is_not_from_a_run_of_the_cli_says_so_in_its_kind_and_note_and_names_the_gap_that_records_it() {
    // Given the set's provenance
    let set = recordings::cassette();
    let gap = "docs/gaps/exec-testkit-recordings-not-from-real-clis.md";
    // When its kind is not `recorded`, Then its note says no run was made and names the gap file, which exists and names the provenance field
    assert!(["authored", "imported"].contains(&set.provenance.kind.as_str()), "{}", set.provenance.kind);
    assert!(set.provenance.note.contains("not recorded from a run of the CLI") && set.provenance.note.contains(gap), "the note must name {gap}");
    let text = std::fs::read_to_string(recordings::root().join("../../..").join(gap)).unwrap_or_else(|e| panic!("{gap}: {e}"));
    assert!(text.contains("provenance.kind"), "{gap} must name the provenance field");
}

#[test]
fn an_interaction_plays_through_the_scripted_cli_as_the_lines_it_holds_and_exits_cleanly() {
    // Given each interaction of the set
    for i in &recordings::cassette().interactions {
        // When it is turned into the scripted CLI's turn, Then the turn prints exactly its stdout lines and ends with code 0 and no stderr
        let turn = i.turn();
        assert_eq!((turn.exit_code, turn.stderr.as_str(), turn.steps.len()), (0, "", i.lines().len()), "{}", i.id);
        assert_eq!(turn, exec_testkit::Turn::replaying(&recordings::named(&i.id).lines), "{}", i.id);
    }
}
