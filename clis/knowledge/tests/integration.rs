//! The Gem sync as `.github/workflows/repomix-gemini-context.yml` runs it, black box: the thin `knowledge` binary started the way the
//! workflow starts it (the destination in `GEMINI_CONTEXT_REMOTE`, the job's summary file in `GITHUB_STEP_SUMMARY`), over a repository whose
//! `pnpm` (repomix) and `wf offload ask` are stubs on PATH, in the sequence the workflow produces: a full run, then the runs after it.
//! Nothing real is reachable. The destination is a `local:` folder: rclone's Drive backend cannot be pointed at a loopback fake, so the Drive
//! hop itself is proven by the live runs, each of which writes a proof line (file id, size, modified time) to its job summary.
//! The flow tests (`tests/flow.rs`) pass `--remote` and test one behaviour each; these follow the workflow's own environment and sequence.
#[path = "flow/support/mod.rs"]
mod support;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use support::{create_sh_stub, iso_from_ms, now_ms, Gem, RunResult};

/// The Gem sync as the workflow sets it up: a destination folder with the one bundle file, and the file the job summary is appended to.
struct Workflow {
    gem: Gem,
    remote_dir: PathBuf,
        remote: String,
    summary: PathBuf,
}

impl Workflow {
    fn new(answers: &[&str]) -> Workflow {
        let gem = Gem::new(answers);
        let remote_dir = gem.dir.path().join("drive-folder");
        std::fs::create_dir(&remote_dir).unwrap();
        let remote = format!("local:{}/ctx", remote_dir.display());
        let summary = gem.dir.path().join("step-summary.md");
        Workflow { gem, remote_dir, remote, summary }
    }

    /// `knowledge knowledge index --gemini` with the destination and the summary file in the environment, as the workflow gives them.
        fn sync(&self, flags: &[&str]) -> RunResult {
        self.gem.index_with_env(flags, &[("GEMINI_CONTEXT_REMOTE", &self.remote), ("GITHUB_STEP_SUMMARY", &self.summary.display().to_string())])
    }

        fn destination(&self) -> String {
        ["ctx-00-architecture.md", "ctx-01.md"].iter().map(|n| std::fs::read_to_string(self.remote_dir.join(n)).unwrap_or_default()).collect::<Vec<_>>().concat()
    }

    fn files_in_destination(&self) -> usize {
        std::fs::read_dir(&self.remote_dir).unwrap().count()
    }

    /// The proof lines the runs wrote to the job summary, oldest first.
    fn proofs(&self) -> Vec<String> {
        std::fs::read_to_string(&self.summary).unwrap_or_default().lines().filter(|l| l.starts_with("- doc ")).map(String::from).collect()
    }

    fn manifest(&self) -> String {
        std::fs::read_to_string(self.gem.repo.join("docs/gemini-context/manifest.json")).unwrap_or_default()
    }
}

#[test]
fn the_first_sync_uploads_the_bundle_to_the_destination_in_the_environment_and_proves_it_in_the_job_summary() {
    // Given a repository whose model drafts an architecture, and an empty destination named by GEMINI_CONTEXT_REMOTE
    let w = Workflow::new(&["# Architecture\\nui to domain"]);

    // When the sync runs as the workflow runs it (no --remote)
    let out = w.sync(&[]);

    // Then it succeeds and the one destination file is the architecture, a rule and the pack
    assert_eq!(out.exit_code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(w.destination(), "# Architecture\nui to domain\n## File: a.ts\nPACK\n");
    assert_eq!(w.files_in_destination(), 2);
    // and it printed one proof line per Doc, the same line the job summary got
    let proofs = w.proofs();
    assert_eq!(proofs.len(), 2, "{proofs:?}");
    assert!(proofs[0].starts_with("- doc ctx-00-architecture id=") && proofs[1].starts_with("- doc ctx-01 id="), "{proofs:?}");
    assert!(out.stdout.contains(&proofs[1][2..]), "{}", out.stdout);
    // and the manifest records the upload to that destination
    let manifest: Value = serde_json::from_str(&w.manifest()).unwrap();
    assert_eq!((manifest["uploaded"].clone(), manifest["remote"].clone()), (json!(true), json!(w.remote.clone())));
}

#[test]
fn the_next_sync_overwrites_the_same_file_in_place_with_the_new_pack_and_adds_a_second_proof() {
    // Given a destination that already holds the first night's bundle
    let w = Workflow::new(&["# Architecture\\nfirst night", "# Architecture\\nsecond night"]);
    assert_eq!(w.sync(&[]).exit_code, 0);
    let first = w.destination();

    // When the repository changed and the sync runs again
    w.gem.set_pack("PACK, but longer after a change");
    let out = w.sync(&[]);

    // Then it is still one file of that name, now holding the new bundle, and the summary lists both nights in order with different sizes
    assert_eq!(out.exit_code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(w.files_in_destination(), 2);
    assert_eq!(w.destination(), "# Architecture\nsecond night\n## File: a.ts\nPACK, but longer after a change\n");
    assert_ne!(w.destination(), first);
    let proofs = w.proofs();
    assert_eq!(proofs.len(), 4, "{proofs:?}");
    assert!(proofs[1].ends_with(&format!(" chars={}", "## File: a.ts\nPACK\n".len())) && proofs[3].ends_with(&format!(" chars={}", "## File: a.ts\nPACK, but longer after a change\n".len())), "{proofs:?}");
}

#[test]
fn a_scheduled_sync_reuses_the_cached_draft_never_asks_the_model_and_still_uploads_the_new_pack() {
    // Given a first sync that drafted and cached the architecture
    let w = Workflow::new(&["# Architecture\\nfirst"]);
    assert_eq!(w.sync(&[]).exit_code, 0);
    assert_eq!(w.gem.asks(), 1);

    // When the scheduled run (--no-summary) comes after the repository changed
    w.gem.set_pack("PACK v2");
    let out = w.sync(&["--no-summary"]);

    // Then the model was not asked again, and the destination holds the cached architecture with the new pack
    assert_eq!(out.exit_code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(w.gem.asks(), 1);
    assert_eq!(w.destination(), "# Architecture\nfirst\n## File: a.ts\nPACK v2\n");
    assert_eq!(w.proofs().len(), 4);
}

#[test]
fn a_sync_whose_draft_cannot_be_made_fails_and_leaves_the_destination_the_summary_and_the_manifest_as_they_were() {
    // Given a destination holding a good bundle, and a model that answers once and then nothing (every pool on cooldown)
    let w = Workflow::new(&["# Architecture\\nfirst"]);
    assert_eq!(w.sync(&[]).exit_code, 0);
    let (bundle, proofs, manifest) = (w.destination(), w.proofs(), w.manifest());

    // When the next full sync runs
    w.gem.set_pack("PACK that must not be uploaded");
    let out = w.sync(&[]);

    // Then it fails loudly, naming the cause, after three attempts, and nothing about the destination changed
    assert_eq!(out.exit_code, 1, "{}", out.stdout);
    assert!(out.stderr.contains("ARCHITECTURE.md could not be drafted after 3 attempts"), "{}", out.stderr);
    assert_eq!(w.gem.asks(), 4);
    assert_eq!((w.destination(), w.proofs(), w.manifest()), (bundle, proofs, manifest));
}

#[test]
fn a_drive_destination_without_a_login_fails_with_the_login_hint_before_the_model_is_asked_or_anything_is_uploaded() {
    // Given a Drive destination (a folder id, not local:) and a gcloud with no logged-in account
    let w = Workflow::new(&["# Architecture\\nunused"]);
    create_sh_stub(&w.gem.bin.join("gcloud"), "exit 0");
    let drive = w.gem.index_with_env(&[], &[("GEMINI_CONTEXT_REMOTE", "1folderIdOfTheGemContext/ctx"), ("GITHUB_STEP_SUMMARY", &w.summary.display().to_string())]);

    // Then it fails naming the login to run, before the slow draft, and nothing was uploaded, summarised or recorded
    assert_eq!(drive.exit_code, 1, "{}", drive.stdout);
    assert!(drive.stderr.contains("gcloud has no usable credential") && drive.stderr.contains("wf secret login gcloud"), "{}", drive.stderr);
    assert_eq!(w.gem.asks(), 0);
    assert!(w.proofs().is_empty() && w.manifest().is_empty() && w.files_in_destination() == 0);
}

/// A Drive that keeps Docs: the four calls of the sync (list, create, overwrite, read back) over loopback, stateful like the real one.
struct FakeDrive {
    base: String,
    docs: Arc<Mutex<BTreeMap<String, (String, String)>>>,
}

impl FakeDrive {
    fn start() -> FakeDrive {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let docs: Arc<Mutex<BTreeMap<String, (String, String)>>> = Arc::default();
        let state = docs.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = state.clone();
                std::thread::spawn(move || serve(stream, &state));
            }
        });
        FakeDrive { base, docs }
    }

    fn text_of(&self, name: &str) -> Option<String> {
        self.docs.lock().unwrap().values().find(|(n, _)| n == name).map(|(_, t)| t.clone())
    }
}

fn serve(mut stream: std::net::TcpStream, docs: &Mutex<BTreeMap<String, (String, String)>>) {
    let mut raw = Vec::new();
    let mut buf = [0u8; 65536];
    let (head_end, length) = loop {
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            return;
        }
        raw.extend_from_slice(&buf[..n]);
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&raw[..i]).to_lowercase();
            let length = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
            break (i + 4, length);
        }
    };
    while raw.len() < head_end + length {
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
    }
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let body = String::from_utf8_lossy(&raw[head_end..]).to_string();
    let mut first = head.lines().next().unwrap_or("").split(' ');
    let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let path = target.split('?').next().unwrap_or("");
    let mut docs = docs.lock().unwrap();
    let json_reply = |v: Value| (200, "application/json", v.to_string());
    let (status, kind, out) = match (method, path) {
        ("GET", "/drive/v3/files") => {
            let q = target.split("q=").nth(1).unwrap_or("").split('&').next().unwrap_or("");
            let q = q.replace("%20", " ").replace("%27", "'");
            let prefix = q.split("name contains '").nth(1).and_then(|r| r.split('\'').next()).unwrap_or("").to_string();
            let files: Vec<Value> = docs.iter().filter(|(_, (n, _))| n.contains(&prefix)).map(|(id, (n, _))| json!({ "id": id, "name": n, "mimeType": "application/vnd.google-apps.document" })).collect();
            json_reply(json!({ "files": files }))
        }
        ("POST", "/upload/drive/v3/files") => {
            let parts: Vec<&str> = body.split("\r\n--wfgembound7d2f").collect();
            let meta: Value = serde_json::from_str(parts[0].split("\r\n\r\n").nth(1).unwrap_or("{}")).unwrap_or_default();
            let text = parts[1].split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            let id = format!("doc{}", docs.len() + 1);
            docs.insert(id.clone(), (meta["name"].as_str().unwrap_or("").to_string(), text));
            json_reply(json!({ "id": id }))
        }
        ("PATCH", p) if p.starts_with("/upload/drive/v3/files/") => {
            let id = p.rsplit('/').next().unwrap_or("").to_string();
            match docs.get_mut(&id) {
                Some(doc) => {
                    doc.1 = body;
                    json_reply(json!({ "id": id }))
                }
                None => (404, "text/plain", "no such file".into()),
            }
        }
        ("PATCH", p) if p.starts_with("/drive/v3/files/") => {
            let id = p.rsplit('/').next().unwrap_or("").to_string();
            let trashed = body.contains("\"trashed\":true") && docs.remove(&id).is_some();
            if trashed { json_reply(json!({ "id": id })) } else { (404, "text/plain", "no such file".into()) }
        }
        ("GET", p) if p.ends_with("/export") => {
            let id = p.trim_end_matches("/export").rsplit('/').next().unwrap_or("");
            match docs.get(id) {
                Some((_, text)) => (200, "text/plain", format!("\u{feff}{}", text.replace('\n', "\r\n"))),
                None => (404, "text/plain", "no such file".into()),
            }
        }
        ("GET", p) if p.starts_with("/drive/v3/files/") => json_reply(json!({ "mimeType": "application/vnd.google-apps.document", "modifiedTime": iso_from_ms(now_ms()) })),
        _ => (400, "text/plain", format!("unexpected {method} {target}")),
    };
    let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
    let _ = stream.write_all(reply.as_bytes());
}

#[test]
fn a_sync_to_drive_creates_the_docs_the_first_night_overwrites_the_same_ones_the_next_and_proves_each_by_reading_it_back() {
    // Given a Drive that holds Docs, a gcloud that is logged in, and the workflow's environment pointing the sync at that Drive
    let w = Workflow::new(&["# Architecture\\nfirst night"]);
    create_sh_stub(&w.gem.bin.join("gcloud"), "case \"$2\" in list) echo me@example.com;; *) echo token;; esac");
    let drive = FakeDrive::start();
    let run = |flags: &[&str]| w.gem.index_with_env(flags, &[("GEMINI_CONTEXT_REMOTE", "1folder/ctx"), ("GITHUB_STEP_SUMMARY", &w.summary.display().to_string()), ("WF_GOOGLE_DRIVE_BASE", &drive.base)]);

    // When the sync runs
    let out = run(&[]);

    // Then the architecture and the pack are two Docs of the folder, each proven by its id and character count
    assert_eq!(out.exit_code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(drive.text_of("ctx-00-architecture").as_deref(), Some("# Architecture\nfirst night\n"));
    assert_eq!(drive.text_of("ctx-01").as_deref(), Some("## File: a.ts\nPACK\n"));
    let first_ids = w.proofs();
    assert_eq!(first_ids.len(), 2, "{first_ids:?}");

    // and an older file of the family sits in the folder
    drive.docs.lock().unwrap().insert("old1".into(), ("ctx.md".into(), "old layout".into()));
    drive.docs.lock().unwrap().insert("other1".into(), ("unrelated".into(), "keep".into()));

    // When the pack changed and the sync runs again
    w.gem.set_pack("PACK v2");
    let out = run(&["--no-summary"]);

    // Then the same two Docs (no third) hold the new text, and the proofs name the same ids
    assert_eq!(out.exit_code, 0, "{}{}", out.stdout, out.stderr);
    // and the stale one is trashed while an unrelated file is left alone
    assert!(out.stdout.contains("trashed stale ctx.md id=old1"), "{}", out.stdout);
    assert_eq!(drive.text_of("ctx.md"), None);
    assert_eq!(drive.text_of("unrelated").as_deref(), Some("keep"));
    assert_eq!(drive.docs.lock().unwrap().len(), 3);
    assert_eq!(drive.text_of("ctx-01").as_deref(), Some("## File: a.ts\nPACK v2\n"));
    let all: Vec<String> = w.proofs().into_iter().filter(|l| l.starts_with("- doc ")).collect();
    let id = |l: &str| l.split("id=").nth(1).unwrap_or("").split(' ').next().unwrap_or("").to_string();
    assert_eq!((id(&all[0]), id(&all[1])), (id(&all[2]), id(&all[3])));
}
