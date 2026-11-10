//! The world around the router: the clock and the event log are doubles, requests enter through the router `main` serves, and
//! every request is signed the way `wf` signs it (a rotating token plus an Ed25519 signature over the request).
use axum::body::Body;
use axum::http::{Request, Uri};
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::pkcs8::EncodePublicKey;
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower::ServiceExt;
use vm_ops_api::api::{self, Limits, Ports, Settings};
use vm_ops_api_core::application::health::Build;
use vm_ops_api_core::application::ports::EventLog;
use vm_ops_api_core::application::scripts::redeploy::RedeployEnv;
use vm_ops_api_core::domain::auth::{derive_rotating_token, AuthConfig};
use vm_ops_api_core::testing::{FakeClock, FakeContainers, FakeCooldownLedger, FakeEventLog, FakeHostFiles, FakeIds, FakeRunLog, FakeSiblings};

pub const SEED: &str = "seed-for-tests";
pub const CI_SEED: &str = "ci-seed-for-tests";
/// 2026-10-04T12:00:00Z
pub const NOON: i64 = 1_791_115_200_000;
pub const DAY: i64 = 86_400_000;

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }
}

pub struct World {
    pub router: Router,
    pub ci_router: Router,
    pub clock: Arc<FakeClock>,
    pub log: Arc<FakeEventLog>,
    pub containers: Arc<FakeContainers>,
    pub run_log: Arc<FakeRunLog>,
    pub siblings: Arc<FakeSiblings>,
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

pub fn build() -> Build {
    Build { commit: "0123456789abcdef".into(), host: "main".into(), debug_mode: false, server_script_sha: "0123456789ab".into(), image_tag: "sha-1234567".into() }
}

/// What the service reads and writes outside itself, scripted before it starts.
#[derive(Default)]
pub struct Doubles {
    pub containers: FakeContainers,
    pub host_files: FakeHostFiles,
    pub cooldowns: FakeCooldownLedger,
    pub siblings: FakeSiblings,
    /// Every sleep of the clock moves the time on.
    pub advancing: bool,
    /// Where the service sends its events instead of the double: the real sinks, for what the sinks themselves are asked.
    pub log: Option<Arc<dyn EventLog>>,
}

pub const DASHBOARD: &[u8] = b"<!doctype html><title>opencode runs</title>\n";

pub fn world(require_signed_auth: bool) -> World {
    world_with(require_signed_auth, Doubles::default())
}

pub fn world_with(require_signed_auth: bool, doubles: Doubles) -> World {
    let clock = Arc::new(if doubles.advancing { FakeClock::at(NOON).advancing() } else { FakeClock::at(NOON) });
    let log = Arc::new(FakeEventLog::default());
    let (containers, run_log, siblings) = (Arc::new(doubles.containers), Arc::new(FakeRunLog::default()), Arc::new(doubles.siblings));
    let pem = key().verifying_key().to_public_key_pem(LineEnding::LF).expect("the test key encodes");
    let auth = AuthConfig::new(SecretString::from(SEED), &pem, require_signed_auth);
    let ports = Ports {
        clock: clock.clone(),
        log: doubles.log.unwrap_or_else(|| log.clone()),
        containers: containers.clone(),
        host_files: Arc::new(doubles.host_files),
        cooldowns: Arc::new(doubles.cooldowns),
        run_log: run_log.clone(),
        ids: Arc::new(FakeIds::default()),
        siblings: siblings.clone(),
    };
    let settings = Settings {
        auth,
        ci_auth: Some(AuthConfig::new(SecretString::from(CI_SEED), &pem, true)),
        build: build(),
        limits: Limits::NODE,
        dashboard: DASHBOARD,
        sibling_image: "ns/vm-ops-api-rs:sha-1234567".into(),
        redeploy: RedeployEnv { host_label: "main".into(), image_namespace: "ns".into(), image_tag: Some("sha-1234567".into()), provision_token: "tok".into(), vm_dir: "/home/ubuntu/vm-ops-api".into(), ..RedeployEnv::default() },
    };
    let routers = api::routers(ports, settings);
    World { router: routers.admin, ci_router: routers.ci.expect("the settings carry CI credentials"), clock, log, containers, run_log, siblings }
}

/// The signature over `METHOD\n<signed_path>\n<timestamp>\n<sha256 of body>`, in standard base64.
pub fn signature(method: &str, signed_path: &str, timestamp: &str, body: &[u8]) -> String {
    let canonical = format!("{method}\n{signed_path}\n{timestamp}\n{}", hex::encode(Sha256::digest(body)));
    STANDARD.encode(key().sign(canonical.as_bytes()).to_bytes())
}

/// The headers `wf` sends: today's rotating token, the signature over `signed_path` and the time.
pub fn signed_headers(method: &str, signed_path: &str, body: &[u8], now_ms: i64, token_date: &str) -> Vec<(String, String)> {
    signed_headers_with(SEED, method, signed_path, body, now_ms, token_date)
}

/// As `signed_headers`, with the rotating token derived from `seed`.
pub fn signed_headers_with(seed: &str, method: &str, signed_path: &str, body: &[u8], now_ms: i64, token_date: &str) -> Vec<(String, String)> {
    let timestamp = now_ms.to_string();
    vec![
        ("authorization".into(), format!("Bearer {}", derive_rotating_token(seed, token_date))),
        ("x-signature".into(), signature(method, signed_path, &timestamp, body)),
        ("x-signature-timestamp".into(), timestamp),
    ]
}

pub fn legacy_headers() -> Vec<(String, String)> {
    vec![("authorization".into(), format!("Bearer {SEED}"))]
}

impl World {
    /// A request with exactly these headers, to the raw `target` as it stands on the request line.
    pub async fn send(&self, method: &str, target: &str, headers: &[(String, String)], body: &[u8]) -> Reply {
        self.send_on(&self.router, method, target, headers, body).await
    }

    pub async fn send_on(&self, router: &Router, method: &str, target: &str, headers: &[(String, String)], body: &[u8]) -> Reply {
        let uri: Uri = target.parse().unwrap_or_else(|e| panic!("{target}: {e}"));
        let mut request = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = router.clone().oneshot(request.body(Body::from(body.to_vec())).expect("a request")).await.expect("the router answers");
        let status = response.status().as_u16();
        let headers = response.headers().iter().map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).to_string())).collect();
        let body = String::from_utf8_lossy(&response.into_body().collect().await.expect("a body").to_bytes()).to_string();
        Reply { status, headers, body }
    }

    /// A request signed over its own pathname and search, as `wf` signs `/health`.
    pub async fn signed(&self, method: &str, target: &str, body: &[u8]) -> Reply {
        let headers = signed_headers(method, target, body, self.clock_now(), &today());
        self.send(method, target, &headers, body).await
    }

    /// A request signed with the CI seed, to the CI port.
    pub async fn ci_signed(&self, method: &str, target: &str, body: &[u8]) -> Reply {
        let headers = signed_headers_with(CI_SEED, method, target, body, self.clock_now(), &today());
        self.send_on(&self.ci_router, method, target, &headers, body).await
    }

    /// A request signed with the CI seed, to the admin port.
    pub async fn ci_signed_on_admin(&self, method: &str, target: &str, body: &[u8]) -> Reply {
        let headers = signed_headers_with(CI_SEED, method, target, body, self.clock_now(), &today());
        self.send(method, target, &headers, body).await
    }

    /// A request signed with the admin seed, to the CI port.
    pub async fn admin_signed_on_ci(&self, method: &str, target: &str, body: &[u8]) -> Reply {
        let headers = signed_headers(method, target, body, self.clock_now(), &today());
        self.send_on(&self.ci_router, method, target, &headers, body).await
    }

    fn clock_now(&self) -> i64 {
        use vm_ops_api_core::application::ports::Clock;
        self.clock.now_ms()
    }
}

pub fn today() -> String {
    vm_ops_api_core::domain::js::utc_date(NOON)
}
