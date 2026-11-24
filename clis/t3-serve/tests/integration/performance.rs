use crate::rig::{Chain, Phone};
use crate::support;
use exec_testkit::Case;
use serde_json::json;

pub(super) fn exporter() -> (tracing::Dispatch, telemetry::Telemetry) {
    let endpoint =
        std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").expect("run under wf test run --performance");
    let destination = telemetry::Destination::new(endpoint, "");
    telemetry::build(
        telemetry::source!("t3-performance"),
        vec![],
        Some(destination),
    )
}

#[test]
#[ignore = "measured through wf test run --performance"]
fn given_a_paired_t3_phone_when_it_browses_the_project_then_rca_reads_the_executor_file() {
    let (dispatch, exporter) = exporter();
    let _dispatch = tracing::dispatcher::set_default(&dispatch);
    let chain = Chain::start();
    let mut case = Case::default();
    let mut phone = Phone::connect(&mut case, &chain);
    let cwd = std::fs::canonicalize(chain.repo.path()).unwrap();
    let content = "x".repeat(4096);
    chain.repo.write("notes/build.log", &content);
    for _ in 0..30 {
        let _sample = tracing::info_span!("t3.project.open").entered();
        let listed = tracing::info_span!("t3.project.list").in_scope(|| {
            phone.call(
                &mut case,
                &chain,
                &[],
                "projects.listEntries",
                json!({"cwd":cwd,"directoryPath":"notes"}),
            )
        });
        assert_eq!(
            support::success(&listed)["entries"][0]["path"],
            "notes/build.log"
        );
        let read = tracing::info_span!("t3.project.read").in_scope(|| {
            phone.call(
                &mut case,
                &chain,
                &[],
                "projects.readFile",
                json!({"cwd":cwd,"relativePath":"notes/build.log"}),
            )
        });
        assert_eq!(support::success(&read)["contents"], content);
        let hits = case.calls().last().unwrap()["executor"].as_array().unwrap();
        assert!(hits
            .iter()
            .any(|hit| hit["tool"] == "read_file" && hit["ok"] == true));
    }
    exporter.shutdown();
}

#[test]
#[ignore = "measured through wf test run --performance"]
fn given_an_open_thread_when_the_phone_requests_its_projection_then_it_receives_the_turn_items() {
    let (dispatch, exporter) = exporter();
    let _dispatch = tracing::dispatcher::set_default(&dispatch);
    let chain = Chain::start();
    let mut case = Case::default();
    let mut phone = Phone::connect(&mut case, &chain);
    phone.seed(&mut case, &chain);
    for _ in 0..30 {
        let projection = tracing::info_span!("t3.thread.projection").in_scope(|| {
            phone.call(
                &mut case,
                &chain,
                &[],
                "orchestration.getThreadProjection",
                json!({"threadId":"t1"}),
            )
        });
        assert!(
            support::success(&projection)["turnItems"].is_array(),
            "{projection}"
        );
    }
    exporter.shutdown();
}
