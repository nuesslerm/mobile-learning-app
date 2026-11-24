#[allow(dead_code)]
#[path = "integration/rig.rs"]
mod rig;

use exec_testkit::Case;
use rig::{Chain, Phone};
use serde_json::json;

#[test]
#[ignore = "measured through wf bench trace"]
fn given_a_paired_orca_phone_when_it_opens_an_artifact_then_rca_reads_the_executor_file() {
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").expect("run under wf bench trace");
    let destination = telemetry::Destination::new(endpoint, "");
    let (dispatch, exporter) = telemetry::build(
        telemetry::source!("orca-performance"),
        vec![],
        Some(destination),
    );
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
