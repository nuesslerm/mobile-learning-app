//! Which port a call goes to: the client's list of CI routes is the server's table, so a route added to one cannot be forgotten in
//! the other. Given/When/Then.
use regex::Regex;
use std::collections::BTreeSet;
use wf_vmops::vmops::{is_ci_route, CI_ROUTES};

fn server_rows() -> Vec<(String, String, String)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/vm-ops-api/core/src/domain/exposure.rs");
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let row = Regex::new(r#"(?m)^\s*spec\("\w+", "(GET|POST)", "([^"]+)", (\w+)\),$"#).unwrap();
    row.captures_iter(&source).map(|c| (c[1].to_string(), c[2].to_string(), c[3].to_string())).collect()
}

#[test]
fn given_the_servers_route_table_when_it_is_read_then_the_client_sends_a_route_to_the_ci_port_exactly_when_the_server_serves_it_there() {
    // Given the rows of the server's table
    let rows = server_rows();

    // When the routes the CI port serves are picked out
    let served: BTreeSet<(String, String)> = rows.iter().filter(|(_, _, exposure)| exposure != "AdminOnly").map(|(m, p, _)| (m.clone(), p.clone())).collect();
    let listed: BTreeSet<(String, String)> = CI_ROUTES.iter().map(|(m, p)| (m.to_string(), p.to_string())).collect();

    // Then the client lists the same set, and the table is not an empty read
    assert!(rows.len() >= 20 && rows.iter().any(|(_, _, exposure)| exposure == "AdminOnly"), "{rows:?}");
    assert_eq!(listed, served);
}

#[test]
fn given_a_path_with_a_query_when_it_is_classified_then_the_ci_routes_match_and_the_exec_script_and_the_rest_do_not() {
    // Given calls as the client makes them, with their queries
    let ci = [("GET", "/health"), ("GET", "/services/leetcode-backend/ps"), ("POST", "/services/leetcode-backend/redeploy?tag=t&ref=dev"), ("POST", "/redeploy?ref=a&tag=b"), ("GET", "/redeploy/status"), ("POST", "/redeploy/smoke/stop")];
    let admin = [
        ("POST", "/services/agent-shell/exec-script"),
        ("GET", "/services/agent-shell/exec-script/0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab"),
        ("POST", "/services/leetcode-backend/retire"),
        ("POST", "/drill?action=stop"),
        ("GET", "/services/leetcode-backend/logs?tail=5"),
        ("POST", "/health"),
        ("GET", "/services//ps"),
    ];

    // When each is classified, Then the CI routes are the CI port's and everything else is the admin port's
    for (method, path) in ci {
        assert!(is_ci_route(method, path), "{method} {path}");
    }
    for (method, path) in admin {
        assert!(!is_ci_route(method, path), "{method} {path}");
    }
}
