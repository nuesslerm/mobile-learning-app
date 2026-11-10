//! The real transport over loopback with every port a double, and a phone-side client built from the primitives the recorded vectors pin
//! (`orca-crypto`): the flow tests' server and caller.
use crate::transport::{self, Running, Shared};
use crate::wiring;
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Host, ServiceStatus};
use orca_crypto::legacy::LegacyChannel;
use orca_crypto::pairing::PairingOffer;
use orca_crypto::ports::{mint_device, Device, DeviceRegistry};
use orca_crypto::testing::{MemoryDevices, PatternEntropy};
use orca_crypto::v2::schedule_from_messages;
use orca_crypto::v2_frame::{self, Direction, Kind, SESSION_ID_LEN};
use orca_crypto::{b64, boxes, KeyPair, SystemEntropy};
use serde_json::{json, Value};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
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
    running: Option<Running>,
    pub addr: SocketAddr,
}

impl Harness {
    pub fn start(status: ServiceStatus) -> Harness {
        Harness::start_with_bundle(status, Box::new(orca_map_mobile::NoBundle))
    }

    /// The server of `start` holding the web bundle `bundle` reads.
    pub fn start_with_bundle(status: ServiceStatus, bundle: Box<dyn orca_map_mobile::BundleSource>) -> Harness {
        Harness::start_full(status, bundle, transport::HANDSHAKE_DEADLINE)
    }

    /// The server of `start` with a shorter handshake deadline, for the tests of the 4002 close.
    pub fn start_with_deadline(status: ServiceStatus, deadline: Duration) -> Harness {
        Harness::start_full(status, Box::new(orca_map_mobile::NoBundle), deadline)
    }

    fn start_full(status: ServiceStatus, bundle: Box<dyn orca_map_mobile::BundleSource>, deadline: Duration) -> Harness {
        let agent = Arc::new(ScriptedAgent::serving(status));
        let keys = KeyPair::generate(&PatternEntropy::new(7));
        let principals = Arc::new(RevocablePrincipals::default());
        let host = Host::new(RUNTIME_ID.into(), wiring::areas(orca_map_mobile::MobileWebArea::new(bundle)), agent.clone(), principals.clone(), wiring::host_info("test rca")).expect("a table without duplicates");
        let devices = Arc::new(MemoryDevices::default());
        let device = mint_device(&PatternEntropy::new(99));
        devices.register(device.clone()).expect("a device");
        let host_public = keys.public;
        let shared = Shared { host: host.clone(), keys: Arc::new(keys), devices: devices.clone(), entropy: Arc::new(SystemEntropy), handshake_deadline: deadline };
        let running = transport::serve("127.0.0.1:0", shared).expect("loopback");
        let addr = running.addr();
        Harness { agent, host, devices, principals, device, host_public, running: Some(running), addr }
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
}

impl PhoneClient {
    pub fn connect(addr: SocketAddr, host_public: [u8; 32]) -> PhoneClient {
        let stream = TcpStream::connect(addr).expect("the server listens");
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let request = format!("ws://{addr}/").into_client_request().unwrap();
        let (ws, _) = tungstenite::client(request, stream).expect("a WebSocket upgrade");
        let keys = KeyPair::generate(&SystemEntropy);
        PhoneClient { ws, keys: None, secret: keys.secret, public: keys.public, host_public, entropy: SystemEntropy, next_id: 0, stashed: Default::default() }
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

    pub fn recv_sealed(&mut self) -> Result<Value, Closed> {
        let Message::Text(text) = self.recv_message()? else { panic!("expected a sealed text frame") };
        let plain = match self.keys.as_mut().expect("a hello came first") {
            Keys::V2 { from_host, session, inn, .. } => {
                let frame = b64::decode_canonical(text.as_str()).expect("canonical base64");
                let plain = v2_frame::open(from_host, session, Direction::DesktopToMobile, Kind::Text, *inn, &frame).expect("a frame this phone can open");
                *inn += 1;
                String::from_utf8(plain).expect("UTF-8")
            }
            Keys::Legacy(c) => c.open_text(text.as_str()).expect("a frame this phone can open"),
        };
        Ok(serde_json::from_str(&plain).expect("sealed JSON"))
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
