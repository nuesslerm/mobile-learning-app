//! The sccache shim's WebDAV protocol over a directory store: what a compile's object round trip looks like, what the start-up
//! check and directory calls get, and that a request without the token, or an entry over its cap, never reaches the store.
use blob_cache::adapters::DirStore;
use blob_cache::domain::{Budget, Generation, Key, Kind};
use blob_cache::ports::BlobStore;
use blob_cache::{Cache, SccacheServer, Writes};
use cli_http::{Request, Via};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;

const HASH: &str = "05241167ad9f0b1887d5a7c0be66ed7c8d231b54b28f3bed4e0e7a6de4befcc8";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("blob-cache-sccache-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn budget(max_entry: u64) -> Budget {
    let mut json: serde_json::Value = serde_json::from_str(include_str!("../data/budget.json")).unwrap();
    json["kinds"]["sccache"]["maxEntryBytes"] = max_entry.into();
    Budget::parse(&json.to_string()).unwrap()
}

fn up(dir: &std::path::Path, max_entry: u64) -> (SccacheServer, Arc<DirStore>) {
    let store = Arc::new(DirStore::new(dir.join("store")));
    let cache = Arc::new(Cache::with_generation(store.clone(), budget(max_entry), Writes::Allowed, Generation(0)));
    (SccacheServer::start(cache).expect("the server listens"), store)
}

fn call(server: &SccacheServer, method: &str, path: &str, token: Option<&str>, body: &[u8]) -> (u16, Vec<u8>) {
    let mut request = Request::new(method, &format!("{}{path}", server.url())).via(Via::Direct);
    if let Some(token) = token {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let response = request.body(body).send().expect("the shim answers");
    (response.status, response.body)
}

/// A WebDAV request the HTTP client refuses to build (`PROPFIND`, `MKCOL`), sent by hand: the status and the body.
fn raw(server: &SccacheServer, method: &str, path: &str, token: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(server.url().trim_start_matches("http://")).unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    let status = text.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, text)
}

#[test]
fn a_compiles_object_round_trips_under_its_hash_and_is_stored_in_the_sccache_namespace() {
    // Given the shim over an empty store
    let dir = scratch("round-trip");
    let (server, store) = up(&dir, 1 << 20);
    let path = format!("/0/5/2/{HASH}");

    // When sccache misses, writes the object into its nested path, and reads it back
    let miss = call(&server, "GET", &path, Some(server.token()), b"");
    let put = call(&server, "PUT", &path, Some(server.token()), b"rlib bytes");
    let hit = call(&server, "GET", &path, Some(server.token()), b"");

    // Then the first read is a 404, the write 201, the second read the body, and the store holds it under sccache/<hash> only
    assert_eq!((miss.0, put.0, hit.0, hit.1.as_slice()), (404, 201, 200, &b"rlib bytes"[..]));
    let key = Key::new(Kind::Sccache, Generation(0), HASH).unwrap();
    assert_eq!(key.as_str(), format!("sccache/g0/{HASH}"));
    assert_eq!(store.get(&key).unwrap(), Some(b"rlib bytes".to_vec()));
    assert_eq!(server.served().gets, 2);
    assert_eq!(server.served().puts, 1);
}

#[test]
fn the_start_up_check_and_the_directory_calls_are_answered_without_touching_the_store() {
    // Given the shim
    let dir = scratch("check");
    let (server, store) = up(&dir, 1 << 20);
    let token = Some(server.token());

    // When sccache runs its storage check (read, write, directory listing) and creates a directory
    let read = call(&server, "GET", "/.sccache_check", token, b"");
    let write = call(&server, "PUT", "/.sccache_check", token, b"x");
    let listing = raw(&server, "PROPFIND", "/0/5/2/", server.token());
    let made = raw(&server, "MKCOL", "/0/5/", server.token());

    // Then the read is a 404, the write a 201 that stores nothing, the listing a 207 that says directory, and the directory a 201
    assert_eq!((read.0, write.0, listing.0, made.0), (404, 201, 207, 201));
    assert!(listing.1.contains("<D:collection/>"));
    assert_eq!(server.served().puts, 0);
    assert!(store.get(&Key::new(Kind::Sccache, Generation(0), HASH).unwrap()).unwrap().is_none());
}

#[test]
fn a_request_without_the_token_is_refused_and_an_entry_over_its_cap_is_dropped() {
    // Given the shim with a 4-byte entry cap
    let dir = scratch("refuse");
    let (server, store) = up(&dir, 4);
    let path = format!("/a/b/c/{HASH}");

    // When a write arrives with no token, with a wrong one, and a right one carrying 5 bytes
    let none = call(&server, "PUT", &path, None, b"abc");
    let wrong = call(&server, "PUT", &path, Some("nope"), b"abc");
    let big = call(&server, "PUT", &path, Some(server.token()), b"12345");

    // Then the first two are 401, the third is acknowledged but the store never holds it
    assert_eq!((none.0, wrong.0, big.0), (401, 401, 201));
    assert!(store.get(&Key::new(Kind::Sccache, Generation(0), HASH).unwrap()).unwrap().is_none());
    assert_eq!(server.served().puts, 0);
}
