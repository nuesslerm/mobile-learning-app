//! The group over a blob store in memory and an executor that is a double: what it keeps and answers, and what it asks of the executor to place
//! a file in a workspace.
use super::*;
use crate::testing::{CountingIds, FixedClock, MemoryBlobs};
use agent_api_attachments as api;
use agent_api_proto::Method;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rca_agent_core::{AgentApi, Build, Group};
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel, RemoteError};
use rca_exec_channel::testing::ScriptedChannel;
use rca_exec_channel::{Executors, Place, Registry};
use serde_json::{json, Value};
use std::sync::Arc;

const BUILD: Build<'static> = Build { version: "1", commit: "c" };
const NOW: u64 = 1_700_000_000_000;

struct Rig {
    api: AgentApi,
    blobs: Arc<MemoryBlobs>,
    executors: Arc<Registry>,
}

fn rig(channel: Arc<dyn ExecutorChannel>) -> Rig {
    let blobs = Arc::new(MemoryBlobs::default());
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "w1", channel).unwrap();
    let attachments = Attachments::new(blobs.clone(), Arc::new(FixedClock(NOW)), Arc::new(CountingIds::default()), executors.clone());
    let groups: Vec<Arc<dyn Group>> = vec![Arc::new(AttachmentsGroup::new(attachments))];
    Rig { api: AgentApi::new(groups).unwrap(), blobs, executors }
}

fn placing() -> Arc<ScriptedChannel> {
    ScriptedChannel::new(|tool, args| Ok(ActionOutput { result: json!({"path": args["path"], "bytes": 0, "tool": tool}), output: String::new() }))
}

impl Rig {
    async fn ask(&self, method: Method, params: Value) -> Value {
        self.api.dispatch_text(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string()).await
    }

    async fn put(&self, params: Value) -> Value {
        let mut full = json!({"name": "shot.png", "mime_type": "image/png", "kind": "image"});
        full.as_object_mut().unwrap().extend(params.as_object().unwrap().clone());
        self.ask(api::UPLOAD_PUT, full).await
    }
}

fn b64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

fn code_of(answer: &Value) -> &str {
    answer["error"]["code"].as_str().unwrap_or_else(|| panic!("not a refusal: {answer}"))
}

#[test]
fn given_the_served_rows_when_compared_with_the_contract_then_each_method_has_one_in_order_and_only_stage_names_an_action() {
    // Given the rows and the contract
    let rows: Vec<(Method, Vec<&str>)> = SERVED.iter().map(|s| (s.method, s.actions.to_vec())).collect();
    // Then the rows are the contract's methods in order and only the one that writes into a workspace names an executor action
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), api::METHODS);
    assert_eq!(rows.iter().filter(|r| !r.1.is_empty()).map(|r| (r.0, r.1.clone())).collect::<Vec<_>>(), [(api::STAGE, vec!["put_blob"])]);
}

#[tokio::test]
async fn given_bytes_put_when_read_back_in_chunks_then_they_come_back_exact_with_the_digest_and_the_end_marked() {
    // Given an attachment of every byte value put under a name the caller chose
    let rig = rig(placing());
    let bytes: Vec<u8> = (0..=255u8).collect();
    let put = rig.put(json!({"id": "a1", "data": b64(&bytes)})).await;
    // Then it is kept with its size, its time from the clock and the digest of the bytes
    let view = &put["result"]["attachment"];
    assert_eq!((view["id"].clone(), view["size_bytes"].clone(), view["created_at_ms"].clone()), (json!("a1"), json!(256), json!(NOW)), "{put}");
    let digest: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(view["sha256"], json!(digest));
    // When it is read in two ranges, the second past the end of the data
    let first = rig.ask(api::GET, json!({"id": "a1", "offset": 0, "length": 100})).await;
    let rest = rig.ask(api::GET, json!({"id": "a1", "offset": 100, "length": 1000})).await;
    // Then the bytes joined are the original and only the last range is the end
    let read = |a: &Value| STANDARD.decode(a["result"]["data"].as_str().unwrap()).unwrap();
    assert_eq!([&read(&first)[..], &read(&rest)[..]].concat(), bytes);
    assert_eq!((first["result"]["eof"].clone(), rest["result"]["eof"].clone(), rest["result"]["bytes"].clone()), (json!(false), json!(true), json!(156)));
}

#[tokio::test]
async fn given_a_put_when_repeated_with_other_bytes_or_too_large_or_with_no_id_then_each_is_answered_by_the_contracts_rule() {
    // Given an attachment under an id
    let rig = rig(placing());
    rig.put(json!({"id": "a1", "data": b64(b"one")})).await;
    // When the same bytes are put again, other bytes under the id, and bytes past the cap
    let again = rig.put(json!({"id": "a1", "data": b64(b"one")})).await;
    let other = rig.put(json!({"id": "a1", "data": b64(b"two")})).await;
    let huge = rig.put(json!({"id": "a2", "data": b64(&vec![1u8; api::MAX_BYTES as usize + 1])})).await;
    // Then the first is the same attachment, the second is ATTACHMENT_EXISTS and the original stays, and the third is ATTACHMENT_TOO_LARGE and is not kept
    assert_eq!(again["result"]["attachment"]["id"], json!("a1"));
    assert_eq!(code_of(&other), api::code::ATTACHMENT_EXISTS);
    let read = rig.ask(api::GET, json!({"id": "a1"})).await;
    assert_eq!(read["result"]["data"], json!(b64(b"one")));
    assert_eq!(code_of(&huge), api::code::ATTACHMENT_TOO_LARGE);
    assert_eq!(rig.blobs.ids(), ["a1"]);
    // And two puts with no id get two different ids the service made
    let (x, y) = (rig.put(json!({"data": b64(b"x")})).await, rig.put(json!({"data": b64(b"y")})).await);
    assert_ne!(x["result"]["attachment"]["id"], y["result"]["attachment"]["id"]);
    assert_eq!(rig.blobs.ids().len(), 3);
}

#[tokio::test]
async fn given_a_store_that_fails_when_called_then_each_method_is_unavailable_and_nothing_is_kept() {
    // Given a service whose store fails
    let rig = rig(placing());
    rig.put(json!({"id": "a1", "data": b64(b"one")})).await;
    rig.blobs.fail_with(Some("disk full"));
    // When each method is called
    let answers = [
        rig.put(json!({"id": "a2", "data": b64(b"two")})).await,
        rig.ask(api::GET, json!({"id": "a1"})).await,
        rig.ask(api::DELETE, json!({"id": "a1"})).await,
        rig.ask(api::STAGE, json!({"workspace": "w1", "ids": ["a1"]})).await,
    ];
    // Then every one says so, naming the cause, and the executor was asked nothing
    for answer in &answers {
        assert_eq!(code_of(answer), api::code::ATTACHMENTS_UNAVAILABLE, "{answer}");
        assert!(answer["error"]["message"].as_str().unwrap().contains("disk full"));
    }
    rig.blobs.fail_with(None);
    assert_eq!(rig.blobs.ids(), ["a1"]);
}

#[tokio::test]
async fn given_attachments_when_staged_then_the_executor_is_asked_put_blob_with_each_exact_bytes_at_a_path_that_stays_below_the_attachment_directory() {
    // Given two attachments, one named so that it would climb out of its directory, and an executor that answers
    let channel = placing();
    let rig = rig(channel.clone());
    let (image, note) = (vec![0u8, 255, 7, 8], b"hello".to_vec());
    rig.put(json!({"id": "a1", "data": b64(&image)})).await;
    rig.put(json!({"id": "a2", "name": "../../etc/notes.txt", "mime_type": "text/plain", "kind": "file", "data": b64(&note)})).await;
    // When they are staged in the workspace
    let staged = rig.ask(api::STAGE, json!({"workspace": "w1", "ids": ["a1", "a2"]})).await;
    // Then the executor got the ignore file and then one put_blob each, in order, with the bytes and the path the contract derives, and the answer names those paths
    let calls = channel.calls();
    assert_eq!(calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["put_blob", "put_blob", "put_blob"]);
    assert_eq!(calls[0].1, json!({"path": ".rca/.gitignore", "data": b64(b"*\n")}));
    assert_eq!(calls[1].1, json!({"path": ".rca/attachments/a1/shot.png", "data": b64(&image)}));
    assert_eq!(calls[2].1, json!({"path": ".rca/attachments/a2/notes.txt", "data": b64(&note)}));
    let files = staged["result"]["files"].as_array().unwrap();
    assert_eq!(files.iter().map(|f| f["path"].as_str().unwrap()).collect::<Vec<_>>(), [".rca/attachments/a1/shot.png", ".rca/attachments/a2/notes.txt"], "{staged}");
    assert_eq!(files[1]["name"], json!("../../etc/notes.txt"));
}

#[tokio::test]
async fn given_a_stage_that_cannot_be_done_when_asked_then_it_writes_nothing_or_says_what_the_executor_said() {
    // Given one attachment and an executor that refuses a path that is taken the first time and anything else the second time
    let channel = ScriptedChannel::new(|_, args| match args["path"].as_str() {
        Some(p) if p.contains("/taken/") => Err(RemoteError::Action { code: "ALREADY_EXISTS".into(), message: "exists".into() }),
        Some(p) if p.contains("/broken/") => Err(RemoteError::Action { code: "PATH_OUTSIDE_ROOT".into(), message: "outside".into() }),
        _ => Ok(ActionOutput { result: json!({}), output: String::new() }),
    });
    let rig = rig(channel.clone());
    for id in ["good", "taken", "broken"] {
        rig.put(json!({"id": id, "data": b64(id.as_bytes())})).await;
    }
    // When an unknown id is among the ids, Then nothing is written, not even for the ids before it
    let unknown = rig.ask(api::STAGE, json!({"workspace": "w1", "ids": ["good", "nope"]})).await;
    assert_eq!(code_of(&unknown), api::code::ATTACHMENT_NOT_FOUND);
    assert!(channel.calls().is_empty());
    // When a file is already in place, Then it is the same file and the stage succeeds
    let taken = rig.ask(api::STAGE, json!({"workspace": "w1", "ids": ["taken"]})).await;
    assert_eq!(taken["result"]["files"][0]["path"], json!(".rca/attachments/taken/shot.png"), "{taken}");
    // When the executor refuses another way, Then its code and message come back as they were
    let broken = rig.ask(api::STAGE, json!({"workspace": "w1", "ids": ["broken"]})).await;
    assert_eq!((code_of(&broken), broken["error"]["message"].clone()), ("PATH_OUTSIDE_ROOT", json!("outside")));
    // And a workspace nobody serves is NO_EXECUTOR
    let away = rig.ask(api::STAGE, json!({"workspace": "w9", "ids": ["good"]})).await;
    assert_eq!((code_of(&away), away["error"]["message"].clone()), ("NO_EXECUTOR", json!("no executor is attached for workspace w9")));
}

#[tokio::test]
async fn given_a_directory_workspace_when_staged_then_the_path_the_executor_gets_is_moved_under_the_workspace_place() {
    // Given a workspace that is a directory of the executor's root, and an executor that records
    let channel = placing();
    let rig = rig(channel.clone());
    rig.executors.add_workspace_at("e1", "sub", Place::directory("pkg/app").unwrap()).unwrap();
    rig.put(json!({"id": "a1", "data": b64(b"x")})).await;
    // When the attachment is staged in it
    let staged = rig.ask(api::STAGE, json!({"workspace": "sub", "ids": ["a1"]})).await;
    // Then the executor wrote under the directory, and the answer still gives the path relative to the workspace
    assert_eq!(channel.calls().iter().map(|c| c.1["path"].as_str().unwrap().to_string()).collect::<Vec<_>>(), ["pkg/app/.rca/.gitignore", "pkg/app/.rca/attachments/a1/shot.png"]);
    assert_eq!(staged["result"]["files"][0]["path"], json!(".rca/attachments/a1/shot.png"));
}

#[tokio::test]
async fn given_a_delete_when_repeated_then_it_is_safe_and_what_was_deleted_can_no_longer_be_read_or_staged() {
    // Given an attachment
    let rig = rig(placing());
    rig.put(json!({"id": "a1", "data": b64(b"x")})).await;
    // When it is deleted twice
    let first = rig.ask(api::DELETE, json!({"id": "a1"})).await;
    let second = rig.ask(api::DELETE, json!({"id": "a1"})).await;
    // Then the first deleted it and the second found nothing and did not fail, and it is gone for a read and a stage
    assert_eq!((first["result"]["deleted"].clone(), second["result"]["deleted"].clone()), (json!(true), json!(false)));
    assert_eq!(code_of(&rig.ask(api::GET, json!({"id": "a1"})).await), api::code::ATTACHMENT_NOT_FOUND);
    assert_eq!(code_of(&rig.ask(api::STAGE, json!({"workspace": "w1", "ids": ["a1"]})).await), api::code::ATTACHMENT_NOT_FOUND);
}

use sha2::Digest;
