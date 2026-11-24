//! The HTTP routes that hand access out and take it back, and the answers the pin gives a request it refuses: 415 for a body not declared
//! JSON, a bare 400 for one its schema refuses, the revoke routes, the hook routes that find no hook, and the project list.
use super::support::*;

const JSON: (&str, &str) = ("Content-Type", "application/json");

fn bearer_with(h: &Harness, scopes: &[&str]) -> String {
    let scopes = scopes.iter().map(|s| s.to_string()).collect();
    bearer_of(h.addr, &h.pair(PairingRequest { scopes, ..PairingRequest::default() }).code)
}

fn post(h: &Harness, path: &str, bearer: &str, body: &str) -> Reply {
    let auth = format!("Bearer {bearer}");
    http(h.addr, "POST", path, &[("Authorization", &auth), JSON], body)
}

#[test]
fn a_json_route_given_a_body_not_declared_json_answers_415_text_and_one_its_schema_refuses_answers_a_bare_400() {
    // Given a client with access:write and read
    let h = Harness::start();
    let admin = bearer_with(&h, &["orchestration:read", "access:write"]);
    let auth = format!("Bearer {admin}");
    // When each JSON route is posted to with no content-type
    for path in ["/api/auth/pairing-token", "/api/pull-requests/diff", "/api/auth/browser-session", "/api/auth/clients/revoke", "/api/auth/pairing-links/revoke"] {
        let plain = http(h.addr, "POST", path, &[("Authorization", &auth), ("x-t3-orchestration-protocol", "2")], "");
        // Then the answer is 415 with the pin's text, before any gate
        assert_eq!(plain.status, 415, "{path}");
        assert!(plain.head.contains("content-type: text/plain\r\n") && plain.head.contains("vary: accept-encoding"), "{}", plain.head);
    }
    // And a body the schema refuses is a 400 with only the CORS header
    for (path, body) in [("/api/auth/browser-session", "{}"), ("/api/auth/clients/revoke", r#"{"sessionId": 7}"#), ("/api/auth/pairing-links/revoke", "[]"), ("/api/auth/pairing-links/revoke", "{")] {
        let refused = post(&h, path, &admin, body);
        assert_eq!((refused.status, refused.body.clone()), (400, Value::Null), "{path} {body}");
        assert!(refused.head.contains("access-control-allow-origin") && !refused.head.contains("content-type") && !refused.head.contains("vary"), "{}", refused.head);
    }
}

#[test]
fn a_browser_session_credential_is_refused_as_an_invalid_credential_with_a_trace_id_this_server_issues_none() {
    // Given a paired client
    let h = Harness::start();
    let bearer = bearer_with(&h, &["orchestration:read"]);
    // When it posts a credential to browser-session
    let refused = post(&h, "/api/auth/browser-session", &bearer, r#"{"credential": "abc"}"#);
    // Then the answer is the 401 the pin gives an unknown credential
    assert_eq!(refused.status, 401);
    assert_eq!((refused.body["_tag"].clone(), refused.body["code"].clone(), refused.body["reason"].clone()), (json!("EnvironmentAuthInvalidError"), json!("auth_invalid"), json!("invalid_credential")));
    assert!(refused.body["traceId"].as_str().is_some_and(|t| !t.is_empty()));
}

#[test]
fn revoking_a_client_signs_that_device_out_and_an_unknown_one_is_reported_not_revoked() {
    // Given an admin and a device that holds a bearer, whose session id is the one its pairing answered with
    let h = Harness::start();
    let admin = bearer_with(&h, &["orchestration:read", "access:write"]);
    let minted = post(&h, "/api/auth/pairing-token", &admin, r#"{"scopes": ["orchestration:read"]}"#);
    let device = bearer_of(h.addr, minted.body["credential"].as_str().unwrap());
    let session = http(h.addr, "GET", "/api/auth/session", &[("Authorization", &format!("Bearer {device}"))], "");
    assert_eq!(session.status, 200);
    let device_id = t3_core::domain::auth::token_id(&t3_core::domain::auth::hash_secret(&device));
    // When the admin revokes the device and an id nobody holds
    let revoked = post(&h, "/api/auth/clients/revoke", &admin, &json!({"sessionId": device_id}).to_string());
    let unknown = post(&h, "/api/auth/clients/revoke", &admin, r#"{"sessionId": "nobody"}"#);
    // Then the first is revoked and its bearer no longer works, and the second is not revoked
    assert_eq!((revoked.status, revoked.body.clone()), (200, json!({"revoked": true})));
    assert_eq!((unknown.status, unknown.body.clone()), (200, json!({"revoked": false})));
    assert_eq!(ticket(h.addr, &device).status, 401);
    assert_eq!(post(&h, "/api/auth/clients/revoke", &admin, &json!({"sessionId": device_id}).to_string()).body, json!({"revoked": false}));
}

#[test]
fn the_revoke_routes_are_refused_without_access_write() {
    // Given a client without access:write
    let h = Harness::start();
    let reader = bearer_with(&h, &["orchestration:read"]);
    // When it asks to revoke a client, a pairing link and the other clients
    let answers = [
        post(&h, "/api/auth/clients/revoke", &reader, r#"{"sessionId": "a"}"#),
        post(&h, "/api/auth/pairing-links/revoke", &reader, r#"{"id": "a"}"#),
        http(h.addr, "POST", "/api/auth/clients/revoke-others", &[("Authorization", &format!("Bearer {reader}"))], ""),
    ];
    // Then each is a 403 naming the scope
    for answer in answers {
        assert_eq!((answer.status, answer.body["requiredScope"].clone()), (403, json!("access:write")), "{:?}", answer.body);
    }
}

#[test]
fn a_pairing_link_not_yet_used_is_revoked_once_and_its_code_pairs_no_device() {
    // Given an admin that minted a pairing credential
    let h = Harness::start();
    let admin = bearer_with(&h, &["orchestration:read", "access:write"]);
    let minted = post(&h, "/api/auth/pairing-token", &admin, r#"{"scopes": ["orchestration:read"]}"#);
    let (id, code) = (minted.body["id"].as_str().unwrap().to_string(), minted.body["credential"].as_str().unwrap().to_string());
    // When the admin revokes the link, twice
    let first = post(&h, "/api/auth/pairing-links/revoke", &admin, &json!({"id": id}).to_string());
    let again = post(&h, "/api/auth/pairing-links/revoke", &admin, &json!({"id": id}).to_string());
    // Then the first revokes it, the second finds none, and the code is refused
    assert_eq!((first.body, again.body), (json!({"revoked": true}), json!({"revoked": false})));
    assert_eq!(exchange(h.addr, &code).status, 401);
}

#[test]
fn a_string_shorter_than_a_link_id_revokes_no_link_though_it_prefixes_one() {
    // Given an admin that minted a pairing credential
    let h = Harness::start();
    let admin = bearer_with(&h, &["orchestration:read", "access:write"]);
    let minted = post(&h, "/api/auth/pairing-token", &admin, r#"{"scopes": ["orchestration:read"]}"#);
    let (id, code) = (minted.body["id"].as_str().unwrap().to_string(), minted.body["credential"].as_str().unwrap().to_string());
    // When the admin revokes the first character of its id
    let short = post(&h, "/api/auth/pairing-links/revoke", &admin, &json!({"id": &id[..1]}).to_string());
    // Then nothing is revoked and the code still pairs a device
    assert_eq!(short.body, json!({"revoked": false}));
    assert_eq!(exchange(h.addr, &code).status, 200);
}

#[test]
fn revoking_the_other_clients_keeps_the_caller_and_counts_the_others() {
    // Given an admin and two other devices
    let h = Harness::start();
    let admin = bearer_with(&h, &["orchestration:read", "access:write"]);
    let others = [bearer_with(&h, &["orchestration:read"]), bearer_with(&h, &["terminal:read"])];
    // When the admin revokes the others, with no body and no content-type
    let revoked = http(h.addr, "POST", "/api/auth/clients/revoke-others", &[("Authorization", &format!("Bearer {admin}"))], "");
    // Then two were revoked, theirs no longer work and the admin's still does
    assert_eq!((revoked.status, revoked.body.clone()), (200, json!({"revokedCount": 2})));
    assert!(others.iter().all(|b| ticket(h.addr, b).status == 401));
    assert_eq!(ticket(h.addr, &admin).status, 200);
    assert_eq!(http(h.addr, "POST", "/api/auth/clients/revoke-others", &[("Authorization", &format!("Bearer {admin}"))], "").body, json!({"revokedCount": 0}));
}

#[test]
fn a_hook_route_answers_that_the_hook_does_not_exist_for_each_method() {
    // Given a server that builds no webhook
    let h = Harness::start();
    for method in ["GET", "POST", "PUT", "PATCH"] {
        // When a hook is called
        let reply = http(h.addr, method, "/api/hooks/x-1/x-1", &[], "");
        // Then it is a 404 hook_not_found with the outcome header
        assert_eq!((reply.status, reply.body.clone()), (404, json!({"error": "hook_not_found"})), "{method}");
        assert!(reply.head.contains("x-t3-hook-outcome: not_found"), "{}", reply.head);
    }
    assert_eq!(http(h.addr, "GET", "/api/hooks/x-1", &[], "").body["code"], "not_found");
}

#[test]
fn the_project_list_holds_the_live_projects_behind_the_read_scope_and_needs_no_protocol_header() {
    // Given a server with one project made and another deleted
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    success(&rpc.call("projects.mutate", project_create("c-p2", "p2")));
    success(&rpc.call("projects.mutate", json!({"type": "project.delete", "commandId": "c-d2", "projectId": "p2"})));
    let reader = bearer_with(&h, &["orchestration:read"]);
    let blind = bearer_with(&h, &["terminal:read"]);
    // When the list is read with and without the scope
    let list = http(h.addr, "GET", "/api/projects", &[("Authorization", &format!("Bearer {reader}"))], "");
    let refused = http(h.addr, "GET", "/api/projects", &[("Authorization", &format!("Bearer {blind}"))], "");
    // Then it holds p1 as the mutation answers a row, with an updatedAt, and the scope is required
    assert_eq!(list.status, 200, "{:?}", list.body);
    let ids: Vec<&Value> = list.body["projects"].as_array().unwrap().iter().map(|p| &p["id"]).collect();
    assert_eq!(ids, [&json!("p1")]);
    assert!(list.body["projects"][0]["deletedAt"].is_null() && list.body["updatedAt"].is_string());
    assert_eq!((refused.status, refused.body["_tag"].clone()), (403, json!("EnvironmentScopeRequiredError")));
}
