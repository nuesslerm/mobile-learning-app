//! `POST /clis/install` on the CI port: one waited sibling on the service's image with the `clis` volume, the tree hash it prints as the answer.
use crate::{expect_json, world};
use serde_json::json;
use vm_ops_api_integration::{case, ToolRule};

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

case!(clis_install_runs_one_sibling_with_the_clis_volume_and_answers_with_the_tree_it_prints, ["POST /clis/install"], {
    // Given the service with its CI port, and a docker whose run prints a tree hash
    let mut w = world();
    w.with_ci_port().rule(ToolRule::docker(&["run"]).answer("tree-1\n", "", 0));
    let svc = w.start();
    // When the CI port is posted the sha256 of the published build
    let reply = svc.ci_signed("POST", &format!("/clis/install?sha256={SHA}"), None);
    // Then it is 200 naming the tree, and docker ran one sibling that mounts the clis volume and no docker socket
    expect_json(&reply, 200, &json!({ "installed": true, "tree": "tree-1" }));
    let calls = svc.world().docker_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].iter().any(|a| a == "clis:/opt/clis") && !calls[0].iter().any(|a| a.contains("docker.sock")), "{:?}", calls[0]);
    // When the sha256 is not well formed, Then it is 400 and docker is asked nothing more
    expect_json(&svc.ci_signed("POST", "/clis/install?sha256=abc", None), 400, &json!({ "error": "sha256 must be 64 lowercase hex digits" }));
    assert_eq!(svc.world().docker_calls().len(), 1);
    // When the admin port is asked, Then it is not served
    assert_ne!(svc.signed("POST", &format!("/clis/install?sha256={SHA}"), None).status, 200);
});
