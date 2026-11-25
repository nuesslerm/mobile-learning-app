use crate::rig;
use crate::rig::{Chain, Phone};
use exec_testkit::Case;
use serde_json::json;

pub(super) fn exporter() -> (tracing::Dispatch, telemetry::Telemetry) {
    let endpoint =
        std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").expect("run under wf test run --performance");
    let destination = telemetry::Destination::new(endpoint, "");
    telemetry::build(
        telemetry::source!("orca-performance"),
        vec![],
        Some(destination),
    )
}

#[test]
#[ignore = "measured through wf test run --performance"]
fn given_a_paired_orca_phone_when_it_opens_an_artifact_then_rca_reads_the_executor_file() {
    let (dispatch, exporter) = exporter();
    let _dispatch = tracing::dispatcher::set_default(&dispatch);
    let chain = Chain::start(None);
    let mut case = Case::default();
    let mut phone = Phone::connect(&mut case, &chain);
    let path = chain.repo.path().join("build.log");
    let content = "x".repeat(4096);
    std::fs::write(&path, &content).unwrap();
    for _ in 0..30 {
        let _sample = tracing::info_span!("orca.artifact.open").entered();
        let resolved = tracing::info_span!("orca.artifact.resolve").in_scope(|| {
            phone.call(
                &mut case,
                &chain,
                &[],
                "files.resolveTerminalPath",
                json!({"worktree":"id:w1::","pathText":path}),
            )
        });
        rig::answered(&case, "artifact.stat", true);
        let read = tracing::info_span!("orca.artifact.read").in_scope(|| {
            phone.call(&mut case, &chain, &[], "files.readTerminalArtifact", json!({"worktree":"id:w1::","grantId":resolved["result"]["openTarget"]["grantId"],"absolutePath":resolved["result"]["absolutePath"]}))
        });
        rig::answered(&case, "artifact.read", true);
        assert_eq!(read["result"]["content"], content);
    }
    exporter.shutdown();
}

#[test]
#[ignore = "measured through wf test run --performance"]
fn given_an_open_session_when_the_phone_requests_history_then_it_receives_the_rca_page() {
    let (dispatch, exporter) = exporter();
    let _dispatch = tracing::dispatcher::set_default(&dispatch);
    let chain = Chain::start(None);
    let mut case = Case::default();
    let mut phone = Phone::connect(&mut case, &chain);
    let created = phone.call(
        &mut case,
        &chain,
        &[],
        "agentSession.create",
        crate::agent_sessions::create_params("claude_perf", "create", None),
    );
    assert_eq!(created["result"]["ok"], true, "{created}");
    for _ in 0..30 {
        let page = tracing::info_span!("orca.session.history").in_scope(|| {
            phone.call(
                &mut case,
                &chain,
                &[],
                "agentSession.history",
                json!({"sessionId":"claude_perf","direction":"tail","limit":200}),
            )
        });
        rig::answered(&case, "thread.history", true);
        assert!(page["result"]["page"]["items"].is_array(), "{page}");
    }
    exporter.shutdown();
}
