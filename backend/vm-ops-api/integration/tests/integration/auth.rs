//! Auth (plan section 2, "Auth"): the rotating token with its signature, the legacy raw token, and what neither lets in.
//! Requests go out byte for byte (`Service::raw`), so a case controls every header's text and the request target.
use crate::world;
use base64::Engine as _;
use std::time::Duration;
use vm_ops_api_integration::{auth, canonical, case, now_ms, signature, token, Reply, Service, SEED};

fn sent(svc: &Service, target: &str, headers: &[(String, String)], body: Option<&[u8]>) -> Reply {
    svc.raw("GET", target, headers, body).unwrap_or_else(|e| panic!("GET {target}: {e}"))
}

/// The status of `GET /health` with `headers`: 200 when the service lets the request in.
fn health(svc: &Service, headers: &[(String, String)]) -> u16 {
    sent(svc, "/health", headers, None).status
}

/// A bearer token plus the signature headers a case chose to send.
fn headers(token: &str, signature: Option<&str>, timestamp: Option<&str>) -> Vec<(String, String)> {
    let mut h = vec![("Authorization".to_string(), format!("Bearer {token}"))];
    h.extend(signature.map(|s| ("X-Signature".to_string(), s.to_string())));
    h.extend(timestamp.map(|t| ("X-Signature-Timestamp".to_string(), t.to_string())));
    h
}

fn signed_health(timestamp: &str) -> String {
    signature(&canonical("GET", "/health", timestamp, b""))
}

/// The status of a signed `GET /health` whose timestamp is `offset_ms` off this clock. The service reads its clock
/// somewhere in the round trip and the 59 s and 61 s bounds leave 1 s, so only a reply within 900 ms decides.
fn health_off_by(svc: &Service, offset_ms: i64) -> u16 {
    for _ in 0..5 {
        let today = token(0);
        let sent_at = now_ms();
        let ts = (sent_at + offset_ms).to_string();
        let status = health(svc, &headers(&today, Some(&signed_health(&ts)), Some(&ts)));
        if now_ms() - sent_at < 900 {
            return status;
        }
    }
    panic!("five signed reads of /health each took 900 ms or more, so a 1 s skew margin cannot be checked");
}

case!(the_service_refuses_to_start_without_an_auth_token, {
    for token in [None, Some("")] {
        // Given a world whose VM_OPS_API_AUTH_TOKEN is unset, or set but empty
        let mut w = world();
        match token {
            None => w.unset("VM_OPS_API_AUTH_TOKEN"),
            Some(v) => w.set("VM_OPS_API_AUTH_TOKEN", v),
        };
        // When the service starts
        let exit = w.run_to_exit(Duration::from_secs(15));
        // Then it exits 1 with the refusal as its only stderr line
        assert_eq!(
            (exit.code, exit.stderr.as_str()),
            (Some(1), "VM_OPS_API_AUTH_TOKEN is not set: refusing to start with no auth configured\n"),
            "token {token:?}, stdout: {}",
            exit.stdout
        );
    }
});

case!(todays_and_yesterdays_signed_tokens_pass_and_an_older_unsigned_or_missigned_one_is_401, {
    // Given the service
    let svc = world().start();
    let ts = now_ms().to_string();
    let sig = signed_health(&ts);
    // When /health is read with the rotating token of today, yesterday and the day before, each with a signature
    // Then today's and yesterday's pass, and the day before yesterday's is 401
    for (days_ago, want) in [(0, 200), (1, 200), (2, 401)] {
        assert_eq!(health(&svc, &headers(&token(days_ago), Some(&sig), Some(&ts))), want, "the token of {days_ago} days ago");
    }
    // When today's token comes with no signature headers, or an empty signature
    // Then each is 401: a rotating token never falls back to the raw-token check
    assert_eq!(health(&svc, &headers(&token(0), None, None)), 401, "no signature headers");
    assert_eq!(health(&svc, &headers(&token(0), Some(""), Some(&ts))), 401, "an empty x-signature");
    // When the signature is over another body or another query than the request's
    // Then it is 401, while the signature over the request's own body and query passes
    let own_body = auth("GET", "/health", &ts, b"x");
    assert_eq!(sent(&svc, "/health", &own_body, Some(b"x")).status, 200, "signed over its own body");
    assert_eq!(sent(&svc, "/health", &own_body, Some(b"y")).status, 401, "signed over another body");
    let own_query = auth("GET", "/health?a=1", &ts, b"");
    assert_eq!(sent(&svc, "/health?a=1", &own_query, None).status, 200, "signed over its own query");
    assert_eq!(sent(&svc, "/health?a=2", &own_query, None).status, 401, "signed over another query");
});

case!(the_timestamp_must_be_within_60_s_and_is_signed_as_its_trimmed_header_text, {
    // Given the service
    let svc = world().start();
    // When the signature timestamp is 59 s or 61 s off, either way
    // Then 59 s passes and 61 s is 401
    for (offset, want) in [(-59_000, 200), (59_000, 200), (-61_000, 401), (61_000, 401)] {
        assert_eq!(health_off_by(&svc, offset), want, "a timestamp {offset} ms off");
    }
    // When the timestamp header is sent padded with spaces, which the HTTP layer trims
    // Then a signature over the trimmed text passes and one over the padded text is 401
    let ts = now_ms().to_string();
    let padded = format!("   {ts}   ");
    assert_eq!(health(&svc, &headers(&token(0), Some(&signed_health(&ts)), Some(&padded))), 200, "signed over the trimmed text");
    assert_eq!(health(&svc, &headers(&token(0), Some(&signed_health(&padded)), Some(&padded))), 401, "signed over the padded text");
});

case!(js_number_forms_of_the_timestamp_pass_when_signed_over_the_text_sent, {
    // Given the service and the time now
    let svc = world().start();
    let ts = now_ms();
    // When the timestamp is sent in a form JS Number() reads, each signed over exactly that text
    // Then each passes
    for form in [format!("{ts}e0"), format!("{ts}.0"), format!("+{ts}"), format!("0x{ts:x}")] {
        assert_eq!(health(&svc, &auth("GET", "/health", &form, b"")), 200, "the timestamp header {form:?}");
    }
    // When the text is no number, Then it is 401
    assert_eq!(health(&svc, &auth("GET", "/health", &format!("{ts}x"), b"")), 401, "a timestamp that is no number");
});

case!(a_signature_in_the_url_safe_base64_alphabet_passes, {
    // Given the service, and a signature whose standard base64 holds a + or a /, so its URL-safe text differs
    let svc = world().start();
    let now = now_ms();
    let (ts, standard) = (0..1_000)
        .map(|back| (now - back).to_string())
        .map(|ts| {
            let sig = signed_health(&ts);
            (ts, sig)
        })
        .find(|(_, sig)| sig.contains(['+', '/']))
        .expect("a signature with + or / among 1000 timestamps");
    let url_safe = standard.replace('+', "-").replace('/', "_").trim_end_matches('=').to_string();
    // When it is sent in the URL-safe alphabet without padding
    // Then it passes: the service decodes base64 leniently
    assert_eq!(health(&svc, &headers(&token(0), Some(&url_safe), Some(&ts))), 200, "{url_safe}");
});

case!(the_signature_covers_the_normalised_pathname_and_search, {
    // Given the service
    let svc = world().start();
    let ts = || now_ms().to_string();
    // When /x/../health is requested, Then the service resolves it to /health: signed as /health it passes, signed
    // over the raw target it is 401
    assert_eq!(sent(&svc, "/x/../health", &auth("GET", "/health", &ts(), b""), None).status, 200, "signed as /health");
    assert_eq!(sent(&svc, "/x/../health", &auth("GET", "/x/../health", &ts(), b""), None).status, 401, "signed as sent");
    // When /health? is requested, Then its bare ? is signed as nothing
    assert_eq!(sent(&svc, "/health?", &auth("GET", "/health", &ts(), b""), None).status, 200, "signed without the ?");
    assert_eq!(sent(&svc, "/health?", &auth("GET", "/health?", &ts(), b""), None).status, 401, "signed with the ?");
});

case!(the_legacy_raw_token_passes_unless_signed_auth_is_required, {
    let legacy = [("Authorization".to_string(), format!("Bearer {SEED}"))];
    // Given the service with REQUIRE_SIGNED_AUTH unset, When /health is read with the raw token, Then it passes
    let svc = world().start();
    assert_eq!(health(&svc, &legacy), 200, "the raw token");
    drop(svc);
    // Given REQUIRE_SIGNED_AUTH=true
    let mut w = world();
    w.set("REQUIRE_SIGNED_AUTH", "true");
    let svc = w.start();
    // When /health is read with the raw token, Then it is 401; a signed request still passes
    assert_eq!(health(&svc, &legacy), 401, "the raw token with REQUIRE_SIGNED_AUTH=true");
    assert_eq!(health(&svc, &auth("GET", "/health", &now_ms().to_string(), b"")), 200, "signed, with REQUIRE_SIGNED_AUTH=true");
});

case!(a_wrong_case_bearer_another_scheme_or_no_authorization_is_401, {
    // Given the service, and the signature headers of a valid request
    let svc = world().start();
    let signed = auth("GET", "/health", &now_ms().to_string(), b"");
    let signature_only = signed[1..].to_vec();
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("ops:{SEED}"));
    let authorization = |value: String| vec![("Authorization".to_string(), value)];
    let mut wrong_case_signed = authorization(format!("bearer {}", token(0)));
    wrong_case_signed.extend(signature_only.iter().cloned());
    let attempts = [
        ("bearer and the raw token", authorization(format!("bearer {SEED}"))),
        ("bearer and today's token, signed", wrong_case_signed),
        ("Basic", authorization(format!("Basic {basic}"))),
        ("no Authorization", Vec::new()),
        ("no Authorization, signature headers only", signature_only),
    ];
    // When /health is read with each, Then each is 401 unauthorized
    for (what, headers) in attempts {
        let r = sent(&svc, "/health", &headers, None);
        assert_eq!((r.status, r.body.as_str()), (401, r#"{"error":"unauthorized"}"#), "{what}");
    }
});
