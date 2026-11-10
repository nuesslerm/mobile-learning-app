//! `attachments.*` and `assets.*` through the chain: an image and a note a phone uploads to the URLs it was given are kept by the real rca-service,
//! written into the workspace by the real rca-exec (`put_blob`) when a message carries them, and read there by the scripted CLI; the phone gets
//! them back through an asset URL, and a delete takes one away. A name that climbs out of the attachment directory, an id that does, and an upload
//! past the cap are refused where they are stopped. A call counts for its row only when it was answered and the agent API method ran in its window.
use crate::chain::{hit, last_window, said};
use crate::rig::*;
use crate::support::*;
use agent_api_attachments::MAX_BYTES;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use exec_testkit::script::{Script, Turn};
use std::io::{Read, Write};
use std::net::SocketAddr;

fn all_scopes() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

struct Answer {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Answer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// One HTTP exchange with a raw body in and out, carrying the `Content-Length` the caller names (none leaves the header out).
fn raw(addr: SocketAddr, method: &str, path: &str, length: Option<usize>, body: &[u8]) -> Answer {
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(60))).unwrap();
    let length = length.map(|n| format!("Content-Length: {n}\r\n")).unwrap_or_default();
    stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: t3\r\nConnection: close\r\n{length}\r\n").as_bytes()).unwrap();
    // a server that refuses on the head alone closes before the body is sent
    let _ = stream.write_all(body);
    let mut all = Vec::new();
    let _ = stream.read_to_end(&mut all);
    let at = all.windows(4).position(|w| w == b"\r\n\r\n").expect("a response head");
    let head = String::from_utf8_lossy(&all[..at]).into_owned();
    Answer { status: head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap(), head, body: all[at + 4..].to_vec() }
}

/// Bytes that are no text: every value of a byte, in an order a text decoder would reject.
fn binary(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 + 3) as u8).collect()
}

/// The upload URL of an attachment as one call of the trace, with the id it will have.
fn upload_url(case: &mut exec_testkit::Case, chain: &Chain, phone: &mut Phone, (kind, name, mime): (&str, &str, &str), size: usize) -> (String, String) {
    let exit = phone.call(case, chain, &["rpc:attachments.createUploadUrl"], "attachments.createUploadUrl", json!({"type": kind, "name": name, "mimeType": mime, "sizeBytes": size}));
    let made = success(&exit);
    (made["attachmentId"].as_str().unwrap().to_string(), made["relativeUrl"].as_str().unwrap().to_string())
}

/// The bytes posted to an upload URL as one call of the trace: served when they were kept (204), so the window shows `upload.put`.
fn upload(case: &mut exec_testkit::Case, chain: &Chain, url: &str, bytes: &[u8]) -> Answer {
    case.call(&chain.tap, &[], || {
        let answer = raw(chain.addr, "POST", url, Some(bytes.len()), bytes);
        (answer.status == 204, answer)
    })
}

fn answered(case: &exec_testkit::Case, method: &str) {
    let (agent, _) = last_window(case);
    assert!(agent.iter().any(|(m, ok)| m == method && *ok), "{method} did not answer in the call's window: {agent:?}");
}

fn dispatch(id: &str, text: &str, attachments: Vec<Value>) -> Value {
    json!({"type": "message.dispatch", "commandId": id, "threadId": "t1", "message": {"messageId": format!("m-{id}"), "text": text, "attachments": attachments}})
}

fn completed(projection: &Value) -> bool {
    projection["runs"].as_array().and_then(|r| r.last()).is_some_and(|r| r["status"] == "completed")
}

/// Where rca-exec puts an attachment in the workspace.
fn staged(id: &str, name: &str) -> String {
    format!(".rca/attachments/{id}/{name}")
}

#[test]
fn an_image_and_a_note_uploaded_to_their_urls_are_written_into_the_workspace_by_rca_exec_for_the_turn_and_come_back_through_asset_urls_and_a_delete_takes_one_away() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a paired phone, a project and thread, and an image and a note to attach
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        let image = binary(3000);
        let note = b"the note the phone attached\n".to_vec();
        // When it asks for an upload URL for each and posts the bytes
        let (image_id, image_url) = upload_url(case, &chain, &mut phone, ("image", "shot.png", "image/png"), image.len());
        assert!(image_id.starts_with("pending-") && image_url.starts_with("/api/attachments/upload/"), "{image_id} {image_url}");
        let stored = upload(case, &chain, &image_url, &image);
        assert_eq!(stored.status, 204, "{}", stored.head);
        answered(case, "upload.put");
        let (note_id, note_url) = upload_url(case, &chain, &mut phone, ("file", "notes.txt", "text/plain"), note.len());
        assert_eq!(upload(case, &chain, &note_url, &note).status, 204);
        // And the same URL used again with the same bytes (a retry after a lost answer) is 204 and with other bytes of that length is 409, the note unchanged
        assert_eq!(raw(chain.addr, "POST", &note_url, Some(note.len()), &note).status, 204);
        assert_eq!(raw(chain.addr, "POST", &note_url, Some(note.len()), &vec![b'x'; note.len()]).status, 409);
        // Then the service keeps both by the ids it gave, byte for byte, and the repository has none of them yet
        let mut rca = chain.direct();
        let kept_note = rca.call("attachments", "attachment.get", json!({"id": note_id})).expect("rca keeps the note");
        assert_eq!(kept_note["data"], json!(STANDARD.encode(&note)));
        let held = rca.call("attachments", "attachment.get", json!({"id": image_id, "length": 4096})).expect("rca keeps the image");
        assert_eq!((held["data"].clone(), held["attachment"]["size_bytes"].clone(), held["attachment"]["mime_type"].clone()), (json!(STANDARD.encode(&image)), json!(3000), json!("image/png")));
        assert!(!chain.repo.path().join(".rca").exists());
        // When the phone sends a message that names both and the CLI's turn reads the note at the path the prompt gives
        let note_at = staged(&note_id, "notes.txt");
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("reading the note")]).then_calling("k1", "read_file", json!({"path": note_at})).then_line(&said("read it"))]));
        let attachments = vec![json!({"type": "image", "id": image_id, "name": "shot.png", "mimeType": "image/png", "sizeBytes": 3000}), json!({"type": "file", "id": note_id, "name": "notes.txt", "mimeType": "text/plain", "sizeBytes": note.len()})];
        let projection = phone.command_settled(case, &chain, dispatch("c-a1", "look at these", attachments.clone()), "t1", completed);
        // Then the executor wrote each below the attachment directory, byte for byte, and the window names put_blob (the ignore file and the two)
        assert_eq!(std::fs::read(chain.repo.path().join(staged(&image_id, "shot.png"))).unwrap(), image);
        assert_eq!(std::fs::read(chain.repo.path().join(&note_at)).unwrap(), note);
        let (agent, executor) = last_window(case);
        assert!(agent.iter().any(|(m, ok)| m == "turn.start" && *ok), "{agent:?}");
        assert_eq!(executor.as_array().unwrap().iter().filter(|h| h["tool"] == "put_blob" && h["ok"] == true).count(), 3, "{executor}");
        assert_eq!(hit(&executor, "tool", "read_file"), Some(&json!({"tool": "read_file", "ok": true})), "{executor}");
        // And the repository's status shows none of them, the CLI was started with the prompt and then both paths, and the CLI read the note
        assert_eq!(chain.repo.git(&["status", "--porcelain"]), "");
        let prompt = chain.cli.invocations("claude").iter().flat_map(|start| start.argv.clone()).find(|a| a.contains("look at these")).expect("the prompt reached the CLI");
        assert!(prompt.ends_with(&format!("\n- {}\n- {note_at}", staged(&image_id, "shot.png"))), "{prompt}");
        let output = rca.events("t1", "tool.output").iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n");
        assert!(output.contains("the note the phone attached"), "{output}");
        // And the message's row shows both attachments as the phone sent them
        let row = projection["messages"].as_array().unwrap().iter().find(|m| m["id"] == "m-c-a1").expect("the message row").clone();
        assert_eq!(row["attachments"], json!(attachments));
        // When the phone asks for asset URLs and fetches them
        let image_asset = phone.call(case, &chain, &["rpc:assets.createUrl"], "assets.createUrl", json!({"resource": {"_tag": "attachment", "attachmentId": image_id, "disposition": "inline"}}));
        answered(case, "attachment.get");
        let note_asset = phone.call(case, &chain, &[], "assets.createUrl", json!({"resource": {"_tag": "attachment", "attachmentId": note_id, "disposition": "inline"}}));
        let (image_path, note_path) = (success(&image_asset)["relativeUrl"].as_str().unwrap().to_string(), success(&note_asset)["relativeUrl"].as_str().unwrap().to_string());
        let got_image = raw(chain.addr, "GET", &image_path, Some(0), b"");
        let got_note = raw(chain.addr, "GET", &note_path, Some(0), b"");
        // Then the image is its bytes and shown inline, and the note is its bytes and a download whatever the phone asked
        let (head_image, head_note) = (got_image.head.to_ascii_lowercase(), got_note.head.to_ascii_lowercase());
        assert_eq!((got_image.status, got_image.body.clone()), (200, image.clone()), "{head_image}");
        assert!(head_image.contains("content-type: image/png") && head_image.contains("content-disposition: inline"), "{head_image}");
        assert_eq!((got_note.status, got_note.body.clone()), (200, note.clone()), "{head_note}");
        assert!(head_note.contains("content-type: text/plain") && head_note.contains("content-disposition: attachment"), "{head_note}");
        // When the phone keeps an image it holds as a data URL under an id of its own
        let png = binary(900);
        let kept = phone.call(case, &chain, &["rpc:assets.persistChatAttachments"], "assets.persistChatAttachments", json!({"threadId": "t1", "messageId": "m-c-a2", "attachments": [{"type": "image", "id": "pasted-1", "name": "pasted.png", "mimeType": "image/png", "sizeBytes": png.len(), "dataUrl": format!("data:image/png;base64,{}", STANDARD.encode(&png))}]}));
        answered(case, "upload.put");
        // Then it is a ChatImageAttachment of that id, and the service holds its bytes
        assert_eq!(success(&kept)["attachments"][0], json!({"type": "image", "id": "pasted-1", "name": "pasted.png", "mimeType": "image/png", "sizeBytes": 900}));
        assert_eq!(rca.call("attachments", "attachment.get", json!({"id": "pasted-1", "length": 4096})).unwrap()["data"], json!(STANDARD.encode(&png)));
        // When an image is uploaded and never sent, and its asset URL is made, and the phone deletes it
        let (spare_id, spare_url) = upload_url(case, &chain, &mut phone, ("image", "spare.png", "image/png"), 40);
        assert_eq!(upload(case, &chain, &spare_url, &binary(40)).status, 204);
        let spare_asset = phone.call(case, &chain, &[], "assets.createUrl", json!({"resource": {"_tag": "attachment", "attachmentId": spare_id}}));
        let spare_path = success(&spare_asset)["relativeUrl"].as_str().unwrap().to_string();
        assert_eq!(raw(chain.addr, "GET", &spare_path, Some(0), b"").status, 200);
        let deleted = phone.call(case, &chain, &["rpc:attachments.delete"], "attachments.delete", json!({"attachmentId": spare_id}));
        answered(case, "attachment.delete");
        // Then the service no longer holds it, its asset URL is 404, and the workspace copies of the sent ones are as they were
        assert_eq!(*success(&deleted), Value::Null);
        assert_eq!(rca.call("attachments", "attachment.get", json!({"id": spare_id})).unwrap_err().code, "ATTACHMENT_NOT_FOUND");
        assert_eq!(raw(chain.addr, "GET", &spare_path, Some(0), b"").status, 404);
        assert_eq!(std::fs::read(chain.repo.path().join(&note_at)).unwrap(), note);
    });
}

#[test]
fn a_name_that_climbs_out_is_kept_below_the_attachment_directory_an_id_that_climbs_out_is_refused_and_an_upload_past_what_the_service_keeps_is_refused_at_each_door() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a paired phone and a CLI whose turns only answer
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("seen")])]));
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When a file named for a path outside the repository is uploaded and sent
        let (id, url) = upload_url(case, &chain, &mut phone, ("file", "../../../outside.txt", "text/plain"), 5);
        assert_eq!(upload(case, &chain, &url, b"hello").status, 204);
        let projection = phone.command_settled(case, &chain, dispatch("c-b1", "see it", vec![json!({"type": "file", "id": id, "name": "../../../outside.txt", "mimeType": "text/plain", "sizeBytes": 5})]), "t1", completed);
        // Then rca-exec wrote it by its last segment below the attachment directory, and no such file is anywhere above or beside it
        assert!(completed(&projection));
        assert_eq!(std::fs::read(chain.repo.path().join(staged(&id, "outside.txt"))).unwrap(), b"hello");
        let root = chain.repo.path();
        for outside in [root.join("outside.txt"), root.parent().unwrap().join("outside.txt"), root.parent().unwrap().parent().unwrap().join("outside.txt")] {
            assert!(!outside.exists(), "{} was written", outside.display());
        }
        // When a message names an attachment by an id that climbs out, Then it is refused, no turn starts and the executor writes nothing more
        let starts = chain.cli.invocations("claude").len();
        let refused = phone.calls.call("orchestration.dispatchCommand", dispatch("c-b2", "x", vec![json!({"type": "file", "id": "../../etc/passwd", "name": "p", "mimeType": "text/plain", "sizeBytes": 1})]));
        assert!(failure(&refused).to_string().contains("every attachment needs an id"), "{refused}");
        assert_eq!(chain.cli.invocations("claude").len(), starts);
        // And rca refuses such an id itself, to read, to delete and to place, so a mapper that let one through could not reach the volume or the workspace
        let mut rca = chain.direct();
        for bad in ["../../etc/passwd", "a/b", ".hidden"] {
            for (method, params) in [("attachment.get", json!({"id": bad})), ("attachment.delete", json!({"id": bad})), ("attachment.stage", json!({"workspace": "w1", "ids": [bad]}))] {
                assert_eq!(rca.call("attachments", method, params).unwrap_err().code, "INVALID_PARAMS", "{method} {bad}");
            }
        }
        // When the phone asks for an upload URL for one byte more than the service keeps, and for an image the pin refuses, Then each is refused and no URL is made
        let over = phone.calls.call("attachments.createUploadUrl", json!({"type": "file", "name": "big.bin", "mimeType": "application/octet-stream", "sizeBytes": MAX_BYTES + 1}));
        assert_eq!(failure(&over)["_tag"], "UnsupportedOperationError", "{over}");
        let svg = phone.calls.call("attachments.createUploadUrl", json!({"type": "image", "name": "a.svg", "mimeType": "image/svg+xml", "sizeBytes": 10}));
        assert_eq!(svg["cause"][0]["_tag"], "Die", "{svg}");
        // When bytes are posted to a URL made for fewer, more bytes than the service keeps are announced, or the length is left out, Then each is 400 and nothing is kept
        let (small_id, small_url) = upload_url(case, &chain, &mut phone, ("file", "small.bin", "application/octet-stream"), 8);
        let wrong = raw(chain.addr, "POST", &small_url, Some(9), b"123456789");
        let huge = raw(chain.addr, "POST", &small_url, Some(MAX_BYTES as usize + 1), b"");
        let none = raw(chain.addr, "POST", &small_url, None, b"");
        assert_eq!((wrong.status, wrong.text()), (400, "Content-Length must match the upload size.".to_string()));
        assert_eq!(huge.status, 400, "{}", huge.text());
        assert_eq!((none.status, none.text()), (400, "Body was 0 bytes, expected 8.".to_string()));
        assert_eq!(rca.call("attachments", "attachment.get", json!({"id": small_id})).unwrap_err().code, "ATTACHMENT_NOT_FOUND");
        // And rca's own cap holds when the bytes come straight to it: one byte more is refused whole
        let too_big = rca.call("attachments", "upload.put", json!({"id": "big-1", "name": "big.bin", "mime_type": "application/octet-stream", "kind": "file", "data": STANDARD.encode(vec![1u8; MAX_BYTES as usize + 1])}));
        assert_eq!(too_big.unwrap_err().code, "ATTACHMENT_TOO_LARGE");
        assert_eq!(rca.call("attachments", "attachment.get", json!({"id": "big-1"})).unwrap_err().code, "ATTACHMENT_NOT_FOUND");
    });
}

#[test]
fn an_upload_of_the_most_the_service_keeps_is_kept_whole_staged_by_the_executor_and_read_back_through_its_asset_url() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a paired phone, and a file of exactly the most an attachment may hold
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("seen")])]));
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        let bytes = binary(MAX_BYTES as usize);
        // When it is uploaded, sent with a message and fetched back
        let (id, url) = upload_url(case, &chain, &mut phone, ("file", "max.bin", "application/octet-stream"), bytes.len());
        let stored = upload(case, &chain, &url, &bytes);
        assert_eq!(stored.status, 204, "{} {}\n{}", stored.head, stored.text(), chain.evidence());
        let sent = dispatch("c-c1", "the biggest", vec![json!({"type": "file", "id": id, "name": "max.bin", "mimeType": "application/octet-stream", "sizeBytes": bytes.len()})]);
        let projection = phone.command_settled(case, &chain, sent, "t1", completed);
        let asset = phone.call(case, &chain, &[], "assets.createUrl", json!({"resource": {"_tag": "attachment", "attachmentId": id}}));
        let got = raw(chain.addr, "GET", success(&asset)["relativeUrl"].as_str().unwrap(), Some(0), b"");
        // Then the turn completed with the file in the workspace whole, and the asset URL gives it back whole
        assert!(completed(&projection), "{}", chain.evidence());
        assert!(std::fs::read(chain.repo.path().join(staged(&id, "max.bin"))).unwrap() == bytes, "the staged file differs");
        assert!(got.status == 200 && got.body == bytes, "{} {}", got.status, got.head);
    });
}
