//! The shapes `wf mimic proxy` exports: what a path, a query, a body and an envelope become once the values are gone.
use crate::support::*;
use wf_mimic::proxy_shape;

#[test]
fn a_path_keeps_the_route_words_and_masks_everything_else() {
    // Given the paths of the routes the observed server serves and of two that carry a value
    let route = proxy_shape::route;
    // Then the route's words stay and the values become <seg>
    assert_eq!(route("/api/orchestration/threads/01JABCDEF/history"), "/api/orchestration/threads/<seg>/history");
    assert_eq!(route("/.well-known/t3/environment"), "/.well-known/t3/environment");
    assert_eq!(route("/api/hooks/the-hook/the-token"), "/api/hooks/<seg>/<seg>");
    assert_eq!(route("/api/projects"), "/api/projects");
    assert_eq!(route("/"), "/");
    // And a query of its own does not change the path
    assert_eq!(route("/health?wsTicket=the-ticket"), "/health");
}

#[test]
fn a_query_keeps_the_names_it_knows_and_masks_the_rest() {
    // Given a query with a known name and an unknown one
    // Then the known name stays and the unknown one is masked
    assert_eq!(proxy_shape::query_names("/ws?wsTicket=the-ticket&orchestrationProtocol=2"), vec!["wsTicket", "orchestrationProtocol"]);
    assert_eq!(proxy_shape::query_names("/ws?the-unknown=2"), vec!["<param>"]);
    assert_eq!(proxy_shape::query_names("/ws?x-t3-orchestration-protocol=2"), vec!["x-t3-orchestration-protocol"]);
    assert_eq!(proxy_shape::query_names("/ws"), Vec::<String>::new());
    // And two names the vocabulary does not know keep a place of their own
    assert_eq!(proxy_shape::query_names("/ws?the-first=1&the-second=2"), vec!["<param>", "<param2>"]);
}

#[test]
fn the_headers_are_names_only() {
    // Given two headers and a repeated one
    let headers = vec![
        ("Authorization".to_string(), "Bearer the-token".to_string()),
        ("Content-Type".to_string(), "application/json".to_string()),
        ("authorization".to_string(), "Bearer the-token".to_string()),
    ];
    // Then the names are lower-cased, sorted and without a repeat
    assert_eq!(proxy_shape::header_names(&headers), vec!["authorization", "content-type"]);
}

#[test]
fn a_header_the_vocabulary_does_not_know_is_masked() {
    // Given a header whose name is a value of its own
    let headers = vec![
        ("X-The-Secret".to_string(), "v".to_string()),
        ("X-Another-Secret".to_string(), "v".to_string()),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    // Then the names the vocabulary knows stay and the others are masked, each of them its own
    assert_eq!(proxy_shape::header_names(&headers), vec!["content-type", "<header>", "<header2>"]);
}

#[test]
fn a_body_becomes_a_shape_of_what_it_is_made_of() {
    // Given an empty body, a JSON body, a form body and a text body
    // Then each has its own shape, and no value is in it
    assert_eq!(proxy_shape::body(b"", "application/json"), json!({"kind": "empty"}));
    let json_body = proxy_shape::body(br#"{"limit":3,"message":"the user's own text"}"#, "application/json");
    assert_eq!(json_body["kind"], json!("object"));
    assert_eq!(json_body["fields"]["limit"], json!({"type": "number"}));
    assert_eq!(json_body["fields"]["message"], json!({"type": "string"}));
    let form = proxy_shape::body(b"grant_type=the-code&scope=a+b", "application/x-www-form-urlencoded");
    assert_eq!(form["kind"], json!("object"));
    assert_eq!(form["fields"]["grant_type"], json!({"type": "string"}));
    assert_eq!(proxy_shape::body(b"the plain text", "text/plain"), json!({"kind": "text", "length": 14}));
    // And a body that claims to be JSON but is not is text
    assert_eq!(proxy_shape::body(b"{oops", "application/json"), json!({"kind": "text", "length": 5}));
}

#[test]
fn a_key_the_vocabulary_does_not_know_is_masked_and_no_entry_is_merged_away() {
    // Given a body whose keys are values of the traffic
    let body = proxy_shape::body(br#"{"threadId":"t1","the-first-secret":"v","the-second-secret":"v"}"#, "application/json");
    // Then the key the fixtures know stays and the others are masked, each of them its own
    assert_eq!(body["fields"]["threadId"], json!({"type": "string"}));
    assert_eq!(body["fields"]["<key>"], json!({"type": "string"}));
    assert_eq!(body["fields"]["<key2>"], json!({"type": "string"}));
    // And the object holds as many fields as it had
    assert_eq!(body["fields"].as_object().unwrap().len(), 3);
}

#[test]
fn a_form_keeps_the_keys_of_oauth_and_masks_the_rest() {
    // Given the form of a token exchange and a form with a key of its own
    let known = proxy_shape::body(b"grant_type=the-code&code=the-code&refresh_token=the-token", "application/x-www-form-urlencoded");
    // Then the keys of OAuth stay
    assert_eq!(known["fields"]["grant_type"], json!({"type": "string"}));
    assert_eq!(known["fields"]["code"], json!({"type": "string"}));
    assert_eq!(known["fields"]["refresh_token"], json!({"type": "string"}));
    // And a key the vocabulary does not know is masked
    let hostile = proxy_shape::body(b"the-form-secret=v", "application/x-www-form-urlencoded");
    assert_eq!(hostile["fields"]["<key>"], json!({"type": "string"}));
}

#[test]
fn an_array_body_keeps_its_length_and_the_shape_of_its_first_item() {
    // Given a body with three items of one shape
    let body = proxy_shape::body(br#"[{"id":"a"},{"id":"b"},{"id":"c"}]"#, "application/json");
    // Then the length and the item's shape are there
    assert_eq!(body["kind"], json!("array"));
    assert_eq!(body["length"], json!(3));
    assert_eq!(body["items"]["fields"]["id"], json!({"type": "string"}));
}

#[test]
fn an_envelope_names_its_method_its_request_and_the_shape_of_its_payload() {
    // Given a request of the orchestration protocol
    let frames = proxy_shape::envelopes(br#"{"_tag":"Request","id":"r-1","tag":"orchestration.dispatchCommand","payload":{"commandId":"c-1"}}"#, false);
    // Then the envelope, the method, the request and the payload's shape are there
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].name, "Request");
    assert_eq!(frames[0].method.as_deref(), Some("orchestration.dispatchCommand"));
    assert_eq!(frames[0].request.as_deref(), Some("r-1"));
    assert_eq!(frames[0].payload["fields"]["commandId"], json!({"type": "string"}));
}

#[test]
fn a_tag_and_a_method_the_vocabulary_does_not_know_are_masked() {
    // Given a frame whose tag and method are values of the traffic
    let frames = proxy_shape::envelopes(br#"{"_tag":"the-tag-secret","id":"r-1","tag":"the-method-secret","payload":{}}"#, false);
    // Then both are masked
    assert_eq!(frames[0].name, "<tag>");
    assert_eq!(frames[0].method.as_deref(), Some("<method>"));
    // And a method of the pinned wire stays
    let known = proxy_shape::envelopes(br#"{"_tag":"Request","id":"r-1","tag":"orchestration.subscribeThread","payload":{}}"#, false);
    assert_eq!(known[0].method.as_deref(), Some("orchestration.subscribeThread"));
}

#[test]
fn a_message_with_several_frames_yields_one_envelope_each() {
    // Given a message that holds two frames
    let frames = proxy_shape::envelopes(br#"[{"_tag":"Ack","requestId":"r-1"},{"_tag":"Ping"}]"#, false);
    // Then both are there
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].name, "Ack");
    assert_eq!(frames[0].request.as_deref(), Some("r-1"));
    assert_eq!(frames[1].name, "Ping");
    assert_eq!(frames[1].payload, json!({"kind": "object", "fields": {}}));
}

#[test]
fn a_frame_that_is_not_json_is_text_and_a_binary_frame_is_binary() {
    // Given a text frame that is not JSON and a binary frame
    let text = proxy_shape::envelopes(b"not json", false);
    // Then each has its own kind and its length
    assert_eq!(text[0].name, "text");
    assert_eq!(text[0].payload, json!({"kind": "text", "length": 8}));
    let binary = proxy_shape::envelopes(&[0u8; 12], true);
    assert_eq!(binary[0].name, "binary");
    assert_eq!(binary[0].payload, json!({"kind": "binary", "length": 12}));
}

#[test]
fn a_verb_the_observed_server_does_not_answer_is_masked() {
    // Given a verb of HTTP and a verb of its own
    // Then the one of the protocol stays and the other is masked
    assert_eq!(proxy_shape::verb("POST"), "POST");
    assert_eq!(proxy_shape::verb("PROPFIND"), "<verb>");
}
