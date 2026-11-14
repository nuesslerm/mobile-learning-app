//! `wf vm stock`: the Hetzner Cloud API served by a local double, the plans read from it and the change since the last run.
use crate::support::{fixtures_dir, run_wf, MockReply, MockServer, RunOpts};
use serde_json::{json, Value};

const TOKEN: &str = "hcloud-test-token-0123456789";

fn fixture() -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixtures_dir().join("hetzner-server-types.json")).unwrap()).unwrap()
}

fn serve(body: Value) -> MockServer {
    let server = MockServer::start(None);
    server.routes(&[("GET /server_types?per_page=50", MockReply::json(200, body))]);
    server
}

fn stock_with(server: &MockServer, token: Option<&str>, extra: &[&str]) -> crate::support::RunResult {
    let api = format!("http://127.0.0.1:{}", server.port);
    let mut args = vec!["vm", "stock", "--api", api.as_str()];
    args.extend_from_slice(extra);
    let mut opts = RunOpts::default().clean_env();
    if let Some(token) = token {
        opts = opts.env("HCLOUD_READ_TOKEN", token);
    }
    run_wf(&args, &opts)
}

fn stock(server: &MockServer, extra: &[&str]) -> crate::support::RunResult {
    stock_with(server, Some(TOKEN), extra)
}

fn json_of(r: &crate::support::RunResult) -> Value {
    serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}{}", r.stdout, r.stderr))
}

fn names(report: &Value) -> Vec<String> {
    report["plans"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect()
}

fn plan<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["plans"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{name} missing in {report}"))
}

fn turn_available(body: &mut Value, name: &str, location: &str) {
    let ty = body["server_types"].as_array_mut().unwrap().iter_mut().find(|t| t["name"] == name).unwrap();
    let place = ty["locations"].as_array_mut().unwrap().iter_mut().find(|l| l["name"] == location).unwrap();
    place["available"] = json!(true);
}

#[test]
fn the_default_families_are_listed_with_their_locations_and_only_cx23_is_orderable() {
    // Given the API answering with seven server types, in the shape of the real one
    let server = serve(fixture());
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state.json");

    // When the stock is read with the default families and a state file that does not exist yet
    let r = stock(&server, &["--state", state.to_str().unwrap()]);

    // Then only the cx and cax plans are listed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report = json_of(&r);
    assert_eq!((report["ok"].clone(), report["provider"].clone(), report["source"].clone()), (json!(true), json!("hetzner"), json!("api")));
    assert_eq!(names(&report), ["cx23", "cx33", "cx43", "cax21", "cax31"]);

    // and cx23 is available in its three locations while the others are unavailable
    assert_eq!(
        plan(&report, "cx23"),
        &json!({ "name": "cx23", "arch": "x86", "category": "cost_optimized", "vcpu": 2, "ramGb": 4, "diskGb": 40, "priceMonthExVatEur": plan(&report, "cx23")["priceMonthExVatEur"], "locations": { "fsn1": true, "nbg1": true, "hel1": true }, "available": true, "availableIn": ["fsn1", "nbg1", "hel1"] })
    );
    assert!(plan(&report, "cx23")["priceMonthExVatEur"].is_number());
    assert_eq!(plan(&report, "cax21")["arch"], json!("arm64"));
    for name in ["cx33", "cx43", "cax21", "cax31"] {
        assert_eq!((plan(&report, name)["available"].clone(), plan(&report, name)["availableIn"].clone()), (json!(false), json!([])), "{name}");
    }
    assert_eq!(report["available"], json!(["cx23"]));
    assert_eq!(report["summary"], json!("orderable now: cx23 (fsn1,nbg1,hel1); unavailable: cx33, cx43, cax21, cax31"));

    // and it is the first run, with the state kept free of the token
    assert_eq!((report["firstRun"].clone(), report["changed"].clone()), (json!(true), json!([])));
    assert!(!std::fs::read_to_string(&state).unwrap().contains(TOKEN));

    // and the request carried the bearer header on the server types path
    let seen = server.requests();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].url.starts_with("/server_types"), "{}", seen[0].url);
    assert_eq!(seen[0].auth.as_deref(), Some(format!("Bearer {TOKEN}").as_str()));
}

#[test]
fn a_plan_that_becomes_orderable_is_reported_against_the_state_of_the_last_run() {
    // Given a first run that left its result in the state file
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state.json");
    let state = state.to_str().unwrap();
    assert_eq!(stock(&serve(fixture()), &["--state", state]).exit_code, 0);

    // When the API shows cx43 available in hel1
    let mut doctored = fixture();
    turn_available(&mut doctored, "cx43", "hel1");
    let r = stock(&serve(doctored.clone()), &["--state", state]);

    // Then cx43 is available and changed from false to true in hel1
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report = json_of(&r);
    assert_eq!(report["available"], json!(["cx23", "cx43"]));
    assert_eq!(report["changed"], json!([{ "name": "cx43", "was": false, "now": true, "locations": ["hel1"] }]));
    assert_eq!(report["firstRun"], json!(false));

    // and a third run on the same answer reports no change
    let again = json_of(&stock(&serve(doctored), &["--state", state]));
    assert_eq!(again["changed"], json!([]));
}

#[test]
fn the_family_and_size_filters_keep_only_the_matching_plans() {
    // Given the same answer
    let server = serve(fixture());

    // When the cpx and ccx families are asked for with at least 4 vCPU and 8 GB
    let r = stock(&server, &["--family", "cpx,ccx", "--min-vcpu", "4", "--min-ram-gb", "8"]);

    // Then only cpx32 is listed (ccx13 has 2 vCPU) and no state keys appear without --state
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report = json_of(&r);
    assert_eq!(names(&report), ["cpx32"]);
    assert_eq!(report["available"], json!(["cpx32"]));
    assert!(report.get("changed").is_none() && report.get("firstRun").is_none());
}

#[test]
fn every_page_of_the_catalogue_is_read() {
    // Given an API that serves the server types on two pages
    let all = fixture();
    let types = all["server_types"].as_array().unwrap().clone();
    let (first, second) = types.split_at(3);
    let page = |items: &[Value], next: Value| json!({ "server_types": items, "meta": { "pagination": { "page": 1, "last_page": 2, "next_page": next, "per_page": 50, "total_entries": 7 } } });
    let server = MockServer::start(None);
    server.routes(&[
        ("GET /server_types?per_page=50", MockReply::json(200, page(first, json!(2)))),
        ("GET /server_types?per_page=50&page=2", MockReply::json(200, page(second, Value::Null))),
    ]);

    // When the stock is read for every family
    let r = stock(&server, &["--family", "cx,cax,cpx,ccx"]);

    // Then plans of both pages are listed, each request carrying the header
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(names(&json_of(&r)).len(), 7);
    let seen = server.requests();
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|q| q.url.starts_with("/server_types") && q.auth.as_deref() == Some(format!("Bearer {TOKEN}").as_str())));
}

#[test]
fn a_refused_token_is_an_error_that_does_not_contain_the_token() {
    // Given an API that answers 401
    let server = MockServer::start(None);
    server.routes(&[("GET /server_types?per_page=50", MockReply::json(401, json!({ "error": { "code": "unauthorized" } })))]);

    // When the stock is read
    let r = stock(&server, &[]);

    // Then it exits 1 saying the token was refused, and no output holds the token
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(json_of(&r), json!({ "ok": false, "error": "the token was refused" }));
    assert!(!r.stdout.contains(TOKEN) && !r.stderr.contains(TOKEN));
}

#[test]
fn another_error_status_is_named_by_its_number() {
    // Given an API that answers 500
    let server = MockServer::start(None);
    server.routes(&[("GET /server_types?per_page=50", MockReply::json(500, json!({})))]);

    // When the stock is read
    let r = stock(&server, &[]);

    // Then it exits 1 with the status
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(json_of(&r), json!({ "ok": false, "error": "API answered 500" }));
}

#[test]
fn without_a_token_nothing_is_asked() {
    // Given an API double and no token anywhere
    let server = serve(fixture());

    // When the stock is read
    let r = stock_with(&server, None, &[]);

    // Then it exits 1 with the not-set error and no request was made
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(json_of(&r), json!({ "ok": false, "error": "HCLOUD_READ_TOKEN is not set" }));
    assert!(server.requests().is_empty());
}

#[test]
fn a_provider_other_than_hetzner_is_refused_without_asking_anything() {
    // Given an API double
    let server = serve(fixture());

    // When another provider is named
    let r = stock(&server, &["--provider", "other"]);

    // Then it exits 1 naming the provider and no request was made
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let report = json_of(&r);
    assert_eq!(report["ok"], json!(false));
    assert!(report["error"].as_str().unwrap().contains("other"), "{report}");
    assert!(server.requests().is_empty());
}

#[test]
fn an_answer_without_server_types_is_the_signal_that_the_check_broke() {
    // Given an API whose answer has no server_types
    let server = serve(json!({ "datacenters": [] }));

    // When the stock is read
    let r = stock(&server, &[]);

    // Then it exits 1 with the API-changed error
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(json_of(&r), json!({ "ok": false, "error": "no server types in the answer: the API changed" }));
}

#[test]
fn a_fractional_vcpu_threshold_is_refused_before_any_request() {
    // Given a double and a threshold of 2.5 vCPU
    let server = serve(fixture());

    // When the stock is read
    let r = stock(&server, &["--min-vcpu", "2.5"]);

    // Then it exits 1 naming the whole-number rule, and the API was never asked
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(json_of(&r)["ok"], json!(false));
    assert!(json_of(&r)["error"].as_str().unwrap().contains("whole number"));
    assert!(server.requests().is_empty());
}
