//! The Nx shim's protocol over a directory store: what it answers, what it sends the store, and that a failing, read-only or slow
//! store never turns into anything but a 200 or a 404 (real Nx fails its run on any other status: `clis/pipeline/tests/flow/nx_cache.rs`).
use blob_cache::adapters::DirStore;
use blob_cache::domain::{Budget, Generation, Key, Kind};
use blob_cache::ports::{BlobStore, PutOutcome, StoreError};
use blob_cache::{Cache, NxServer, Writes};
use cli_http::{Request, Via};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A store that counts what it is asked, and how many asks are in flight at once.
struct Spy<S> {
    inner: S,
    delay: Duration,
    gets: AtomicU64,
    puts: AtomicU64,
    /// Of the puts, those that asked the store to keep the first writer's body.
    first_wins: AtomicU64,
    exists: AtomicU64,
    inflight: AtomicU64,
    peak: AtomicU64,
}

impl<S> Spy<S> {
    fn new(inner: S, delay: Duration) -> Arc<Spy<S>> {
        Arc::new(Spy { inner, delay, gets: AtomicU64::new(0), puts: AtomicU64::new(0), first_wins: AtomicU64::new(0), exists: AtomicU64::new(0), inflight: AtomicU64::new(0), peak: AtomicU64::new(0) })
    }

    fn count(&self, counter: &AtomicU64) -> u64 {
        counter.load(Ordering::SeqCst)
    }
}

impl<S: BlobStore> BlobStore for Spy<S> {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        let now = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(self.delay);
        let found = self.inner.get(key);
        self.inflight.fetch_sub(1, Ordering::SeqCst);
        found
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        self.first_wins.fetch_add(u64::from(if_absent), Ordering::SeqCst);
        self.inner.put(key, body, if_absent)
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        self.exists.fetch_add(1, Ordering::SeqCst);
        self.inner.exists(key)
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("blob-cache-nx-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn nx_key(name: &str) -> Key {
    Key::new(Kind::Nx, Generation(0), name).unwrap()
}

fn budget(max_nx_entry: u64) -> Budget {
    let mut json: serde_json::Value = serde_json::from_str(include_str!("../data/budget.json")).unwrap();
    json["kinds"]["nx"]["maxEntryBytes"] = max_nx_entry.into();
    Budget::parse(&json.to_string()).unwrap()
}

/// One request as Nx sends it; the status and body, and every status seen goes to `seen`.
fn call(server: &NxServer, seen: &std::sync::Mutex<Vec<u16>>, method: &str, path: &str, token: &str, body: &[u8]) -> (u16, Vec<u8>, Option<String>) {
    let response = Request::new(method, &format!("{}{path}", server.url())).via(Via::Direct).header("Authorization", &format!("Bearer {token}")).body(body).send().expect("the shim answers");
    seen.lock().unwrap().push(response.status);
    let kind = response.header("content-type").map(str::to_string);
    (response.status, response.body, kind)
}

/// One request as a keep-alive client sends it: the answer's status and length.
fn on_one_connection(server: &NxServer, hash: &str) -> (u16, usize) {
    let mut stream = TcpStream::connect(server.url().trim_start_matches("http://")).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(stream, "GET /v1/cache/{hash} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {}\r\n\r\n", server.token()).unwrap();
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).unwrap();
    let text = String::from_utf8_lossy(&answer);
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    (head.split(' ').nth(1).unwrap().parse().unwrap(), body.len())
}

#[test]
fn given_nx_asks_then_stores_when_the_shim_answers_then_it_sends_200_or_404_and_the_first_writer_wins() {
    // Given a shim over a directory store that holds entries of at most 8 bytes
    let dir = scratch("protocol");
    let spy = Spy::new(DirStore::new(dir.join("store")), Duration::ZERO);
    let cache = Arc::new(Cache::with_generation(spy.clone(), budget(8), Writes::Allowed, Generation(0)));
    let server = NxServer::start(cache).unwrap();
    let (seen, token) = (std::sync::Mutex::new(Vec::new()), server.token().to_string());
    let nx = |method: &str, hash: &str, body: &[u8]| call(&server, &seen, method, &format!("/v1/cache/{hash}"), &token, body);

    // When an entry is asked for and absent, then stored right after that miss
    assert_eq!(nx("GET", "100", b"").0, 404);
    assert_eq!(nx("PUT", "100", b"first").0, 200);
    // Then the store was written once and told not to check for an earlier writer, and the entry is served as bytes
    assert_eq!((spy.count(&spy.puts), spy.count(&spy.first_wins)), (1, 0));
    assert_eq!(nx("GET", "100", b""), (200, b"first".to_vec(), Some("application/octet-stream".into())));

    // When an entry nobody asked for is stored twice, Then the store is told the first writer wins, and the first body stays
    assert_eq!((nx("PUT", "200", b"one").0, nx("PUT", "200", b"two").0), (200, 200));
    assert_eq!((spy.count(&spy.first_wins), nx("GET", "200", b"").1), (2, b"one".to_vec()));

    // When an entry is over its kind's size, Then the body is dropped with a 200 and the connection still serves the next call
    assert_eq!(nx("PUT", "300", b"123456789").0, 200);
    assert_eq!((DirStore::new(dir.join("store")).exists(&nx_key("300")), nx("GET", "100", b"").0), (Ok(false), 200));

    // When a caller without the token, a path that is not a cache entry or another method calls, Then it is a 404 and nothing is stored
    assert_eq!(call(&server, &seen, "GET", "/v1/cache/100", "wrong", b"").0, 404);
    assert_eq!(call(&server, &seen, "PUT", "/v1/cache/400", "wrong", b"stolen").0, 404);
    assert_eq!([call(&server, &seen, "GET", "/v1/cache/", &token, b"").0, call(&server, &seen, "GET", "/", &token, b"").0, call(&server, &seen, "DELETE", "/v1/cache/100", &token, b"").0], [404, 404, 404]);
    assert_eq!(DirStore::new(dir.join("store")).exists(&nx_key("400")), Ok(false));

    // And no status but 200 and 404 was ever sent, a first call down a connection a keep-alive client would reuse included
    assert_eq!(on_one_connection(&server, "100"), (200, 5));
    assert!(seen.lock().unwrap().iter().all(|s| [200, 404].contains(s)), "{:?}", seen.lock().unwrap());
    server.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn given_a_dead_read_only_or_slow_store_when_nx_keeps_asking_then_every_answer_is_200_or_404_and_a_cold_run_fetches_in_parallel() {
    let dir = scratch("degraded");
    std::fs::write(dir.join("plain-file"), b"x").unwrap();
    let seen = std::sync::Mutex::new(Vec::new());

    // Given a store that cannot be reached
    let dead = Spy::new(DirStore::new(dir.join("plain-file")), Duration::ZERO);
    let cache = Arc::new(Cache::with_generation(dead.clone(), Budget::committed().clone(), Writes::Allowed, Generation(0)));
    let server = NxServer::start(cache.clone()).unwrap();
    // When Nx reads and writes, Then it is told 404 and 200 as if the cache were empty, and the store is asked once, not again
    let token = server.token().to_string();
    let answers: Vec<u16> = [("GET", "1"), ("GET", "2"), ("PUT", "1"), ("PUT", "3")].iter().map(|(m, h)| call(&server, &seen, m, &format!("/v1/cache/{h}"), &token, b"entry").0).collect();
    assert_eq!(answers, [404, 404, 200, 200]);
    assert_eq!((dead.count(&dead.gets), dead.count(&dead.puts), dead.count(&dead.exists)), (1, 0, 0));
    assert_eq!(cache.degraded().map(|d| d.as_str()), Some("unreachable"));
    server.stop();

    // Given a read-only run
    let store = Spy::new(DirStore::new(dir.join("store")), Duration::ZERO);
    let read_only = Arc::new(Cache::with_generation(store.clone(), Budget::committed().clone(), Writes::ReadOnly("dirty: x".into()), Generation(0)));
    let server = NxServer::start(read_only).unwrap();
    let token = server.token().to_string();
    // When Nx stores three entries, Then each is answered 200, the shim received all three and the store was sent none
    let answers: Vec<u16> = (0..3).map(|i| call(&server, &seen, "PUT", &format!("/v1/cache/ro{i}"), &token, b"entry").0).collect();
    assert_eq!((answers, server.served().puts, store.count(&store.puts), store.count(&store.exists)), (vec![200, 200, 200], 3, 0, 0));
    server.stop();

    // Given a slow store holding 91 entries, the size of a cold sandbox's replay
    let slow_dir = DirStore::new(dir.join("slow"));
    (0..91).for_each(|i| assert_eq!(slow_dir.put(&nx_key(&format!("t{i}")), format!("result {i}").as_bytes(), true), Ok(PutOutcome::Stored)));
    let slow = Spy::new(DirStore::new(dir.join("slow")), Duration::from_millis(50));
    let server = Arc::new(NxServer::start(Arc::new(Cache::with_generation(slow.clone(), Budget::committed().clone(), Writes::Allowed, Generation(0)))).unwrap());
    // When Nx asks for all of them at once, Then every one is served and the store was asked many at a time
    let seen = Arc::new(seen);
    let clients: Vec<_> = (0..91)
        .map(|i| {
            let (server, seen) = (server.clone(), seen.clone());
            std::thread::spawn(move || call(&server, &seen, "GET", &format!("/v1/cache/t{i}"), server.token(), b"").1 == format!("result {i}").as_bytes())
        })
        .collect();
    assert!(clients.into_iter().all(|c| c.join().unwrap()));
    assert!(slow.count(&slow.peak) >= 8, "peak {}", slow.count(&slow.peak));

    // And nothing in any of it was answered but 200 or 404
    assert!(seen.lock().unwrap().iter().all(|s| [200, 404].contains(s)), "{:?}", seen.lock().unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn given_many_idle_connections_when_a_real_request_arrives_then_it_is_answered_at_once() {
    // Given a shim and 32 connections that were opened and never send a byte, as Nx's pre-opened sockets do
    let store = Spy::new(DirStore::new(scratch("idle").join("store")), Duration::ZERO);
    let server = NxServer::start(Arc::new(Cache::with_generation(store, budget(8), Writes::Allowed, Generation(0)))).unwrap();
    let seen = std::sync::Mutex::new(Vec::new());
    let warm: Vec<u16> = std::thread::scope(|s| {
        let calls: Vec<_> = (0..8).map(|_| s.spawn(|| call(&server, &seen, "GET", "/v1/cache/warm", server.token(), b"").0)).collect();
        calls.into_iter().map(|c| c.join().unwrap()).collect()
    });
    assert_eq!(warm, [404; 8]);
    let idle: Vec<TcpStream> = (0..32).map(|_| TcpStream::connect(server.url().trim_start_matches("http://")).unwrap()).collect();

    // When a request comes on a fresh connection
    let started = std::time::Instant::now();
    let status = call(&server, &std::sync::Mutex::new(Vec::new()), "GET", "/v1/cache/absent", server.token(), b"").0;

    // Then it is answered within a second, whatever the idle ones hold
    assert_eq!(status, 404);
    assert!(started.elapsed() < Duration::from_secs(1), "answered after {:?}", started.elapsed());
    drop(idle);
    server.stop();
}

#[test]
fn given_a_served_request_when_the_answer_is_read_then_the_connection_is_closed() {
    // Given a shim and a request sent on a fresh connection
    let store = Spy::new(DirStore::new(scratch("eof").join("store")), Duration::ZERO);
    let server = NxServer::start(Arc::new(Cache::with_generation(store, budget(8), Writes::Allowed, Generation(0)))).unwrap();
    let mut stream = TcpStream::connect(server.url().trim_start_matches("http://")).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    write!(stream, "GET /v1/cache/x HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {}\r\n\r\n", server.token()).unwrap();

    // When everything the shim sends is read, Then the shim ends the connection itself, so no client holds it idle
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("the shim closes after its answer");
    assert!(answer.starts_with("HTTP/1.1 404") && answer.to_ascii_lowercase().contains("connection: close"), "{answer}");
    server.stop();
}
