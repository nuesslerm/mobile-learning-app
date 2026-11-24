//! `HttpStore` against an S3 double that verifies every SigV4 signature, and against a bearer-token server.
use super::s3_fake::{BearerFake, S3Fake, ACCESS_KEY, BEARER, BUCKET, REGION, SECRET_KEY};
use blob_cache::adapters::sigv4::UNSIGNED_PAYLOAD;
use blob_cache::adapters::{Auth, HttpStore, S3Location, Side};
use blob_cache::domain::{Generation, Key, Kind};
use blob_cache::ports::{BlobStore, PutOutcome, StoreError};
use cli_http::Via;

fn key(name: &str) -> Key {
    Key::new(Kind::Nx, Generation(0), name).unwrap()
}

fn s3_side(endpoint: &str, bucket: &str, secret: &str) -> Side {
    Side::s3(&S3Location { endpoint, bucket, prefix: "ci/cache", virtual_host: false }, Auth::s3(ACCESS_KEY, secret, REGION))
}

fn s3_store(endpoint: &str, secret: &str) -> HttpStore {
    HttpStore::new(s3_side(endpoint, BUCKET, secret), Some(s3_side(endpoint, BUCKET, secret))).via(Via::Direct)
}

// The double serves keys under /bkt/, so the prefix is part of what it stores: reads and writes agree.

#[test]
fn given_a_signing_s3_double_over_http_when_objects_are_put_and_read_then_every_request_verifies_with_a_real_payload_hash() {
    // Given an S3 double that recomputes each signature and a store signed with the shared secret
    let fake = S3Fake::start();
    let store = s3_store(&fake.endpoint, SECRET_KEY);
    let k = key("roundtrip");

    // When a body is put, read, checked and streamed through files in both directions
    assert_eq!(store.put(&k, b"hello s3", false), Ok(PutOutcome::Stored));
    let read = store.get(&k);
    let exists = (store.exists(&k), store.exists(&key("absent")));
    let dir = std::env::temp_dir().join(format!("http-store-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let big: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.join("up"), &big).unwrap();
    let streamed = store.put_file(&key("big"), &dir.join("up"), false);
    let fetched = store.get_file(&key("big"), &dir.join("down"));
    let down = std::fs::read(dir.join("down")).unwrap();
    let missing = store.get_file(&key("nothing"), &dir.join("never"));
    let _ = std::fs::remove_dir_all(&dir);

    // Then all succeed, a miss is a miss, and plain http signs the real body hash, never UNSIGNED-PAYLOAD
    assert_eq!(read, Ok(Some(b"hello s3".to_vec())));
    assert_eq!(exists, (Ok(true), Ok(false)));
    assert_eq!((streamed, fetched, down == big, missing), (Ok(PutOutcome::Stored), Ok(true), true, Ok(false)));
    assert!(fake.seen().iter().all(|s| s.payload_hash != UNSIGNED_PAYLOAD && s.payload_hash.len() == 64), "{:?}", fake.seen());
}

#[test]
fn given_if_absent_puts_when_conditional_is_off_or_on_then_it_is_head_then_put_or_one_conditional_put() {
    for conditional in [false, true] {
        // Given a store with the conditional flag as set
        let fake = S3Fake::start();
        let store = s3_store(&fake.endpoint, SECRET_KEY).conditional_put(conditional);
        let k = key("once");

        // When the same key is put if absent twice
        let first = store.put(&k, b"a", true);
        let first_requests: Vec<_> = fake.seen().iter().map(|s| (s.method.clone(), s.if_none_match)).collect();
        let second = store.put(&k, b"b", true);

        // Then the first stores and the second finds it, with the requests the flag implies, and the object is the first body
        assert_eq!((first, second), (Ok(PutOutcome::Stored), Ok(PutOutcome::Exists)));
        let expected = if conditional { vec![("PUT".to_string(), true)] } else { vec![("HEAD".to_string(), false), ("PUT".to_string(), false)] };
        assert_eq!(first_requests, expected);
        assert_eq!(store.requests_per_put(true), if conditional { 1 } else { 2 });
        assert_eq!(store.requests_per_put(false), 1);
        assert_eq!(store.get(&k), Ok(Some(b"a".to_vec())));
    }
}

#[test]
fn given_a_wrong_secret_or_clock_when_requests_are_signed_then_the_error_is_a_credential_that_leaks_no_key() {
    // Given a store signing with another secret
    let fake = S3Fake::start();
    let wrong = s3_store(&fake.endpoint, "another/secret");
    let k = key("denied");

    // When it reads and writes, Then both are credential errors that name the S3 code and carry neither key
    let (read, write) = (wrong.get(&k).unwrap_err(), wrong.put(&k, b"x", false).unwrap_err());
    for error in [&read, &write] {
        assert!(matches!(error, StoreError::Credential(_)), "{error}");
        let text = error.to_string();
        assert!(text.contains("403") && text.contains("SignatureDoesNotMatch"), "{text}");
        assert!(!text.contains(SECRET_KEY) && !text.contains("another/secret") && !text.contains(ACCESS_KEY), "{text}");
    }

    // When the store reports the clock is off, Then it is a credential error with a clock hint
    let skewed = S3Fake::start();
    skewed.skew_clocks();
    let error = s3_store(&skewed.endpoint, SECRET_KEY).get(&k).unwrap_err();
    assert!(matches!(&error, StoreError::Credential(text) if text.contains("RequestTimeTooSkewed") && text.contains("clock")), "{error}");
}

#[test]
fn given_failures_when_the_store_answers_503_404_or_a_missing_bucket_then_it_retries_misses_or_refuses() {
    let fake = S3Fake::start();
    let store = s3_store(&fake.endpoint, SECRET_KEY);
    let k = key("flaky");
    assert_eq!(store.put(&k, b"kept", false), Ok(PutOutcome::Stored));

    // When one 503 precedes a read and two 503s precede a write, Then both are asked again and succeed
    fake.fail_next(1, 503);
    let read = store.get(&k);
    fake.fail_next(2, 503);
    let written = store.put(&k, b"again", false);
    assert_eq!((read, written), (Ok(Some(b"kept".to_vec())), Ok(PutOutcome::Stored)));

    // When the store keeps failing with 503 or 429, Then it is unreachable after the retries
    fake.fail_next(10, 503);
    assert!(matches!(store.get(&k), Err(StoreError::Unreachable(_))));
    fake.fail_next(10, 429);
    assert!(matches!(store.exists(&k), Err(StoreError::Unreachable(_))));
    fake.fail_next(0, 0);

    // When the key is absent, Then it is a miss, and when the bucket does not exist it is a refusal, never a cold cache
    assert_eq!(store.get(&key("nobody")), Ok(None));
    let nowhere = HttpStore::new(s3_side(&fake.endpoint, "nowhere", SECRET_KEY), None).via(Via::Direct);
    assert!(matches!(nowhere.get(&k), Err(StoreError::Rejected(text)) if text.contains("NoSuchBucket")));

    // And a store with no write side refuses to put
    assert!(matches!(nowhere.put(&k, b"x", false), Err(StoreError::Rejected(_))));
}

#[test]
fn given_a_bearer_server_when_the_token_is_right_or_wrong_then_requests_carry_it_and_no_error_repeats_it() {
    // Given a server that wants the bearer token
    let fake = BearerFake::start();
    let store = |token: &str| HttpStore::new(Side::new(&fake.url, Auth::Bearer(token.to_string())), Some(Side::new(&fake.url, Auth::Bearer(token.to_string())))).via(Via::Direct);
    let k = key("bearer");

    // When the right token puts, reads and checks
    let right = store(BEARER);
    let results = (right.put(&k, b"dav", false), right.get(&k), right.exists(&k), right.get(&key("absent")));

    // Then they succeed and every request carried the header
    assert_eq!(results, (Ok(PutOutcome::Stored), Ok(Some(b"dav".to_vec())), Ok(true), Ok(None)));
    assert!(fake.authorizations.lock().unwrap().iter().all(|a| a.as_deref() == Some(&format!("Bearer {BEARER}"))));

    // When a wrong token reads and writes, Then both are credential errors without either token
    let wrong = store("WRONG-TOKEN");
    let (read, write) = (wrong.get(&k).unwrap_err(), wrong.put(&k, b"x", false).unwrap_err());
    assert!(matches!((&read, &write), (StoreError::Credential(_), StoreError::Credential(_))));

    // And a store that nothing listens on is unreachable without the token in the text
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let down = HttpStore::new(Side::new(&format!("http://127.0.0.1:{closed}/o/"), Auth::Bearer(BEARER.into())), None).via(Via::Direct).get(&k).unwrap_err();
    assert!(matches!(down, StoreError::Unreachable(_)));
    assert!(!format!("{read} {write} {down}").contains("TOKEN"), "{read} {write} {down}");
}
