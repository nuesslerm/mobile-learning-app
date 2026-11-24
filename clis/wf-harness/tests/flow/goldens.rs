//! `wf test goldens`: a published run's named screenshots become the goldens of its harness. The bucket is a mock HTTP service.
use crate::support::{MockReply, MockServer};
use serde_json::{json, Value};
use std::path::Path;

#[test]
fn a_published_runs_named_screenshots_become_the_goldens_of_its_harness() {
    // Given a published Android run whose bundle lists an assert_screenshot capture with its sidecar, expected and diff images
    // beside other files
    let s = crate::support::SlotSandbox::new("wf-flow-goldens-");
    let bucket = MockServer::start(None);
    let read = format!("http://127.0.0.1:{}/o", bucket.port);
    let file = |p: &str| json!({ "path": p, "url": format!("{read}/verification/test-runs/run-1/{p}") });
    bucket.routes(&[
        ("GET /o/verification/test-runs/run-1/bundle.json", MockReply::json(200, json!({ "runId": "run-1", "harness": "android", "files": [file("out/result.json"), file("report.html"), file("goldens/editor-two-sum-result.actual.png"), file("goldens/editor-two-sum-result.expected.png"), file("goldens/editor-two-sum-result.diff.png"), file("goldens/editor-two-sum-result.json"), file("goldens.json")] }))),
        ("GET /o/verification/test-runs/run-1/goldens/editor-two-sum-result.actual.png", MockReply::text(200, "png bytes")),
        ("GET /o/verification/test-runs/run-1/goldens/editor-two-sum-result.json", MockReply::text(200, "{\"name\":\"editor-two-sum-result\"}")),
        ("GET /o/verification/test-runs/run-2/bundle.json", MockReply::json(200, json!({ "runId": "run-2", "harness": "android", "files": [file("report.html")] }))),
    ]);
    let env = [("VERIFICATION_ARTIFACTS_READ_URL", read.as_str())];
    let dir = Path::new(&s.file("README.md", "")).with_file_name("goldens").display().to_string();

    // When its run id is accepted as goldens
    let r = s.run(&["test", "goldens", "run-1", "--goldens-dir", &dir], &env);
    // Then the capture and its sidecar are the goldens, under the golden's own name, and only they were written
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let v: Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!((v["platform"].as_str(), v["goldens"].clone()), (Some("android"), json!(["editor-two-sum-result"])), "{v}");
    assert_eq!(std::fs::read_to_string(format!("{dir}/editor-two-sum-result.png")).unwrap(), "png bytes");
    assert_eq!(std::fs::read_to_string(format!("{dir}/editor-two-sum-result.json")).unwrap(), "{\"name\":\"editor-two-sum-result\"}");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);

    // When a run with no named screenshot is accepted, and when no run is named
    let none = s.run(&["test", "goldens", "run-2", "--goldens-dir", &dir], &env);
    let bare = s.run(&["test", "goldens"], &env);
    // Then each fails with the reason and the next step
    let e: Value = serde_json::from_str(none.stdout.trim()).unwrap();
    assert!(none.exit_code == 1 && e["error"].as_str().unwrap().contains("no named screenshots") && e["next"].as_str().unwrap().contains("wf test goldens"), "{e}");
    assert!(bare.exit_code == 1 && bare.stdout.contains("wf test goldens <runId|bundle URL>"), "{}", bare.stdout);
}

#[test]
fn ios_reference_comparison_keeps_the_source_and_writes_a_diff() {
    // Given a portrait screenshot, a different design reference, and the expected accessibility element
    let s = crate::support::SlotSandbox::new("wf-ios-reference-");
    let png = |value: u8| {
        wf_harness::golden::encode(&wf_harness::golden::Image {
            width: 4,
            height: 8,
            rgba: [value, value, value, 255].repeat(32),
        }).unwrap()
    };
    let captured = s.file("captured.png", png(0));
    let source = png(255);
    let reference = s.file("reference.png", &source);
    s.stub("idb", r#"case "$1" in
        ui) echo '[{"AXUniqueId":"Editor"}]' ;;
        screenshot) cat "$CAPTURED" ;;
    esac"#);
    s.stub("sips", r#"while [ "$2" != "--out" ] && [ "$#" -gt 2 ]; do shift; done
        cp "$1" "$3""#);

    // When the simulator screenshot is compared directly with the supplied reference
    let r = s.run(&["test", "ios-visual", "editor", "--device", "test-sim", "--expect", "Editor", "--reference", &reference, "--out", "run"], &[("CAPTURED", &captured)]);

    // Then the comparison fails with both images and a diff, and the input reference remains intact
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let result: Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!(result["verdict"], "mismatch");
    assert_eq!(result["differing_pixels"], 32);
    for field in ["actual", "expected", "diff"] {
        assert!(s.repo.join(result[field].as_str().unwrap()).is_file(), "{result}");
    }
    assert_eq!(std::fs::read(&reference).unwrap(), source);

    // When a comparison is combined with an update, then the command rejects it
    let update = s.run(&["test", "ios-visual", "editor", "--device", "test-sim", "--expect", "Editor", "--reference", &reference, "--update-goldens"], &[]);
    assert_eq!(update.exit_code, 2, "{}{}", update.stdout, update.stderr);
    assert!(update.stdout.contains("cannot be used with --update-goldens"), "{}", update.stdout);
    assert_eq!(std::fs::read(reference).unwrap(), source);
}
