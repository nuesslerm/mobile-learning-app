#[allow(dead_code)]
#[path = "integration/rig.rs"]
mod rig;
#[allow(dead_code, unused_imports)]
#[path = "flow/support/mod.rs"]
mod support;

use exec_testkit::Case;
use rig::{Chain, Phone};
use serde_json::json;

#[test]
#[ignore = "measured through wf bench trace"]
fn given_a_paired_t3_phone_when_it_browses_the_project_then_rca_reads_the_executor_file() {
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").expect("run under wf bench trace");
    let destination = telemetry::Destination::new(endpoint, "");
    let (dispatch, exporter) = telemetry::build(
        telemetry::source!("t3-performance"),
        vec![],
        Some(destination),
    );
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
