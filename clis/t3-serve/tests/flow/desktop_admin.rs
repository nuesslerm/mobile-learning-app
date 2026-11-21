//! The administration reads a desktop makes: who may connect, the host's resource history, the process and trace diagnostics and the scan for
//! other local servers. The history is built from the host readings the double gives.
use super::support::*;

fn admin(h: &Harness) -> Rpc {
    let scopes = ["orchestration:read", "diagnostics:read", "access:read", "access:write", "environment:maintain", "preview:operate"].map(String::from).to_vec();
    connect_with(h, PairingRequest { scopes, label: "desktop".into(), ttl_seconds: 300 })
}

fn sessions(snapshot: &Value) -> Vec<(String, bool)> {
    snapshot["payload"]["clientSessions"].as_array().unwrap().iter().map(|s| (s["client"]["label"].as_str().unwrap().to_string(), s["current"].as_bool().unwrap())).collect()
}

#[test]
fn the_access_stream_lists_this_session_as_current_and_a_code_nobody_redeemed_as_a_pairing_link() {
    // Given a desktop connected with access scopes, and a second code minted but not redeemed
    let h = Harness::start();
    let mut rpc = admin(&h);
    let spare = h.pair(PairingRequest { label: "spare".into(), ..PairingRequest::default() });
    // When it subscribes to the access list
    let id = rpc.send("subscribeAuthAccess", json!({}));
    let first = rpc.chunk(&id).remove(0);
    // Then its own bearer is the one current and connected session, with the legacy scopes beside the permissions
    assert_eq!((first["type"].clone(), first["revision"].clone(), first["version"].clone()), (json!("snapshot"), json!(1), json!(1)));
    let session = &first["payload"]["clientSessions"][0];
    assert_eq!(sessions(&first), [("desktop".to_string(), true)]);
    assert_eq!((session["connected"].clone(), session["method"].clone(), session["subject"].clone()), (json!(true), json!("bearer-access-token"), json!("one-time-token")));
    assert_eq!(session["scopes"], json!(["orchestration:read", "access:read", "access:write"]));
    assert_eq!(session["permissions"], json!(["orchestration:read", "diagnostics:read", "access:read", "access:write", "environment:maintain", "preview:operate"]));
    // And the spare code is a link with its label, the scopes it grants and the id a revoke takes
    let links = first["payload"]["pairingLinks"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{first}");
    assert_eq!((links[0]["label"].clone(), links[0]["subject"].clone()), (json!("spare"), json!("one-time-token")));
    assert_eq!(links[0]["expiresAt"], json!(t3_contracts::time::iso(START_MS + 300_000)));
    let id_of_spare = t3_core::domain::auth::token_id(&t3_core::domain::auth::hash_secret(&spare.code));
    assert_eq!(links[0]["id"], json!(id_of_spare));
}

#[test]
fn a_second_bearer_is_listed_and_a_revoked_one_is_gone_from_the_next_snapshot() {
    // Given a desktop and another client that redeemed its own code
    let h = Harness::start();
    let mut rpc = admin(&h);
    let other = h.pair(PairingRequest { label: "phone".into(), ..PairingRequest::default() });
    bearer_of(h.addr, &other.code);
    let id = rpc.send("subscribeAuthAccess", json!({}));
    let first = rpc.chunk(&id).remove(0);
    let mut listed = sessions(&first);
    listed.sort();
    assert_eq!(listed, [("desktop".to_string(), true), ("phone".to_string(), false)]);
    // When the desktop revokes the phone's session by the id the list gave
    let session = first["payload"]["clientSessions"].as_array().unwrap().iter().find(|s| s["client"]["label"] == "phone").unwrap()["sessionId"].clone();
    let revoked = http(h.addr, "POST", "/api/auth/clients/revoke", &[("Authorization", &format!("Bearer {}", bearer_of(h.addr, &h.pair(PairingRequest { scopes: vec!["access:write".into()], ..PairingRequest::default() }).code))), ("Content-Type", "application/json")], &json!({"sessionId": session}).to_string());
    assert_eq!(revoked.body, json!({"revoked": true}));
    let again = rpc.send("subscribeAuthAccess", json!({}));
    let second = rpc.chunk(&again).remove(0);
    assert!(sessions(&second).iter().all(|(label, _)| label != "phone"), "{second}");
}

#[test]
fn a_connection_without_access_read_is_denied_the_access_stream() {
    // Given a connection that was granted orchestration only
    let h = Harness::start();
    let mut rpc = connect_with(&h, PairingRequest { scopes: vec!["orchestration:read".into()], ..PairingRequest::default() });
    // When it subscribes to the access list
    let exit = rpc.call("subscribeAuthAccess", json!({}));
    // Then it is refused with the missing permission
    assert!(failure(&exit)["requiredScope"].is_string(), "{exit}");
}

#[test]
fn process_diagnostics_name_this_server_and_list_no_child_process() {
    // Given a connected desktop
    let h = Harness::start();
    let mut rpc = admin(&h);
    // When it reads the process diagnostics
    let diagnostics = success(&rpc.call("server.getProcessDiagnostics", json!({}))).clone();
    // Then the server's pid is this process and the list of children is empty, as the recording
    assert_eq!((diagnostics["serverPid"].clone(), diagnostics["processCount"].clone(), diagnostics["processes"].clone(), diagnostics["error"].clone()), (json!(std::process::id()), json!(0), json!([]), json!({"_tag": "None"})));
    assert_eq!(diagnostics["totalRssBytes"], json!(0));
}

#[test]
fn the_process_history_buckets_the_host_readings_each_read_takes_and_clamps_the_spans_to_a_second() {
    // Given a desktop that has asked for the history three times, 2 s apart, while the host showed 25 % of 8 GB with 6 GB free
    let h = Harness::start();
    let mut rpc = admin(&h);
    for _ in 0..3 {
        success(&rpc.call("server.getProcessResourceHistory", json!({"windowMs": 0, "bucketMs": 0})));
        h.clock.advance_ms(2000);
    }
    // When it asks for a ten second window in one second buckets
    let history = success(&rpc.call("server.getProcessResourceHistory", json!({"windowMs": 10_000, "bucketMs": 1000}))).clone();
    // Then each reading is a bucket of 25 % cpu and 2 GB in use, and the ring holds all four readings
    let buckets = history["buckets"].as_array().unwrap();
    assert_eq!(buckets.len(), 4, "{history}");
    assert!(buckets.iter().all(|b| b["avgCpuPercent"] == json!(25.0) && b["maxRssBytes"] == json!(2_000_000_000u64) && b["maxProcessCount"] == json!(0)), "{history}");
    assert_eq!((history["retainedSampleCount"].clone(), history["windowMs"].clone(), history["bucketMs"].clone(), history["topProcesses"].clone()), (json!(4), json!(10_000), json!(1000), json!([])));
    // And a window of 0 is answered as one second, as the pinned server does
    let clamped = success(&rpc.call("server.getProcessResourceHistory", json!({"windowMs": 0, "bucketMs": 0}))).clone();
    assert_eq!((clamped["windowMs"].clone(), clamped["bucketMs"].clone()), (json!(1000), json!(1000)));
}

#[test]
fn a_host_that_cannot_be_read_gives_an_empty_history_and_says_so_in_the_telemetry_health() {
    // Given a host the probe cannot read
    let h = Harness::start();
    *h.host.0.lock().unwrap() = Err("no /proc here".into());
    let mut rpc = admin(&h);
    // When the desktop reads the process history and the telemetry history
    let process = success(&rpc.call("server.getProcessResourceHistory", json!({"windowMs": 1000, "bucketMs": 1000}))).clone();
    let telemetry = success(&rpc.call("server.getResourceTelemetryHistory", json!({"windowMs": 1000, "bucketMs": 1000}))).clone();
    // Then no bucket is listed, the process history carries the error, and the telemetry health names the native sampler as unavailable
    assert_eq!((process["buckets"].clone(), process["error"].clone()), (json!([]), json!({"_tag": "Some", "value": {"message": "no /proc here"}})));
    assert_eq!((telemetry["buckets"].clone(), telemetry["health"]["native"]["status"].clone(), telemetry["health"]["native"]["lastError"].clone()), (json!([]), json!("unavailable"), json!({"_tag": "Some", "value": "no /proc here"})));
}

#[test]
fn the_telemetry_history_has_the_recorded_keys_and_a_desktop_that_is_unavailable() {
    // Given a desktop connected to a readable host
    let h = Harness::start();
    let mut rpc = admin(&h);
    // When it reads the telemetry history
    let telemetry = success(&rpc.call("server.getResourceTelemetryHistory", json!({"windowMs": 1000, "bucketMs": 1000}))).clone();
    // Then the keys are the recorded ones, the io columns are zero and the desktop sampler is unavailable with its reason
    let mut keys: Vec<&String> = telemetry.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["bucketMs", "buckets", "health", "readAt", "retainedSampleCount", "sampleIntervalMs", "topProcesses", "windowMs"]);
    assert_eq!((telemetry["buckets"][0]["ioReadBytes"].clone(), telemetry["buckets"][0]["ioWriteBytes"].clone()), (json!(0), json!(0)));
    assert_eq!(telemetry["health"]["desktop"]["status"], "unavailable");
    assert_eq!(telemetry["health"]["native"]["status"], "healthy");
}

#[test]
fn retrying_the_telemetry_accepts_nothing_and_answers_a_snapshot_of_no_process() {
    // Given a connected desktop
    let h = Harness::start();
    let mut rpc = admin(&h);
    // When it asks the telemetry to retry
    let retried = success(&rpc.call("server.retryResourceTelemetry", json!({}))).clone();
    // Then nothing was accepted, every group is zero and no process is listed
    assert_eq!((retried["accepted"].clone(), retried["snapshot"]["processes"].clone()), (json!(false), json!([])));
    for group in ["allT3", "backend", "electron", "monitor"] {
        assert_eq!(retried["snapshot"]["groups"][group]["processCount"], json!(0), "{group}");
    }
    assert_eq!(retried["snapshot"]["power"]["stale"], json!(true));
}

#[test]
fn trace_diagnostics_say_there_is_no_trace_file_with_the_pinned_error_kind() {
    // Given a connected desktop
    let h = Harness::start();
    let mut rpc = admin(&h);
    // When it reads the trace diagnostics
    let trace = success(&rpc.call("server.getTraceDiagnostics", json!({}))).clone();
    // Then the schema's own not-found kind is the error and no span, failure or log is listed
    assert_eq!(trace["error"], json!({"_tag": "Some", "value": {"kind": "trace-file-not-found", "message": "t3 serve writes no trace file."}}));
    assert_eq!((trace["recordCount"].clone(), trace["failureCount"].clone(), trace["topSpansByCount"].clone(), trace["scannedFilePaths"].clone()), (json!(0), json!(0), json!([]), json!([])));
}

#[test]
fn the_scan_for_local_servers_finds_none_and_stays_open_until_the_desktop_closes_it() {
    // Given a connected desktop
    let h = Harness::start();
    let mut rpc = admin(&h);
    // When it subscribes to the discovered local servers with a configured URL
    let id = rpc.send("subscribeDiscoveredLocalServers", json!({"configuredUrls": ["http://127.0.0.1:3773"]}));
    let scan = rpc.chunk(&id).remove(0);
    // Then one scan is sent with no server, stamped with the clock, and no further frame follows
    assert_eq!((scan["servers"].clone(), scan["scannedAt"].clone()), (json!([]), json!(t3_contracts::time::iso(START_MS))));
    assert!(rpc.try_recv(std::time::Duration::from_millis(300)).is_none());
}
