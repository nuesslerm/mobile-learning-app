//! The real transport over loopback with every port a double, and a phone-side client built from the primitives the recorded vectors pin
//! (`orca-crypto`): the flow tests' server and caller.
use crate::transport::{self, Running, Shared};
use crate::wiring;
use orca_core::area::Out;
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::wire::{self, Failure};
use orca_core::{Area, Ctx, Host, Kind as MethodKind, Method, ServiceStatus, Subscription};
use orca_crypto::legacy::LegacyChannel;
use orca_crypto::pairing::PairingOffer;
use orca_crypto::ports::{mint_device, Device, DeviceRegistry};
use orca_crypto::testing::{MemoryDevices, PatternEntropy};
use orca_crypto::v2::schedule_from_messages;
use orca_crypto::v2_frame::{self, Direction, Kind, SESSION_ID_LEN};
use orca_crypto::{b64, boxes, KeyPair, SystemEntropy};
use serde_json::{json, Value};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::{Error, Message, WebSocket};

pub const RUNTIME_ID: &str = "11111111-1111-4111-8111-111111111111";

pub struct Harness {
    pub agent: Arc<ScriptedAgent>,
    pub host: Arc<Host>,
    pub devices: Arc<MemoryDevices>,
    pub principals: Arc<RevocablePrincipals>,
    /// A paired phone's device, registered in `devices`.
    pub device: Device,
    pub host_public: [u8; 32],
    /// The binary frames the `probe.wire` stream received, in order (empty unless started with `start_with_probe`).
    pub probe_inbound: Arc<Mutex<Vec<Vec<u8>>>>,
    running: Option<Running>,
    pub addr: SocketAddr,
}

/// Payload of the one binary frame `probe.wire` emits.
pub const PROBE_PAYLOAD: &[u8] = b"probe-bytes";

struct ProbeArea(Arc<Mutex<Vec<Vec<u8>>>>);

struct ProbeStream {
    id: String,
    number: u32,
    opened: bool,
    inbound: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Subscription for ProbeStream {
    fn id(&self) -> &str {
        &self.id
    }
    fn poll(&mut self) -> Vec<Value> {
        Vec::new()
    }
    fn end(&mut self) -> Vec<Value> {
        Vec::new()
    }
    fn poll_out(&mut self) -> Vec<Out> {
        if std::mem::replace(&mut self.opened, true) {
            return Vec::new();
        }
        let frame = [vec![0x74, 1, 0, 0], self.number.to_le_bytes().to_vec(), PROBE_PAYLOAD.to_vec()].concat();
        vec![Out::Text(json!({"type": "subscribed", "streamNumber": self.number}).to_string()), Out::Binary(frame), Out::Text(json!({"type": "after"}).to_string())]
    }
    fn number(&self) -> Option<u32> {
        Some(self.number)
    }
    fn on_binary(&mut self, frame: &[u8]) -> Vec<Out> {
        self.inbound.lock().unwrap().push(frame.to_vec());
        Vec::new()
    }
}

impl Area for ProbeArea {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn methods(&self) -> &'static [Method] {
        &[Method { name: "probe.wire", kind: MethodKind::Streaming }]
    }
    fn call(&self, _ctx: &mut Ctx, method: &str, _params: &Value) -> Result<Value, Failure> {
        Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {method}")))
    }
    fn open(&self, ctx: &mut Ctx, _method: &str, _params: &Value) -> Result<Box<dyn Subscription>, Failure> {
        Ok(Box::new(ProbeStream { id: ctx.subscription_id("probe-"), number: ctx.stream_number(), opened: false, inbound: self.0.clone() }))
    }
}

impl Harness {
    pub fn start(status: ServiceStatus) -> Harness {
        Harness::start_with_bundle(status, Box::new(orca_map_mobile::NoBundle))
    }

    /// The server of `start` holding the web bundle `bundle` reads.
    pub fn start_with_bundle(status: ServiceStatus, bundle: Box<dyn orca_map_mobile::BundleSource>) -> Harness {
        Harness::start_full(status, bundle, transport::HANDSHAKE_DEADLINE, false)
    }

    /// The server of `start` plus the `probe.wire` stream: one JSON frame, one binary frame, one JSON frame, and it records inbound binary.
    pub fn start_with_probe(status: ServiceStatus) -> Harness {
        Harness::start_full(status, Box::new(orca_map_mobile::NoBundle), transport::HANDSHAKE_DEADLINE, true)
    }

    /// The server of `start` with a shorter handshake deadline, for the tests of the 4002 close.
    pub fn start_with_deadline(status: ServiceStatus, deadline: Duration) -> Harness {
        Harness::start_full(status, Box::new(orca_map_mobile::NoBundle), deadline, false)
    }

    fn start_full(status: ServiceStatus, bundle: Box<dyn orca_map_mobile::BundleSource>, deadline: Duration, probe: bool) -> Harness {
        let agent = Arc::new(ScriptedAgent::serving(status));
        let keys = KeyPair::generate(&PatternEntropy::new(7));
        let principals = Arc::new(RevocablePrincipals::default());
        let probe_inbound = Arc::new(Mutex::new(Vec::new()));
        let mut areas = wiring::areas(orca_map_mobile::MobileWebArea::new(bundle));
        if probe {
            areas.push(Box::new(ProbeArea(probe_inbound.clone())));
        }
        let host = Host::new(RUNTIME_ID.into(), areas, agent.clone(), principals.clone(), wiring::host_info("test rca")).expect("a table without duplicates");
        let devices = Arc::new(MemoryDevices::default());
        let device = mint_device(&PatternEntropy::new(99));
        devices.register(device.clone()).expect("a device");
        let host_public = keys.public;
        let shared = Shared { host: host.clone(), keys: Arc::new(keys), devices: devices.clone(), entropy: Arc::new(SystemEntropy), handshake_deadline: deadline };
        let running = transport::serve("127.0.0.1:0", shared).expect("loopback");
        let addr = running.addr();
        Harness { agent, host, devices, principals, device, host_public, probe_inbound, running: Some(running), addr }
    }

    /// The link `orca-serve pair` would print for this device.
    pub fn pairing_link(&self) -> String {
        PairingOffer::direct(&format!("ws://{}", self.addr), &self.device, &self.host_public).to_uri()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop();
        }
    }
}

/// How a socket ended: the close frame's code and reason.
#[derive(Debug, Clone, PartialEq)]
pub struct Closed {
    pub code: u16,
    pub reason: String,
}

enum Keys {
    V2 { to_host: [u8; 32], from_host: [u8; 32], session: [u8; SESSION_ID_LEN], out: u64, inn: u64, transcript_hash_b64: String },
    Legacy(LegacyChannel),
}

pub struct PhoneClient {
    ws: WebSocket<TcpStream>,
    keys: Option<Keys>,
    secret: [u8; 32],
    public: [u8; 32],
    host_public: [u8; 32],
    entropy: SystemEntropy,
    next_id: u64,
    stashed: std::collections::VecDeque<Value>,
    binaries: std::collections::VecDeque<Vec<u8>>,
}

impl PhoneClient {
    pub fn connect(addr: SocketAddr, host_public: [u8; 32]) -> PhoneClient {
        let stream = TcpStream::connect(addr).expect("the server listens");
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let request = format!("ws://{addr}/").into_client_request().unwrap();
        let (ws, _) = tungstenite::client(request, stream).expect("a WebSocket upgrade");
        let keys = KeyPair::generate(&SystemEntropy);
        PhoneClient { ws, keys: None, secret: keys.secret, public: keys.public, host_public, entropy: SystemEntropy, next_id: 0, stashed: Default::default(), binaries: Default::default() }
    }

    pub fn send_raw(&mut self, text: &str) {
        self.ws.send(Message::text(text)).expect("the socket accepts a frame");
    }

    pub fn send_binary(&mut self, bytes: Vec<u8>) {
        self.ws.send(Message::binary(bytes)).expect("the socket accepts a frame");
    }

    /// A WebSocket ping: traffic that is no handshake step.
    pub fn send_ping(&mut self) -> bool {
        self.ws.send(Message::Ping(vec![0u8].into())).is_ok()
    }

    fn recv_message(&mut self) -> Result<Message, Closed> {
        loop {
            match self.ws.read() {
                Ok(Message::Close(frame)) => return Err(Closed { code: frame.as_ref().map_or(1005, |f| u16::from(f.code)), reason: frame.map(|f| f.reason.to_string()).unwrap_or_default() }),
                Ok(m @ (Message::Text(_) | Message::Binary(_))) => return Ok(m),
                Ok(_) => continue,
                Err(Error::ConnectionClosed | Error::AlreadyClosed) => return Err(Closed { code: 1006, reason: "closed without a close frame".into() }),
                Err(e) => panic!("the socket failed: {e}"),
            }
        }
    }

    pub fn recv_plain(&mut self) -> Result<Value, Closed> {
        match self.recv_message()? {
            Message::Text(t) => Ok(serde_json::from_str(t.as_str()).expect("plain JSON")),
            Message::Binary(_) => panic!("a binary frame before the handshake ended"),
            _ => unreachable!(),
        }
    }

    /// Sends the v2 `e2ee_hello` and returns the host's `e2ee_ready`.
    pub fn hello_v2(&mut self) -> Result<Value, Closed> {
        let nonce: [u8; 32] = orca_crypto::entropy::random_array(&self.entropy);
        let context = json!({"protocol": "orca-mobile-e2ee", "initiator": "mobile", "responder": "desktop", "transport": "direct"});
        let hello = json!({"type": "e2ee_hello", "v": 2, "clientPublicKeyB64": b64::encode(&self.public), "clientNonceB64": b64::encode(&nonce), "capabilities": {"framing": [2], "payloadKinds": ["text", "binary"]}, "context": context});
        self.send_raw(&hello.to_string());
        let ready = self.recv_plain()?;
        let shared = boxes::shared_key(&self.secret, &self.host_public).expect("a host key of full order");
        let k = schedule_from_messages(&hello, &ready, &shared).expect("a ready that answers this hello");
        self.keys = Some(Keys::V2 { to_host: k.mobile_to_desktop, from_host: k.desktop_to_mobile, session: k.session_id, out: 0, inn: 0, transcript_hash_b64: b64::encode(&k.transcript_hash) });
        Ok(ready)
    }

    /// Sends the legacy `e2ee_hello` and returns the host's plain `e2ee_ready`.
    pub fn hello_legacy(&mut self) -> Result<Value, Closed> {
        self.send_raw(&json!({"type": "e2ee_hello", "publicKeyB64": b64::encode(&self.public)}).to_string());
        let ready = self.recv_plain()?;
        self.keys = Some(Keys::Legacy(LegacyChannel::new(&self.secret, &b64::encode(&self.host_public)).expect("a host key")));
        Ok(ready)
    }

    pub fn send_sealed(&mut self, plaintext: &str) {
        let text = match self.keys.as_mut().expect("a hello came first") {
            Keys::V2 { to_host, session, out, .. } => {
                let frame = v2_frame::seal(to_host, session, Direction::MobileToDesktop, Kind::Text, *out, plaintext.as_bytes());
                *out += 1;
                b64::encode(&frame)
            }
            Keys::Legacy(c) => c.seal_text(&self.entropy, plaintext),
        };
        self.send_raw(&text);
    }

    /// The next sealed text frame; binary frames before it are kept for `next_binary`.
    pub fn recv_sealed(&mut self) -> Result<Value, Closed> {
        loop {
            match self.recv_message()? {
                Message::Text(text) => return Ok(self.open_text(text.as_str())),
                Message::Binary(bytes) => {
                    let plain = self.open_binary(&bytes);
                    self.binaries.push_back(plain);
                }
                _ => unreachable!(),
            }
        }
    }

    fn open_text(&mut self, text: &str) -> Value {
        let plain = match self.keys.as_mut().expect("a hello came first") {
            Keys::V2 { from_host, session, inn, .. } => {
                let frame = b64::decode_canonical(text).expect("canonical base64");
                let plain = v2_frame::open(from_host, session, Direction::DesktopToMobile, Kind::Text, *inn, &frame).expect("a frame this phone can open");
                *inn += 1;
                String::from_utf8(plain).expect("UTF-8")
            }
            Keys::Legacy(c) => c.open_text(text).expect("a frame this phone can open"),
        };
        serde_json::from_str(&plain).expect("sealed JSON")
    }

    fn open_binary(&mut self, bytes: &[u8]) -> Vec<u8> {
        match self.keys.as_mut().expect("a hello came first") {
            Keys::V2 { from_host, session, inn, .. } => {
                let plain = v2_frame::open(from_host, session, Direction::DesktopToMobile, Kind::Binary, *inn, bytes).expect("a frame this phone can open");
                *inn += 1;
                plain
            }
            Keys::Legacy(c) => c.open_bytes(bytes).expect("a frame this phone can open"),
        }
    }

    /// The next binary frame, opened: one kept from an earlier read, or the next to arrive. Text frames read on the way are kept for `next_stream_frame`.
    pub fn next_binary(&mut self) -> Result<Vec<u8>, Closed> {
        loop {
            if let Some(plain) = self.binaries.pop_front() {
                return Ok(plain);
            }
            match self.recv_message()? {
                Message::Text(text) => {
                    let frame = self.open_text(text.as_str());
                    self.stashed.push_back(frame);
                }
                Message::Binary(bytes) => return Ok(self.open_binary(&bytes)),
                _ => unreachable!(),
            }
        }
    }

    /// Seals `plaintext` as a binary frame in the connection's framing and sends it.
    pub fn send_sealed_binary(&mut self, plaintext: &[u8]) {
        let frame = match self.keys.as_mut().expect("a hello came first") {
            Keys::V2 { to_host, session, out, .. } => {
                let frame = v2_frame::seal(to_host, session, Direction::MobileToDesktop, Kind::Binary, *out, plaintext);
                *out += 1;
                frame
            }
            Keys::Legacy(c) => c.seal_bytes(&self.entropy, plaintext),
        };
        self.send_binary(frame);
    }

    /// The next frame, which must be a sealed binary one, opened.
    pub fn recv_sealed_binary(&mut self) -> Result<Vec<u8>, Closed> {
        if let Some(plain) = self.binaries.pop_front() {
            return Ok(plain);
        }
        let Message::Binary(bytes) = self.recv_message()? else { panic!("expected a sealed binary frame") };
        Ok(self.open_binary(&bytes))
    }

    /// Sends `e2ee_auth` with the token (in the shape the framing needs) and returns the host's sealed answer.
    pub fn authenticate(&mut self, token: &str) -> Result<Value, Closed> {
        let auth = match self.keys.as_ref().expect("a hello came first") {
            Keys::V2 { transcript_hash_b64, .. } => json!({"type": "e2ee_auth", "v": 2, "transcriptHashB64": transcript_hash_b64, "deviceToken": token}),
            Keys::Legacy(_) => json!({"type": "e2ee_auth", "deviceToken": token}),
        };
        self.send_sealed(&auth.to_string());
        self.recv_sealed()
    }

    /// The v2 handshake and a token auth in one: the state in which requests can be made.
    /// The legacy handshake and a token auth in one.
    pub fn open_legacy(addr: SocketAddr, host_public: [u8; 32], token: &str) -> PhoneClient {
        let mut phone = PhoneClient::connect(addr, host_public);
        phone.hello_legacy().expect("e2ee_ready");
        let answer = phone.authenticate(token).expect("an answer to e2ee_auth");
        assert_eq!(answer["type"], "e2ee_authenticated", "the host accepted the token: {answer}");
        phone
    }

    pub fn open_v2(addr: SocketAddr, host_public: [u8; 32], token: &str) -> PhoneClient {
        let mut phone = PhoneClient::connect(addr, host_public);
        phone.hello_v2().expect("e2ee_ready");
        let answer = phone.authenticate(token).expect("an answer to e2ee_auth");
        assert_eq!(answer["type"], "e2ee_authenticated", "the host accepted the token: {answer}");
        phone
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> String {
        self.next_id += 1;
        let id = format!("req-{}", self.next_id);
        self.send_sealed(&json!({"id": id, "method": method, "params": params}).to_string());
        id
    }

    /// One request and its one answer: frames of other requests' streams in between are kept for `next_stream_frame`.
    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.send_request(method, params);
        loop {
            let frame = self.recv_sealed().expect("the socket stays open");
            if frame["id"] == id.as_str() && frame["streaming"] != true {
                return frame;
            }
            self.stashed.push_back(frame);
        }
    }

    /// The next frame of the stream a request opened.
    pub fn next_stream_frame(&mut self, id: &str) -> Value {
        if let Some(at) = self.stashed.iter().position(|f| f["id"] == id) {
            return self.stashed.remove(at).expect("a position of the queue");
        }
        loop {
            let frame = self.recv_sealed().expect("the socket stays open");
            if frame["id"] == id {
                return frame;
            }
        }
    }

    /// Reads until the host closes the socket.
    pub fn wait_closed(&mut self) -> Closed {
        loop {
            match self.recv_message() {
                Err(closed) => return closed,
                Ok(_) => continue,
            }
        }
    }
}
