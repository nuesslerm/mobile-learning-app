//! The desktop side of the E2EE v2 handshake: validate the phone's `e2ee_hello`, answer `e2ee_ready`, derive the per-direction keys from the
//! shared secret and the transcript, then seal and open counted frames. Mirrors `mobile-e2ee-v2-contract.ts` and the desktop session.
use crate::boxes::{shared_key, KeyPair, KEY_LEN};
use crate::entropy::{random_array, Entropy};
use crate::v2_frame::{self, Direction, FrameRefusal, Kind, SESSION_ID_LEN};
use crate::b64;
use hkdf::Hkdf;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

const PROTOCOL: &str = "orca-mobile-e2ee";
const TRANSCRIPT_DOMAIN: &str = "orca-mobile-e2ee/v2/transcript";
const SALT_LABEL: &[u8] = b"orca-mobile-e2ee/v2/salt\0";
const INFO_LABEL: &[u8] = b"orca-mobile-e2ee/v2/session\0";

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Transport {
    Direct,
    Relay { host_id: String },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandshakeRefusal {
    NotAnObject,
    Shape,
    Context,
    Keys,
}

pub struct KeySchedule {
    pub mobile_to_desktop: [u8; KEY_LEN],
    pub desktop_to_mobile: [u8; KEY_LEN],
    pub session_id: [u8; SESSION_ID_LEN],
    pub transcript_hash: [u8; 32],
}

pub fn derive_key_schedule(
    shared_secret: &[u8; KEY_LEN],
    transcript: &[u8],
    client_nonce: &[u8; 32],
    desktop_nonce: &[u8; 32],
) -> KeySchedule {
    let transcript_hash: [u8; 32] = Sha256::digest(transcript).into();
    let salt = Sha256::new().chain_update(SALT_LABEL).chain_update(client_nonce).chain_update(desktop_nonce).finalize();
    let info = [INFO_LABEL, &transcript_hash].concat();
    let mut okm = [0u8; 96];
    Hkdf::<Sha256>::new(Some(&salt), shared_secret).expand(&info, &mut okm).expect("96 bytes is within the HKDF limit");
    KeySchedule {
        mobile_to_desktop: okm[..32].try_into().unwrap(),
        desktop_to_mobile: okm[32..64].try_into().unwrap(),
        session_id: okm[64..].try_into().unwrap(),
        transcript_hash,
    }
}

/// The key schedule of a hello/ready pair under a given shared secret (the pinned normative vector feeds it a fixed one).
pub fn schedule_from_messages(hello: &Value, ready: &Value, shared_secret: &[u8; KEY_LEN]) -> Result<KeySchedule, HandshakeRefusal> {
    let v = validate(hello, ready)?;
    Ok(derive_key_schedule(shared_secret, &transcript(&v), &v.client_nonce, &v.desktop_nonce))
}

struct Validated {
    client_public: [u8; 32],
    desktop_public: [u8; 32],
    client_nonce: [u8; 32],
    desktop_nonce: [u8; 32],
    transport: Transport,
}

fn exact_keys(map: &Map<String, Value>, keys: &[&str]) -> bool {
    map.len() == keys.len() && keys.iter().all(|k| map.contains_key(*k))
}

fn string_list(value: &Value, expected: &[&str]) -> bool {
    value.as_array().is_some_and(|a| a.len() == expected.len() && a.iter().zip(expected).all(|(v, e)| v.as_str() == Some(e)))
}

fn parse_context(value: &Value) -> Option<Transport> {
    let map = value.as_object()?;
    let relay = map.get("transport")?.as_str()? == "relay";
    let keys: &[&str] = if relay {
        &["protocol", "initiator", "responder", "transport", "relayHostId"]
    } else {
        &["protocol", "initiator", "responder", "transport"]
    };
    if !exact_keys(map, keys) || map["protocol"] != PROTOCOL || map["initiator"] != "mobile" || map["responder"] != "desktop" {
        return None;
    }
    if !relay {
        return (map["transport"] == "direct").then_some(Transport::Direct);
    }
    let id = map["relayHostId"].as_str()?;
    let ok = id.len() == 16 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then(|| Transport::Relay { host_id: id.to_string() })
}

fn validate(hello: &Value, ready: &Value) -> Result<Validated, HandshakeRefusal> {
    use HandshakeRefusal::*;
    let (h, r) = (hello.as_object().ok_or(NotAnObject)?, ready.as_object().ok_or(NotAnObject)?);
    if !exact_keys(h, &["type", "v", "clientPublicKeyB64", "clientNonceB64", "capabilities", "context"])
        || !exact_keys(r, &["type", "v", "desktopPublicKeyB64", "clientNonceB64", "desktopNonceB64", "selection", "context"])
        || h["type"] != "e2ee_hello"
        || h["v"] != 2
        || r["type"] != "e2ee_ready"
        || r["v"] != 2
    {
        return Err(Shape);
    }
    let caps = h["capabilities"].as_object().ok_or(Shape)?;
    let selection = r["selection"].as_object().ok_or(Shape)?;
    if !exact_keys(caps, &["framing", "payloadKinds"])
        || caps["framing"] != json!([2])
        || !string_list(&caps["payloadKinds"], &["text", "binary"])
        || !exact_keys(selection, &["framing", "payloadKinds"])
        || selection["framing"] != 2
        || !string_list(&selection["payloadKinds"], &["text", "binary"])
    {
        return Err(Shape);
    }
    let transport = parse_context(&h["context"]).ok_or(Context)?;
    if parse_context(&r["context"]).as_ref() != Some(&transport) || r["clientNonceB64"] != h["clientNonceB64"] {
        return Err(Context);
    }
    let key = |v: &Value| b64::decode_canonical_exact::<32>(v.as_str()?);
    match (key(&h["clientPublicKeyB64"]), key(&r["desktopPublicKeyB64"]), key(&h["clientNonceB64"]), key(&r["desktopNonceB64"])) {
        (Some(client_public), Some(desktop_public), Some(client_nonce), Some(desktop_nonce)) => {
            Ok(Validated { client_public, desktop_public, client_nonce, desktop_nonce, transport })
        }
        _ => Err(Keys),
    }
}

fn field(name: &str, value: &[u8], out: &mut Vec<u8>) {
    out.extend((name.len() as u32).to_be_bytes());
    out.extend(name.as_bytes());
    out.extend((value.len() as u32).to_be_bytes());
    out.extend(value);
}

fn list(items: &[&str]) -> Vec<u8> {
    let mut out = (items.len() as u32).to_be_bytes().to_vec();
    for item in items {
        out.extend((item.len() as u32).to_be_bytes());
        out.extend(item.as_bytes());
    }
    out
}

/// The bytes both sides hash into the session keys; every field is length-prefixed, so no two handshakes share an encoding.
fn transcript(v: &Validated) -> Vec<u8> {
    let (kind, host) = match &v.transport {
        Transport::Direct => ("direct", ""),
        Transport::Relay { host_id } => ("relay", host_id.as_str()),
    };
    let u32s = |n: u32| n.to_be_bytes().to_vec();
    let context = |side: &str, out: &mut Vec<u8>| {
        field(&format!("{side}.context.protocol"), PROTOCOL.as_bytes(), out);
        field(&format!("{side}.context.initiator"), b"mobile", out);
        field(&format!("{side}.context.responder"), b"desktop", out);
        field(&format!("{side}.context.transport"), kind.as_bytes(), out);
        field(&format!("{side}.context.relay-host-id"), host.as_bytes(), out);
    };
    let mut out = Vec::new();
    field("domain", TRANSCRIPT_DOMAIN.as_bytes(), &mut out);
    field("mobile-to-desktop.type", b"e2ee_hello", &mut out);
    field("mobile-to-desktop.version", &u32s(2), &mut out);
    field("mobile-to-desktop.client-public-key", &v.client_public, &mut out);
    field("mobile-to-desktop.client-nonce", &v.client_nonce, &mut out);
    field("mobile-to-desktop.capabilities.framing", &[u32s(1), u32s(2)].concat(), &mut out);
    field("mobile-to-desktop.capabilities.payload-kinds", &list(&["text", "binary"]), &mut out);
    context("mobile-to-desktop", &mut out);
    field("desktop-to-mobile.type", b"e2ee_ready", &mut out);
    field("desktop-to-mobile.version", &u32s(2), &mut out);
    field("desktop-to-mobile.desktop-public-key", &v.desktop_public, &mut out);
    field("desktop-to-mobile.client-nonce-echo", &v.client_nonce, &mut out);
    field("desktop-to-mobile.desktop-nonce", &v.desktop_nonce, &mut out);
    field("desktop-to-mobile.selection.framing", &u32s(2), &mut out);
    field("desktop-to-mobile.selection.payload-kinds", &list(&["text", "binary"]), &mut out);
    context("desktop-to-mobile", &mut out);
    out
}

pub struct DesktopSession {
    pub ready: Value,
    transcript_hash_b64: String,
    keys: KeySchedule,
    inbound: u64,
    outbound: u64,
}

impl DesktopSession {
    /// `expected` is the transport this socket arrived on: a hello claiming another one is refused.
    pub fn accept(hello: &Value, server: &KeyPair, expected: &Transport, entropy: &dyn Entropy) -> Result<Self, HandshakeRefusal> {
        let context = hello.get("context").and_then(parse_context).ok_or(HandshakeRefusal::Context)?;
        if &context != expected {
            return Err(HandshakeRefusal::Context);
        }
        let desktop_nonce: [u8; 32] = random_array(entropy);
        let ready = json!({
            "type": "e2ee_ready",
            "v": 2,
            "desktopPublicKeyB64": b64::encode(&server.public),
            "clientNonceB64": hello.get("clientNonceB64").cloned().unwrap_or(Value::Null),
            "desktopNonceB64": b64::encode(&desktop_nonce),
            "selection": {"framing": 2, "payloadKinds": ["text", "binary"]},
            "context": hello["context"].clone(),
        });
        let v = validate(hello, &ready)?;
        let secret = shared_key(&server.secret, &v.client_public).ok_or(HandshakeRefusal::Keys)?;
        let keys = schedule_from_messages(hello, &ready, &secret)?;
        Ok(Self { ready, transcript_hash_b64: b64::encode(&keys.transcript_hash), keys, inbound: 0, outbound: 0 })
    }

    pub fn transcript_hash_b64(&self) -> &str {
        &self.transcript_hash_b64
    }

    pub fn open_text(&mut self, frame_b64: &str) -> Result<String, FrameRefusal> {
        let frame = b64::decode_canonical(frame_b64).ok_or(FrameRefusal::TooShort)?;
        String::from_utf8(self.open(&frame, Kind::Text)?).map_err(|_| FrameRefusal::Unauthentic)
    }

    pub fn open_binary(&mut self, frame: &[u8]) -> Result<Vec<u8>, FrameRefusal> {
        self.open(frame, Kind::Binary)
    }

    pub fn seal_text(&mut self, plaintext: &str) -> Option<String> {
        self.seal(plaintext.as_bytes(), Kind::Text).map(|f| b64::encode(&f))
    }

    pub fn seal_binary(&mut self, plaintext: &[u8]) -> Option<Vec<u8>> {
        self.seal(plaintext, Kind::Binary)
    }

    // A frame that fails to open does not advance the counter, so garbage cannot desynchronise a live socket.
    fn open(&mut self, frame: &[u8], kind: Kind) -> Result<Vec<u8>, FrameRefusal> {
        let k = &self.keys;
        let out = v2_frame::open(&k.mobile_to_desktop, &k.session_id, Direction::MobileToDesktop, kind, self.inbound, frame)?;
        self.inbound += 1;
        Ok(out)
    }

    // None once the 64-bit counter is spent; the socket must be re-keyed.
    fn seal(&mut self, payload: &[u8], kind: Kind) -> Option<Vec<u8>> {
        let k = &self.keys;
        let frame = v2_frame::seal(&k.desktop_to_mobile, &k.session_id, Direction::DesktopToMobile, kind, self.outbound, payload);
        self.outbound = self.outbound.checked_add(1)?;
        Some(frame)
    }
}
