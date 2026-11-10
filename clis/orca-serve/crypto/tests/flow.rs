//! Behaviour against the recorded set `vectors/orca-crypto` (a `wf mimic` cassette of the pinned reference code, with fixed keys and nonces).
use orca_crypto::auth::{authenticate, AuthFailure};
use orca_crypto::boxes::KeyPair;
use orca_crypto::legacy::{LegacyChannel, READY_TEXT};
use orca_crypto::pairing::{OfferRefusal, PairingOffer};
use orca_crypto::ports::{load_or_create_keypair, mint_device, Device, DeviceRegistry, KeyPairStore};
use orca_crypto::testing::{FixedEntropy, MemoryDevices, MemoryKeyPairStore, PatternEntropy};
use orca_crypto::v2::{schedule_from_messages, DesktopSession, HandshakeRefusal, Transport};
use orca_crypto::v2_frame::FrameRefusal;
use serde_json::{json, Value};

fn set_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vectors/orca-crypto")
}

fn recorded() -> Vec<Value> {
    let text = std::fs::read_to_string(set_dir().join("interactions.jsonl")).expect("the recorded set");
    text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn payload(id: &str) -> Value {
    recorded().iter().find(|i| i["id"] == id).unwrap_or_else(|| panic!("no recorded interaction {id}"))["steps"][0]["payload"].clone()
}

/// What the recorded interactions say, in the shape the tests below read.
fn vectors() -> Value {
    let (client, server) = (payload("keypair-client"), payload("keypair-server"));
    json!({
        "serverSecretHex": server["secretHex"], "serverPublicHex": server["publicHex"], "clientPublicHex": client["publicHex"],
        "handshakes": [payload("handshake-direct"), payload("handshake-relay")],
        "pinnedV2Vector": payload("pinned-v2-vector"), "legacy": payload("legacy"), "offer": payload("offer"),
    })
}

fn unhex(v: &Value) -> Vec<u8> {
    let s = v.as_str().unwrap();
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn arr<const N: usize>(v: &Value) -> [u8; N] {
    unhex(v).try_into().unwrap()
}

fn server() -> KeyPair {
    let v = vectors();
    let pair = KeyPair::from_secret(arr(&v["serverSecretHex"]));
    assert_eq!(pair.public.to_vec(), unhex(&v["serverPublicHex"]), "public key derives from the secret as in the reference");
    pair
}

fn transport(h: &Value) -> Transport {
    match h["relayHostId"].as_str() {
        Some(id) => Transport::Relay { host_id: id.to_string() },
        None => Transport::Direct,
    }
}

fn accept(h: &Value) -> DesktopSession {
    DesktopSession::accept(&h["hello"], &server(), &transport(h), &FixedEntropy(0x44)).expect("the recorded hello is accepted")
}

#[test]
fn given_recorded_hellos_when_accepted_then_ready_and_transcript_match_the_reference_byte_for_byte() {
    for h in vectors()["handshakes"].as_array().unwrap() {
        let session = accept(h);
        assert_eq!(session.ready, h["ready"], "{}", h["transport"]);
        assert_eq!(session.transcript_hash_b64(), h["transcriptHashB64"], "{}", h["transport"]);
    }
}

#[test]
fn given_the_pinned_normative_vector_when_the_schedule_is_derived_then_keys_and_session_id_match() {
    let p = &vectors()["pinnedV2Vector"];
    let s = schedule_from_messages(&p["hello"], &p["ready"], &arr(&p["sharedSecretHex"])).unwrap();
    assert_eq!(s.transcript_hash.to_vec(), unhex(&p["transcriptHashHex"]));
    assert_eq!(s.mobile_to_desktop.to_vec(), unhex(&p["mobileToDesktopKeyHex"]));
    assert_eq!(s.desktop_to_mobile.to_vec(), unhex(&p["desktopToMobileKeyHex"]));
    assert_eq!(s.session_id.to_vec(), unhex(&p["sessionIdHex"]));
}

#[test]
fn given_the_phones_sealed_frames_when_opened_in_order_then_plaintexts_match_and_replies_are_identical_to_the_reference() {
    for h in vectors()["handshakes"].as_array().unwrap() {
        let mut s = accept(h);
        assert_eq!(s.open_text(h["authFrameB64"].as_str().unwrap()).unwrap(), h["authText"].as_str().unwrap());
        assert_eq!(s.seal_text(h["authenticatedText"].as_str().unwrap()).unwrap(), h["replyFrameB64"].as_str().unwrap());
        assert_eq!(s.open_binary(&unhex(&h["clientBinaryFrameHex"])).unwrap(), unhex(&h["clientBinaryHex"]));
        assert_eq!(s.seal_binary(&unhex(&h["serverBinaryHex"])).unwrap(), unhex(&h["serverBinaryFrameHex"]));
    }
}

#[test]
fn given_a_replayed_frame_when_opened_again_then_it_is_refused_and_the_next_frame_still_opens() {
    let v = vectors();
    let h = &v["handshakes"][0];
    let mut s = accept(h);
    let frame = h["authFrameB64"].as_str().unwrap();
    s.open_text(frame).unwrap();
    assert_eq!(s.open_text(frame), Err(FrameRefusal::NotExpected));
    assert_eq!(s.open_binary(&unhex(&h["clientBinaryFrameHex"])).unwrap(), unhex(&h["clientBinaryHex"]), "the next frame in order still opens");
}

#[test]
fn given_frames_for_the_wrong_direction_or_kind_or_socket_when_opened_then_each_is_refused() {
    let v = vectors();
    let (direct, relay) = (&v["handshakes"][0], &v["handshakes"][1]);
    let mut s = accept(direct);
    assert!(s.open_text(direct["replyFrameB64"].as_str().unwrap()).is_err(), "a desktop-to-mobile frame is not a phone frame");
    assert!(s.open_binary(&unhex(&direct["clientBinaryFrameHex"])).is_err(), "counter 1 binary arriving first is out of order");
    assert!(s.open_text(relay["authFrameB64"].as_str().unwrap()).is_err(), "a frame of another session fails the keys");
}

#[test]
fn given_a_truncated_or_flipped_frame_when_opened_then_it_is_refused_and_the_counter_does_not_move() {
    let v = vectors();
    let h = &v["handshakes"][0];
    let frame = orca_crypto_b64(h["authFrameB64"].as_str().unwrap());
    let mut s = accept(h);
    for len in 0..frame.len() {
        assert!(s.open_binary(&frame[..len]).is_err(), "prefix {len}");
        assert!(s.open_text(&base64_encode(&frame[..len])).is_err(), "text prefix {len}");
    }
    for i in 0..frame.len() {
        let mut bad = frame.clone();
        bad[i] ^= 0x01;
        assert!(s.open_text(&base64_encode(&bad)).is_err(), "flip at byte {i}");
    }
    assert_eq!(s.open_text(h["authFrameB64"].as_str().unwrap()).unwrap(), h["authText"].as_str().unwrap(), "garbage did not desynchronise the socket");
}

#[test]
fn given_non_canonical_base64_when_a_text_frame_is_opened_then_it_is_refused() {
    let h = &vectors()["handshakes"][0];
    let mut s = accept(h);
    let frame = h["authFrameB64"].as_str().unwrap();
    assert!(s.open_text(&format!("{frame}=")).is_err());
    assert!(s.open_text(&format!(" {frame}")).is_err());
}

#[test]
fn given_a_client_that_pinned_another_host_key_when_it_sends_auth_then_this_host_cannot_open_it() {
    let h = &vectors()["handshakes"][0];
    let other = KeyPair::from_secret([0x77; 32]);
    let mut s = DesktopSession::accept(&h["hello"], &other, &Transport::Direct, &FixedEntropy(0x44)).unwrap();
    assert!(s.open_text(h["authFrameB64"].as_str().unwrap()).is_err(), "another identity derives another session");
}

#[test]
fn given_malformed_or_misrouted_hellos_when_accepted_then_each_is_refused_with_its_reason() {
    let h = &vectors()["handshakes"][0];
    let go = |hello: &Value, t: &Transport| DesktopSession::accept(hello, &server(), t, &FixedEntropy(0x44)).err();
    assert_eq!(go(&h["hello"], &Transport::Relay { host_id: "AbCdEf0123_-xyZ9".into() }), Some(HandshakeRefusal::Context), "arrived on another transport");
    let mut extra = h["hello"].clone();
    extra["surprise"] = json!(1);
    assert_eq!(go(&extra, &Transport::Direct), Some(HandshakeRefusal::Shape));
    let mut v3 = h["hello"].clone();
    v3["v"] = json!(3);
    assert_eq!(go(&v3, &Transport::Direct), Some(HandshakeRefusal::Shape));
    let mut short = h["hello"].clone();
    short["clientPublicKeyB64"] = json!("AAAA");
    assert_eq!(go(&short, &Transport::Direct), Some(HandshakeRefusal::Keys));
    let mut alias = h["hello"].clone();
    alias["clientNonceB64"] = json!(h["hello"]["clientNonceB64"].as_str().unwrap().replace('=', "") );
    assert_eq!(go(&alias, &Transport::Direct), Some(HandshakeRefusal::Keys));
    let low_order = base64_encode(&[0u8; 32]);
    let mut zero = h["hello"].clone();
    zero["clientPublicKeyB64"] = json!(low_order);
    assert_eq!(go(&zero, &Transport::Direct), Some(HandshakeRefusal::Keys), "a small-order key yields an all-zero secret");
    assert_eq!(go(&json!("hello"), &Transport::Direct), Some(HandshakeRefusal::Context));
}

fn registry_with(token: &str) -> MemoryDevices {
    let devices = MemoryDevices::default();
    devices.register(Device { id: "phone-1".into(), token: token.into() }).unwrap();
    devices
}

#[test]
fn given_the_recorded_v2_auth_when_the_token_is_registered_then_the_device_authenticates() {
    let h = &vectors()["handshakes"][0];
    let text = h["authText"].as_str().unwrap();
    let token = serde_json::from_str::<Value>(text).unwrap()["deviceToken"].as_str().unwrap().to_string();
    let ok = authenticate(text, Some(h["transcriptHashB64"].as_str().unwrap()), &registry_with(&token)).unwrap();
    assert_eq!(ok.device.id, "phone-1");
}

#[test]
fn given_a_wrong_token_or_a_mismatched_transcript_or_a_mixed_shape_when_authenticating_then_each_is_refused() {
    let h = &vectors()["handshakes"][0];
    let text = h["authText"].as_str().unwrap();
    let hash = h["transcriptHashB64"].as_str().unwrap();
    let token = serde_json::from_str::<Value>(text).unwrap()["deviceToken"].as_str().unwrap().to_string();
    let devices = registry_with(&format!("{token}x"));
    assert_eq!(authenticate(text, Some(hash), &devices).err(), Some(AuthFailure::Unauthorized), "wrong token");
    let devices = registry_with(&token);
    assert_eq!(authenticate(text, Some("AAAA"), &devices).err(), Some(AuthFailure::BadAuth), "another session's transcript");
    let mut with_caps: Value = serde_json::from_str(text).unwrap();
    with_caps["clientCapabilities"] = json!(["x"]);
    assert_eq!(authenticate(&with_caps.to_string(), Some(hash), &devices).err(), Some(AuthFailure::BadAuth), "v2 token auth is exact-key");
    assert_eq!(authenticate(text, None, &devices).err(), Some(AuthFailure::BadAuth), "a v2 shape on legacy framing");
    let key_auth = json!({"type": "e2ee_auth", "v": 2, "transcriptHashB64": hash, "deviceId": "d", "deviceProofB64": "p"});
    assert_eq!(authenticate(&key_auth.to_string(), Some(hash), &devices).err(), Some(AuthFailure::BadAuth), "key auth needs a verifier");
    assert_eq!(authenticate("not json", Some(hash), &devices).err(), Some(AuthFailure::BadAuth));
    devices.revoke("phone-1").unwrap();
    assert_eq!(authenticate(text, Some(hash), &devices).err(), Some(AuthFailure::Unauthorized), "revoked device");
    assert_eq!(AuthFailure::Unauthorized.sealed_error()["error"]["code"], "unauthorized");
}

#[test]
fn given_the_recorded_legacy_frame_when_opened_then_it_decrypts_and_any_damage_or_wrong_key_is_refused() {
    let v = vectors();
    let l = &v["legacy"];
    let client_public = base64_encode(&unhex(&v["clientPublicHex"]));
    let channel = LegacyChannel::new(&arr(&v["serverSecretHex"]), &client_public).unwrap();
    assert_eq!(channel.open_text(l["frameB64"].as_str().unwrap()).unwrap(), l["plaintext"].as_str().unwrap());
    let frame = unhex(&l["frameHex"]);
    for i in 0..frame.len() {
        let mut bad = frame.clone();
        bad[i] ^= 0x80;
        assert!(channel.open_bytes(&bad).is_none(), "flip at byte {i}");
    }
    assert!(channel.open_bytes(&frame[..frame.len() - 1]).is_none());
    assert!(channel.open_bytes(&frame[..23]).is_none());
    let stranger = LegacyChannel::new(&[0x99; 32], &client_public).unwrap();
    assert!(stranger.open_bytes(&frame).is_none());
    let sealed = channel.seal_text(&PatternEntropy::new(1), "hi");
    assert_eq!(channel.open_text(&sealed).as_deref(), Some("hi"));
    assert_eq!(serde_json::from_str::<Value>(READY_TEXT).unwrap()["type"], "e2ee_ready");
    assert!(LegacyChannel::new(&arr(&v["serverSecretHex"]), &base64_encode(&[0u8; 32])).is_none(), "small-order client key");
}

#[test]
fn given_the_reference_offer_when_encoded_and_parsed_then_the_uri_is_identical_and_round_trips() {
    let v = vectors();
    let offer: PairingOffer = serde_json::from_value(v["offer"]["json"].clone()).unwrap();
    assert_eq!(offer.to_uri(), v["offer"]["uri"].as_str().unwrap());
    assert_eq!(PairingOffer::parse(v["offer"]["uri"].as_str().unwrap()).unwrap(), offer);
    let code = v["offer"]["uri"].as_str().unwrap().trim_start_matches("orca://pair?code=");
    assert_eq!(PairingOffer::parse(code).unwrap(), offer, "bare code, as pasted");
    assert_eq!(offer.host_public().unwrap().to_vec(), unhex(&v["serverPublicHex"]));
}

#[test]
fn given_unusable_pairing_input_when_parsed_then_each_is_refused() {
    let v = vectors();
    let uri = v["offer"]["uri"].as_str().unwrap();
    assert_eq!(PairingOffer::parse("https://example.com/pair?code=abc"), Err(OfferRefusal::NotAPairingLink));
    assert_eq!(PairingOffer::parse("orca://pair?code=***"), Err(OfferRefusal::BadCode));
    assert_eq!(PairingOffer::parse(&format!("orca://pair?code={}", "A".repeat(128 * 1024 + 1))), Err(OfferRefusal::BadCode));
    let enc = |j: Value| format!("orca://pair?code={}", base64_url(j.to_string().as_bytes()));
    let mut old = v["offer"]["json"].clone();
    old["v"] = json!(1);
    assert_eq!(PairingOffer::parse(&enc(old)), Err(OfferRefusal::BadOffer));
    let mut relay = v["offer"]["json"].clone();
    relay["relay"] = json!({"v": 1});
    assert_eq!(PairingOffer::parse(&enc(relay)), Err(OfferRefusal::RelayUnsupported));
    let mut empty = v["offer"]["json"].clone();
    empty["deviceToken"] = json!("");
    assert_eq!(PairingOffer::parse(&enc(empty)), Err(OfferRefusal::BadOffer));
    assert!(PairingOffer::parse(&uri[..uri.len() - 5]).is_err(), "a cut code");
}

#[test]
fn given_a_fresh_store_when_the_identity_is_loaded_twice_then_the_same_pair_comes_back_and_minted_devices_are_distinct() {
    let store = MemoryKeyPairStore::default();
    let e = PatternEntropy::new(9);
    let first = load_or_create_keypair(&store, &e).unwrap();
    assert_eq!(load_or_create_keypair(&store, &e).unwrap(), first);
    assert_eq!(store.load().unwrap(), Some(first));
    let (a, b) = (mint_device(&e), mint_device(&e));
    assert_ne!(a, b);
    assert_eq!(a.token.len(), 48);
    assert_eq!(a.id.len(), 36);
    assert_eq!(a.id.as_bytes()[14], b'4', "uuid version nibble");
}

fn base64_encode(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn base64_url(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn orca_crypto_b64(s: &str) -> Vec<u8> {
    orca_crypto::b64::decode_canonical(s).unwrap()
}

/// The ops the Rust crate answers from a recorded input alone: it holds the desktop side of the handshake, so the recorded hello is the
/// input of the tests above, not something it can derive.
mod replay {
    use super::*;
    use mimic_core::allowlist::Allowlist;
    use mimic_core::cassette::{Cassette, DriverInfo};
    use mimic_core::differ::{DiffOptions, Severity};
    use mimic_core::ports::{CassetteSet, Driver, DriverError, Hello, Observed, Request};
    use mimic_core::rules::RuleSet;
    use mimic_core::usecase::replay;

    /// The crate as a candidate: `answer` is its behaviour, `plant` an edit of its reply to plant a defect.
    struct Candidate(fn(&Request, &mut Value));

    impl Driver for Candidate {
        fn hello(&mut self) -> Result<Hello, DriverError> {
            let driver = DriverInfo { name: "orca-crypto-rust".into(), version: "1".into(), protocol: "mimic-driver/1".into(), toolchain_lock_hash: None };
            Ok(Hello { driver, target_version: None, coverage: false })
        }

        fn run(&mut self, r: &Request) -> Result<Observed, DriverError> {
            let mut payload = answer(r).ok_or_else(|| DriverError(format!("no op {:?}", r.op)))?;
            (self.0)(r, &mut payload);
            Ok(serde_json::from_value(json!({"steps": [{"dir": "ret", "kind": "json", "payload": payload}], "outcome": {"status": "ok", "reference_accepts": true}})).unwrap())
        }
    }

    fn load_set() -> CassetteSet {
        let read = |name: &str| std::fs::read_to_string(set_dir().join(name)).unwrap();
        let cassette = Cassette::parse(&read("cassette.json"), &read("interactions.jsonl")).unwrap();
        CassetteSet { cassette, rules: serde_json::from_str::<RuleSet>(&read("rules.json")).unwrap(), corpus: None }
    }

    fn fill(hex: &str, n: usize) -> Vec<u8> {
        vec![u8::from_str_radix(hex, 16).unwrap(); n]
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn key(i: &Value, name: &str) -> [u8; 32] {
        fill(i[name].as_str().unwrap(), 32).try_into().unwrap()
    }

    fn answer(r: &Request) -> Option<Value> {
        let i = &r.input;
        Some(match r.op.as_deref()? {
            "crypto.keypair" => {
                let k = KeyPair::from_secret(key(i, "secretFill"));
                json!({"secretHex": hex(&key(i, "secretFill")), "publicHex": hex(&k.public)})
            }
            "crypto.fixed-bytes" => json!({"hex": hex(&fill(i["fill"].as_str().unwrap(), i["length"].as_u64().unwrap() as usize))}),
            "crypto.box-shared-key" => {
                let client = KeyPair::from_secret(key(i, "clientSecretFill"));
                json!({"hex": hex(&orca_crypto::boxes::shared_key(&key(i, "serverSecretFill"), &client.public).unwrap())})
            }
            "crypto.legacy" => {
                let (server, client) = (KeyPair::from_secret(key(i, "serverSecretFill")), KeyPair::from_secret(key(i, "clientSecretFill")));
                let channel = LegacyChannel::new(&server.secret, &base64_encode(&client.public)).unwrap();
                let nonce = u8::from_str_radix(i["nonceFill"].as_str().unwrap(), 16).unwrap();
                let frame = channel.seal_bytes(&FixedEntropy(nonce), i["plaintext"].as_str().unwrap().as_bytes());
                json!({"plaintext": i["plaintext"], "nonceHex": hex(&frame[..24]), "frameHex": hex(&frame), "frameB64": base64_encode(&frame)})
            }
            "crypto.offer" => {
                let host = KeyPair::from_secret(key(i, "serverSecretFill"));
                let mut json = i["offer"].clone();
                json["publicKeyB64"] = json!(base64_encode(&host.public));
                let offer: PairingOffer = serde_json::from_value(json.clone()).unwrap();
                json!({"json": json, "uri": offer.to_uri()})
            }
            _ => return None,
        })
    }

    const OPS: [&str; 5] = ["crypto.keypair", "crypto.fixed-bytes", "crypto.box-shared-key", "crypto.legacy", "crypto.offer"];

    #[test]
    fn given_the_recorded_set_when_the_rust_crate_answers_the_inputs_it_can_then_every_reply_is_the_references() {
        // Given the recorded set, and the interactions whose input the crate can answer alone
        let set = load_set();
        let ids: Vec<String> = set.cassette.interactions.iter().filter(|i| OPS.contains(&i.op.as_deref().unwrap_or(""))).map(|i| i.id.clone()).collect();
        assert!(ids.len() >= OPS.len(), "each op is recorded at least once: {ids:?}");
        // When the set is replayed against it
        let verdict = replay(&set, &mut Candidate(|_, _| {}), &Allowlist::default(), &DiffOptions::default(), Some(&ids)).unwrap();
        // Then every one is clean
        assert_eq!((verdict.severity, verdict.summary.total, verdict.summary.clean), (Severity::None, ids.len(), ids.len()), "{:?}", verdict.results);
    }

    #[test]
    fn given_a_crate_that_derives_a_wrong_key_when_the_set_is_replayed_then_the_difference_is_found() {
        // Given the same candidate with the public key of the pair planted wrong
        let mut wrong = Candidate(|r, payload| {
            if r.op.as_deref() == Some("crypto.keypair") {
                payload["publicHex"] = json!("00".repeat(32));
            }
        });
        // When the keypair interactions are replayed
        let only = vec!["keypair-client".to_string(), "keypair-server".to_string()];
        let verdict = replay(&load_set(), &mut wrong, &Allowlist::default(), &DiffOptions::default(), Some(&only)).unwrap();
        // Then the replay is not clean
        assert_ne!(verdict.severity, Severity::None);
    }
}
