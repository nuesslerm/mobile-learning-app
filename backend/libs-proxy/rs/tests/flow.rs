//! Flow tests: the real router driven in-process over an in-memory bucket, the real OCI adapter against a loopback fake of the bucket,
//! and a real Cargo client resolving and building against the proxy on a loopback port.
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, Request, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::Router;
use http_body_util::BodyExt;
use libs_proxy::api::App;
use libs_proxy::infrastructure::config::Secret;
use libs_proxy::infrastructure::OciObjectRead;
use libs_proxy_core::testing::InMemoryObjects;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const BASE: &str = "http://libs-proxy:8793";

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

fn proxy(store: &Arc<InMemoryObjects>) -> Router {
    App::new(store.clone(), BASE, Some("abc123")).router()
}

async fn send(router: &Router, method: Method, path: &str, headers: &[(&str, &str)]) -> Reply {
    let mut request = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = router.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
    let (parts, body) = response.into_parts();
    Reply { status: parts.status, headers: parts.headers, body: body.collect().await.unwrap().to_bytes().to_vec() }
}

async fn get(router: &Router, path: &str) -> Reply {
    send(router, Method::GET, path, &[]).await
}

fn header_of(reply: &Reply, name: header::HeaderName) -> &str {
    reply.headers.get(name).map(|v| v.to_str().unwrap()).unwrap_or("")
}

async fn listen(router: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    addr
}

const INDEX_LINE: &str = "{\"name\":\"service-kit\",\"vers\":\"1.0.0\",\"deps\":[],\"cksum\":\"00\",\"features\":{},\"yanked\":false}\n";

fn seeded() -> Arc<InMemoryObjects> {
    let store = Arc::new(InMemoryObjects::default());
    store.put("libs/index/se/rv/service-kit", INDEX_LINE.as_bytes());
    store.put("libs/crates/service-kit/1.0.0.crate", b"\x1f\x8bcrate-bytes");
    store
}

#[tokio::test]
async fn given_a_traversal_a_bad_route_or_a_write_verb_when_requested_then_it_is_refused_and_the_bucket_is_never_read() {
    let store = seeded();
    let router = proxy(&store);
    // Given a positive control: the same router does serve a good path
    assert_eq!(get(&router, "/index/se/rv/service-kit").await.status, StatusCode::OK);
    let reads_before = store.reads().len();

    // When each hostile request arrives
    for path in ["/index/../config.json", "/dl/../../etc/passwd.crate", "/index//se/rv/service-kit", "/index/se%2Frv/service-kit", "/index/%2e%2e/x", "/dl/service-kit%2F../1.0.0.crate"] {
        let reply = get(&router, path).await;
        // Then the proxy itself answered 400 (the request arrived and was refused)
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{path}");
    }
    for path in ["/", "/libs/index/se/rv/service-kit", "/list", "/dl/service-kit/1.0.0", "/index/xx/yy/service-kit"] {
        assert_eq!(get(&router, path).await.status, StatusCode::NOT_FOUND, "{path}");
    }
    for method in [Method::POST, Method::PUT, Method::DELETE, Method::HEAD] {
        let reply = send(&router, method.clone(), "/index/se/rv/service-kit", &[]).await;
        assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(header_of(&reply, header::ALLOW), "GET");
    }
    // And nothing reached the bucket
    assert_eq!(store.reads().len(), reads_before);
}

#[tokio::test]
async fn given_a_published_crate_when_the_index_and_crate_are_requested_then_the_bytes_and_cache_headers_are_right() {
    let store = seeded();
    let router = proxy(&store);

    let index = get(&router, "/index/se/rv/service-kit").await;
    assert_eq!(index.status, StatusCode::OK);
    assert_eq!(index.body, INDEX_LINE.as_bytes());
    assert!(header_of(&index, header::CONTENT_TYPE).starts_with("text/plain"));
    assert_eq!(header_of(&index, header::CACHE_CONTROL), "public, max-age=60, must-revalidate");

    let krate = get(&router, "/dl/service-kit/1.0.0.crate").await;
    assert_eq!(krate.status, StatusCode::OK);
    assert_eq!(krate.body, b"\x1f\x8bcrate-bytes");
    assert_eq!(header_of(&krate, header::CONTENT_TYPE), "application/octet-stream");
    assert_eq!(header_of(&krate, header::CACHE_CONTROL), "public, max-age=31536000, immutable");
    assert_eq!(store.reads(), vec!["libs/index/se/rv/service-kit", "libs/crates/service-kit/1.0.0.crate"]);
}

#[tokio::test]
async fn given_a_client_holding_the_etag_when_it_revalidates_then_the_proxy_answers_304() {
    let router = proxy(&seeded());
    let first = get(&router, "/index/se/rv/service-kit").await;
    let etag = header_of(&first, header::ETAG).to_string();
    assert!(!etag.is_empty());

    let again = send(&router, Method::GET, "/index/se/rv/service-kit", &[("if-none-match", &etag)]).await;

    assert_eq!(again.status, StatusCode::NOT_MODIFIED);
    assert!(again.body.is_empty());
}

#[tokio::test]
async fn given_a_missing_object_or_a_failing_bucket_when_requested_then_404_and_502_without_detail() {
    let store = seeded();
    let router = proxy(&store);

    let missing = get(&router, "/dl/service-kit/9.9.9.crate").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(String::from_utf8_lossy(&missing.body).trim(), "not found");

    store.fail_reads();
    let failing = get(&router, "/index/se/rv/service-kit").await;
    assert_eq!(failing.status, StatusCode::BAD_GATEWAY);
    assert_eq!(String::from_utf8_lossy(&failing.body).trim(), "bad gateway");
}

#[tokio::test]
async fn given_no_config_in_the_bucket_when_config_json_is_requested_then_it_is_generated_with_the_proxy_download_template() {
    let store = Arc::new(InMemoryObjects::default());
    let router = App::new(store.clone(), "http://libs-proxy:8793/", None).router();

    let reply = get(&router, "/index/config.json").await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(header_of(&reply, header::CONTENT_TYPE), "application/json");
    let config: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(config, serde_json::json!({ "dl": "http://libs-proxy:8793/dl/{crate}/{version}.crate" }));
    assert!(store.reads().is_empty());
}

#[tokio::test]
async fn given_a_running_service_when_health_is_requested_then_it_says_ok_with_the_commit() {
    let store = Arc::new(InMemoryObjects::default());
    let reply = get(&proxy(&store), "/health").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&reply.body).unwrap(), serde_json::json!({ "status": "ok", "commit": "abc123" }));

    let unknown = get(&App::new(store, BASE, None).router(), "/health").await;
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&unknown.body).unwrap()["commit"], "unknown");
}

#[tokio::test]
async fn given_a_real_listener_when_a_raw_traversal_request_is_sent_then_the_service_answers_400() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let addr = listen(proxy(&seeded())).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();

    stream.write_all(b"GET /index/../config.json HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).await.unwrap();

    assert!(text.starts_with("HTTP/1.1 400"), "{text}");
}

#[derive(Clone, Default)]
struct FakeBucket {
    objects: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    seen: Arc<Mutex<Vec<(String, HeaderMap)>>>,
}

const TOKEN: &str = "PARTOKEN-s3cr3t";
const BUCKET_PATH: &str = "/p/PARTOKEN-s3cr3t/n/ns/b/libs/o/";

async fn fake_object(State(fake): State<FakeBucket>, method: Method, uri: Uri, headers: HeaderMap) -> axum::response::Response {
    fake.seen.lock().unwrap().push((format!("{method} {uri}"), headers));
    let Some(key) = uri.path().strip_prefix(BUCKET_PATH) else { return StatusCode::FORBIDDEN.into_response() };
    if key.ends_with("boom") {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("ORACLE-INTERNAL-DETAIL {TOKEN} 127.0.0.1")).into_response();
    }
    match fake.objects.lock().unwrap().get(key) {
        Some(bytes) => ([(header::ETAG, "\"v1\"")], bytes.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "ObjectNotFound").into_response(),
    }
}

#[tokio::test]
async fn given_the_oci_adapter_over_a_fake_bucket_when_clients_read_then_the_read_link_is_used_upstream_and_never_shown() {
    // Given a fake bucket holding objects behind a read link, and the real adapter pointed at it
    let fake = FakeBucket::default();
    fake.objects.lock().unwrap().insert("libs/crates/service-kit/1.0.0.crate".into(), b"crate-bytes".to_vec());
    fake.objects.lock().unwrap().insert("libs/index/se/rv/service-kit".into(), INDEX_LINE.as_bytes().to_vec());
    let bucket = listen(Router::new().fallback(fake_object).with_state(fake.clone())).await;
    let par = Secret::new(format!("http://{bucket}{BUCKET_PATH}"));
    let app = App::new(Arc::new(OciObjectRead::new(&par).unwrap()), BASE, None).router();

    // When a crate, an index file, a missing crate and a failing object are read through the proxy
    let ok = get(&app, "/dl/service-kit/1.0.0.crate").await;
    let index = get(&app, "/index/se/rv/service-kit").await;
    let missing = get(&app, "/dl/service-kit/2.0.0.crate").await;
    let failing = get(&app, "/index/bo/om/boom").await;
    let revalidated = send(&app, Method::GET, "/index/se/rv/service-kit", &[("if-none-match", "\"v1\"")]).await;

    // Then the bucket was asked for exactly those objects under the read link
    let seen: Vec<String> = fake.seen.lock().unwrap().iter().map(|(line, _)| line.clone()).collect();
    assert_eq!(
        seen,
        [
            format!("GET {BUCKET_PATH}libs/crates/service-kit/1.0.0.crate"),
            format!("GET {BUCKET_PATH}libs/index/se/rv/service-kit"),
            format!("GET {BUCKET_PATH}libs/crates/service-kit/2.0.0.crate"),
            format!("GET {BUCKET_PATH}libs/index/bo/om/boom"),
            format!("GET {BUCKET_PATH}libs/index/se/rv/service-kit"),
        ]
    );
    assert_eq!(fake.seen.lock().unwrap()[4].1.get(header::IF_NONE_MATCH).unwrap(), "\"v1\"");
    // And the statuses and bytes are the object's, with the failure reduced to 502
    assert_eq!((ok.status, ok.body.as_slice()), (StatusCode::OK, b"crate-bytes".as_slice()));
    assert_eq!(index.body, INDEX_LINE.as_bytes());
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(failing.status, StatusCode::BAD_GATEWAY);
    assert_eq!(header_of(&ok, header::ETAG), "\"v1\"");
    assert_eq!(revalidated.status, StatusCode::OK);
    // And no response body or header carries the link, the upstream address, or the upstream's own error text
    let forbidden = [TOKEN, "127.0.0.1", "ORACLE-INTERNAL-DETAIL", "ObjectNotFound", &bucket.port().to_string(), "/p/", "libs/"];
    for reply in [&ok, &index, &missing, &failing, &revalidated] {
        let mut everything = String::from_utf8_lossy(&reply.body).into_owned();
        for (name, value) in &reply.headers {
            everything.push_str(&format!("\n{name}: {}", value.to_str().unwrap_or("")));
        }
        for needle in forbidden {
            assert!(!everything.contains(needle), "{needle} leaked in {everything}");
        }
    }
}

#[tokio::test]
async fn given_the_oci_adapter_when_a_refused_request_arrives_then_the_bucket_sees_nothing_and_the_refusal_came_from_the_proxy() {
    let fake = FakeBucket::default();
    let bucket = listen(Router::new().fallback(fake_object).with_state(fake.clone())).await;
    let par = Secret::new(format!("http://{bucket}{BUCKET_PATH}"));
    let proxy_addr = listen(App::new(Arc::new(OciObjectRead::new(&par).unwrap()), BASE, None).router()).await;
    let client = reqwest::Client::new();

    // When a write verb and a listing-style request go to the real proxy listener
    let post = client.post(format!("http://{proxy_addr}/index/se/rv/service-kit")).send().await.unwrap();
    let list = client.get(format!("http://{proxy_addr}/?prefix=libs/")).send().await.unwrap();
    let health = client.get(format!("http://{proxy_addr}/health")).send().await.unwrap();

    // Then the proxy answered each (405, 404, 200: so the requests arrived) and the bucket was never contacted
    assert_eq!((post.status().as_u16(), list.status().as_u16(), health.status().as_u16()), (405, 404, 200));
    assert!(fake.seen.lock().unwrap().is_empty());
}

fn crate_fixture(root: &std::path::Path) -> (Vec<u8>, String) {
    let lib = root.join("tinylib");
    std::fs::create_dir_all(lib.join("src")).unwrap();
    std::fs::write(lib.join("Cargo.toml"), "[package]\nname = \"tinylib\"\nversion = \"1.0.0\"\nedition = \"2021\"\ndescription = \"fixture\"\nlicense = \"MIT\"\n").unwrap();
    std::fs::write(lib.join("src/lib.rs"), "pub fn greeting() -> &'static str {\n    \"hello through the proxy\"\n}\n").unwrap();
    let status = std::process::Command::new("cargo")
        .args(["package", "--offline", "--no-verify", "--allow-dirty", "--quiet"])
        .current_dir(&lib)
        .env("CARGO_HOME", root.join("pkg-home"))
        .env("CARGO_TARGET_DIR", root.join("pkg-target"))
        .status()
        .unwrap();
    assert!(status.success(), "cargo package failed");
    let bytes = std::fs::read(root.join("pkg-target/package/tinylib-1.0.0.crate")).unwrap();
    let cksum = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    (bytes, cksum)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_crate_published_to_the_store_when_real_cargo_builds_a_consumer_then_it_resolves_downloads_and_runs_through_the_proxy() {
    // Given a tiny crate's sparse index line and .crate in the store, and the proxy on a loopback port that knows its own address
    let root = tempfile::tempdir().unwrap();
    let (bytes, cksum) = crate_fixture(root.path());
    let store = Arc::new(InMemoryObjects::default());
    store.put("libs/crates/tinylib/1.0.0.crate", &bytes);
    store.put("libs/index/ti/ny/tinylib", format!("{{\"name\":\"tinylib\",\"vers\":\"1.0.0\",\"deps\":[],\"cksum\":\"{cksum}\",\"features\":{{}},\"yanked\":false}}\n").as_bytes());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = App::new(store.clone(), &base, None).router();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    // And a consumer that names the proxy as its registry
    let consumer = root.path().join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    std::fs::create_dir_all(consumer.join(".cargo")).unwrap();
    std::fs::write(consumer.join("Cargo.toml"), "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ntinylib = { version = \"1\", registry = \"mla\" }\n").unwrap();
    std::fs::write(consumer.join(".cargo/config.toml"), format!("[registries.mla]\nindex = \"sparse+{base}/index/\"\n")).unwrap();
    std::fs::write(consumer.join("src/main.rs"), "fn main() {\n    println!(\"{}\", tinylib::greeting());\n}\n").unwrap();

    // When real Cargo resolves, downloads, builds and runs it
    let output = tokio::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&consumer)
        .env("CARGO_HOME", root.path().join("home"))
        .env("CARGO_TARGET_DIR", root.path().join("target"))
        .env_remove("CARGO_NET_OFFLINE")
        .env_remove("http_proxy")
        .env_remove("HTTP_PROXY")
        .env_remove("all_proxy")
        .env_remove("ALL_PROXY")
        .output()
        .await
        .unwrap();

    // Then the program ran the downloaded crate, the lockfile names the proxy as the source, and Cargo read config, index and crate from it
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "cargo failed: {stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello through the proxy");
    let lock = std::fs::read_to_string(consumer.join("Cargo.lock")).unwrap();
    assert!(lock.contains(&format!("sparse+{base}/index/")), "{lock}");
    assert!(lock.contains(&cksum), "{lock}");
    assert_eq!(store.reads(), vec!["libs/index/ti/ny/tinylib", "libs/crates/tinylib/1.0.0.crate"]);
}
