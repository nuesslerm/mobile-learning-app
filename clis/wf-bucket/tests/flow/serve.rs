//! `wf bucket serve` through the built binary: a directory served with bearer tokens, driven over loopback HTTP. The PAR adapter
//! sends no Authorization header, so the cases with a token go through `cli-http` and the adapter shows what it gets today.
use crate::support::{spawn_wf, wait_until, RunOpts, Spawned};
use blob_cache::adapters::OciParStore;
use blob_cache::domain::Key;
use blob_cache::ports::{BlobStore, StoreError};
use cli_http::{Request, Response};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

const READ: &str = "read-token-for-the-test";
const WRITE: &str = "write-token-for-the-test";

struct Served {
    _process: Spawned,
    dir: tempfile::TempDir,
    url: String,
    ready: Value,
}

impl Served {
    fn start(extra: &[&str]) -> Served {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (objects, tokens) = (dir.path().join("objects"), dir.path());
        std::fs::write(tokens.join("read"), format!("{READ}\n")).expect("a token file");
        std::fs::write(tokens.join("write"), format!("{WRITE}\n")).expect("a token file");
        let mut args = vec!["bucket", "serve", "--dir", objects.to_str().unwrap(), "--listen", "127.0.0.1:0"];
        let (read, write) = (tokens.join("read"), tokens.join("write"));
        args.extend(["--read-token-file", read.to_str().unwrap(), "--write-token-file", write.to_str().unwrap()]);
        args.extend(extra);
        let process = spawn_wf(&args, &RunOpts::default());
        wait_until(Duration::from_secs(10), "the ready line", || process.out().contains('\n'));
        let ready: Value = serde_json::from_str(process.out().lines().next().unwrap()).expect("one JSON line");
        let url = format!("http://{}", ready["listen"].as_str().expect("listen"));
        Served { _process: process, dir, url, ready }
    }

    fn objects(&self) -> std::path::PathBuf {
        self.dir.path().join("objects")
    }

    fn send(&self, method: &str, path: &str, token: Option<&str>, headers: &[(&str, &str)], body: &[u8]) -> Response {
        let mut request = Request::new(method, &format!("{}{path}", self.url)).direct().timeout(Duration::from_secs(10));
        if let Some(token) = token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        for (name, value) in headers {
            request = request.header(name, value);
        }
        if !body.is_empty() {
            request = request.body(body);
        }
        request.send().expect("the server answers")
    }
}

fn files_under(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path.strip_prefix(dir).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    found
}

#[test]
fn given_a_server_when_it_is_ready_then_one_json_line_names_the_bound_address_and_directory() {
    // Given a server started on port 0
    let served = Served::start(&[]);
    // Then the line says ok, a real port and the directory, and no token
    assert_eq!(served.ready["ok"], true);
    assert_ne!(served.ready["listen"].as_str().unwrap().rsplit(':').next(), Some("0"));
    assert_eq!(served.ready["dir"], served.objects().to_str().unwrap());
    assert!(!served._process.out().contains(WRITE) && !served._process.out().contains(READ));
}

#[test]
fn given_the_write_token_when_an_object_is_put_then_both_tokens_read_it_back_and_head_gives_its_size() {
    // Given a server
    let served = Served::start(&[]);
    // When the write token puts an object under a nested key
    let put = served.send("PUT", "/o/nx/g0/abc.tar", Some(WRITE), &[], b"hello blob");
    // Then it is stored, and the read token and the write token both get the bytes
    assert_eq!(put.status, 200);
    assert_eq!(std::fs::read(served.objects().join("nx/g0/abc.tar")).unwrap(), b"hello blob");
    for token in [READ, WRITE] {
        let got = served.send("GET", "/o/nx/g0/abc.tar", Some(token), &[], b"");
        assert_eq!((got.status, got.body.as_slice()), (200, b"hello blob".as_slice()));
    }
    // And HEAD answers the length with no body, and a missing key is 404
    let head = served.send("HEAD", "/o/nx/g0/abc.tar", Some(READ), &[], b"");
    assert_eq!((head.status, head.header("content-length"), head.body.len()), (200, Some("10"), 0));
    assert_eq!(served.send("GET", "/o/nx/g0/none", Some(READ), &[], b"").status, 404);
    // And no temporary file is left beside the object
    assert_eq!(files_under(&served.objects()), vec!["nx/g0/abc.tar".to_string()]);
}

#[test]
fn given_no_token_a_wrong_token_or_the_read_token_when_a_request_comes_then_it_is_401() {
    // Given a server that holds an object
    let served = Served::start(&[]);
    served.send("PUT", "/o/a", Some(WRITE), &[], b"x");
    // Then GET and HEAD with no or a wrong token are 401, and so is PUT with the read token
    assert_eq!(served.send("GET", "/o/a", None, &[], b"").status, 401);
    assert_eq!(served.send("GET", "/o/a", Some("wrong"), &[], b"").status, 401);
    assert_eq!(served.send("HEAD", "/o/a", Some("wrong"), &[], b"").status, 401);
    assert_eq!(served.send("PUT", "/o/b", Some(READ), &[], b"y").status, 401);
    assert_eq!(served.send("PUT", "/o/b", None, &[], b"y").status, 401);
    // And nothing was written by those
    assert_eq!(files_under(&served.objects()), vec!["a".to_string()]);
}

#[test]
fn given_an_existing_key_when_put_with_if_none_match_then_it_is_412_and_the_first_body_stays() {
    // Given a stored key
    let served = Served::start(&[]);
    served.send("PUT", "/o/k", Some(WRITE), &[], b"first");
    // When a second writer asks to be first, then it is refused
    let second = served.send("PUT", "/o/k", Some(WRITE), &[("If-None-Match", "*")], b"second");
    assert_eq!(second.status, 412);
    assert_eq!(std::fs::read(served.objects().join("k")).unwrap(), b"first");
    // And on a new key the same header stores, and a plain put overwrites
    assert_eq!(served.send("PUT", "/o/new", Some(WRITE), &[("If-None-Match", "*")], b"n").status, 200);
    assert_eq!(served.send("PUT", "/o/k", Some(WRITE), &[], b"over").status, 200);
    assert_eq!(std::fs::read(served.objects().join("k")).unwrap(), b"over");
}

#[test]
fn given_keys_that_climb_start_with_a_dot_or_are_absolute_when_requested_then_they_are_400() {
    // Given a server and a file outside its directory
    let served = Served::start(&[]);
    std::fs::write(served.dir.path().join("secret"), "outside").unwrap();
    // Then every bad key is 400 for read and write alike
    for path in ["/o/../secret", "/o/a/../../secret", "/o/.hidden", "/o/a/.b", "/o//etc/passwd", "/o/", "/o/a//b", "/o/a%2e%2e/b", "/o/a/"] {
        assert_eq!(served.send("GET", path, Some(READ), &[], b"").status, 400, "GET {path}");
        assert_eq!(served.send("PUT", path, Some(WRITE), &[], b"x").status, 400, "PUT {path}");
    }
    assert!(files_under(&served.objects()).is_empty());
}

#[test]
fn given_a_prefix_or_a_delete_when_requested_then_nothing_is_listed_and_nothing_is_removed() {
    // Given an object under a prefix
    let served = Served::start(&[]);
    served.send("PUT", "/o/nx/g0/one", Some(WRITE), &[], b"1");
    // Then a GET on the prefix is a miss, never a listing
    for prefix in ["/o/nx", "/o/nx/g0"] {
        assert_eq!(served.send("GET", prefix, Some(WRITE), &[], b"").status, 404);
    }
    // And DELETE is refused with either token and the object stays
    for token in [READ, WRITE] {
        assert_eq!(served.send("DELETE", "/o/nx/g0/one", Some(token), &[], b"").status, 405);
    }
    assert_eq!(files_under(&served.objects()), vec!["nx/g0/one".to_string()]);
}

#[test]
fn given_a_one_mb_limit_when_a_bigger_object_is_put_then_it_is_413_and_one_at_the_limit_is_stored() {
    // Given a server that takes at most 1 MB
    let served = Served::start(&["--max-object-mb", "1"]);
    // Then an object over the limit is refused and not stored, and one exactly at it is stored
    assert_eq!(served.send("PUT", "/o/big", Some(WRITE), &[], &vec![7u8; 1024 * 1024 + 1]).status, 413);
    assert!(!served.objects().join("big").exists());
    assert_eq!(served.send("PUT", "/o/fits", Some(WRITE), &[], &vec![7u8; 1024 * 1024]).status, 200);
}

#[test]
fn given_the_par_adapter_when_it_reaches_the_server_then_it_is_refused_as_a_credential_for_it_sends_no_bearer() {
    // Given a server that holds an object, and the PAR adapter pointed at the server's /o/ for both links
    let served = Served::start(&[]);
    served.send("PUT", "/o/probe", Some(WRITE), &[], b"x");
    let link = format!("{}/o/", served.url);
    let store = OciParStore::new(&link, Some(&link)).via(cli_http::Via::Direct);
    let key = Key::at("probe").unwrap();
    // Then a read and a write are both refused as a credential failure, and nothing is written
    assert!(matches!(store.get(&key), Err(StoreError::Credential(_))));
    assert!(matches!(store.put(&Key::at("other").unwrap(), b"y", true), Err(StoreError::Credential(_))));
    assert_eq!(files_under(&served.objects()), vec!["probe".to_string()]);
}

#[test]
fn given_a_token_file_that_is_missing_or_empty_when_serve_starts_then_it_fails_with_json_and_exit_1() {
    // Given an empty read token file
    let dir = tempfile::tempdir().unwrap();
    let (read, write) = (dir.path().join("read"), dir.path().join("write"));
    std::fs::write(&read, "\n").unwrap();
    std::fs::write(&write, "w").unwrap();
    let objects = dir.path().join("objects");
    let args = |read: &Path| vec!["bucket".to_string(), "serve".into(), "--dir".into(), objects.display().to_string(), "--listen".into(), "127.0.0.1:0".into(), "--read-token-file".into(), read.display().to_string(), "--write-token-file".into(), write.display().to_string()];
    for token in [read.clone(), dir.path().join("absent")] {
        // When serve starts, then it exits 1 with {ok: false}
        let words = args(&token);
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        let mut process = spawn_wf(&refs, &RunOpts::default());
        assert_eq!(process.wait(Duration::from_secs(10)), Some(1));
        let line: Value = serde_json::from_str(process.out().lines().find(|l| l.starts_with('{')).unwrap()).unwrap();
        assert_eq!(line["ok"], false);
    }
}
