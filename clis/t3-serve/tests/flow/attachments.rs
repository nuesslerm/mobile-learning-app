//! `attachments.*` and `assets.*` through the real router and HTTP routes over an agent double that keeps attachments as the agent API does:
//! what a phone's upload URL and asset URL are, what each route checks before it asks the agent API anything, the bytes that cross, and
//! the attachments a message or a launch carries into the turn. The chain (`tests/integration/attachments_m12.rs`) runs the same calls through the
//! real service and executor.
use super::support::*;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use t3_core::ports::{AgentError, Op};

/// The bytes of an attachment kept by the double, by id, with what `upload.put` was given.
type Kept = Arc<Mutex<HashMap<String, (Value, Vec<u8>)>>>;

const CHUNK: usize = 5;

fn started() -> (Harness, Rpc, Kept) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let mut rpc = connect_with(&h, all);
    seed(&mut rpc);
    let kept: Kept = Arc::default();
    let store = kept.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| {
        let Op::Attachment { method, params } = op else { return None };
        let mut store = store.lock().unwrap();
        let id = params["id"].as_str().unwrap_or_default().to_string();
        let refuse = |code: &str, message: &str| Err(AgentError::Refused { code: code.into(), message: message.into() });
        Some(match *method {
            "upload.put" => {
                let bytes = STANDARD.decode(params["data"].as_str().unwrap()).unwrap();
                if store.contains_key(&id) {
                    return Some(refuse("ATTACHMENT_EXISTS", "attachment already holds other bytes"));
                }
                let view = json!({"id": id, "name": params["name"], "mime_type": params["mime_type"], "kind": params["kind"], "size_bytes": bytes.len(), "sha256": format!("sha-{}", bytes.len()), "created_at_ms": 0});
                store.insert(id, (view.clone(), bytes));
                Ok(json!({"attachment": view}))
            }
            "attachment.get" => match store.get(&id) {
                None => refuse("ATTACHMENT_NOT_FOUND", &format!("no attachment has the id {id}")),
                Some((view, bytes)) => {
                    let (offset, length) = (params["offset"].as_u64().unwrap_or(0) as usize, (params["length"].as_u64().unwrap_or(CHUNK as u64) as usize).min(CHUNK));
                    let end = (offset + length).min(bytes.len());
                    let part = &bytes[offset.min(end)..end];
                    Ok(json!({"attachment": view, "data": STANDARD.encode(part), "offset": offset, "bytes": part.len(), "eof": end >= bytes.len()}))
                }
            },
            _ => Ok(json!({"id": id, "deleted": store.remove(&id).is_some()})),
        })
    }));
    (h, rpc, kept)
}

fn asked(h: &Harness) -> Vec<(String, Value)> {
    h.agent.ops.lock().unwrap().iter().filter_map(|op| if let Op::Attachment { method, params } = op { Some((method.to_string(), params.clone())) } else { None }).collect()
}

struct Answer {
    status: u16,
    head: String,
    body: Vec<u8>,
}

/// One HTTP exchange with a raw body in and out, which `support::http` (JSON bodies) cannot carry.
fn raw(h: &Harness, method: &str, path: &str, body: &[u8]) -> Answer {
    raw_as(h, method, path, Some(body.len()), body)
}

/// The same with the `Content-Length` header the caller names (none leaves it out).
fn raw_as(h: &Harness, method: &str, path: &str, length: Option<usize>, body: &[u8]) -> Answer {
    let mut stream = std::net::TcpStream::connect(h.addr).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let length = length.map(|n| format!("Content-Length: {n}\r\n")).unwrap_or_default();
    stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: t3\r\nConnection: close\r\n{length}\r\n").as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut all = Vec::new();
    stream.read_to_end(&mut all).unwrap();
    let at = all.windows(4).position(|w| w == b"\r\n\r\n").expect("a response head");
    let head = String::from_utf8_lossy(&all[..at]).into_owned();
    Answer { status: head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap(), head, body: all[at + 4..].to_vec() }
}

fn upload_url(rpc: &mut Rpc, kind: &str, name: &str, mime: &str, size: usize) -> Value {
    success(&rpc.call("attachments.createUploadUrl", json!({"type": kind, "name": name, "mimeType": mime, "sizeBytes": size}))).clone()
}

#[test]
fn an_upload_url_takes_exactly_the_bytes_it_was_made_for_once_and_hands_them_to_the_agent_api_whole() {
    // Given a paired phone and an image it will upload
    let (h, mut rpc, kept) = started();
    let bytes: Vec<u8> = (0..=200u8).collect();
    // When it asks for an upload URL
    let made = upload_url(&mut rpc, "image", "shot.png", "image/png", bytes.len());
    // Then the id is a pending one, the URL is on this server, and it expires in ten minutes
    let (id, url) = (made["attachmentId"].as_str().unwrap().to_string(), made["relativeUrl"].as_str().unwrap().to_string());
    assert!(id.starts_with("pending-") && url.starts_with("/api/attachments/upload/"), "{made}");
    assert_eq!(made["expiresAt"], json!(START_MS + 600_000));
    assert!(asked(&h).is_empty(), "making the URL asks the agent API nothing");
    // When a body of another size is posted, or one that names no length and sends none, Then it is 400 with the reason and nothing is kept
    let text = |a: &Answer| (a.status, String::from_utf8_lossy(&a.body).into_owned());
    assert_eq!(text(&raw(&h, "POST", &url, &bytes[..10])), (400, "Content-Length must match the upload size.".to_string()));
    assert_eq!(text(&raw_as(&h, "POST", &url, None, b"")), (400, "Body was 0 bytes, expected 201.".to_string()));
    assert!(asked(&h).is_empty());
    // When the bytes are posted, Then it is 204 and the agent API was given them whole, under the id, with the name and type the URL was made for
    let done = raw(&h, "POST", &url, &bytes);
    assert_eq!(done.status, 204, "{}", done.head);
    let puts = asked(&h);
    assert_eq!(puts.len(), 1);
    assert_eq!((puts[0].0.as_str(), puts[0].1["id"].clone(), puts[0].1["name"].clone(), puts[0].1["mime_type"].clone(), puts[0].1["kind"].clone()), ("upload.put", json!(id), json!("shot.png"), json!("image/png"), json!("image")));
    assert_eq!(kept.lock().unwrap()[&id].1, bytes);
    // When the same URL is used again, Then it is 409 and the bytes are as they were
    assert_eq!(raw(&h, "POST", &url, &bytes).status, 409);
    assert_eq!(kept.lock().unwrap()[&id].1, bytes);
}

#[test]
fn an_upload_url_that_was_altered_expired_or_never_made_is_404_and_asks_the_agent_api_nothing() {
    // Given an upload URL
    let (h, mut rpc, _kept) = started();
    let made = upload_url(&mut rpc, "file", "notes.txt", "text/plain", 3);
    let url = made["relativeUrl"].as_str().unwrap().to_string();
    let (payload, signature) = url.rsplit_once('/').unwrap().1.split_once('.').unwrap();
    let flipped = |text: &str| -> String {
        let mut chars: Vec<char> = text.chars().collect();
        chars[3] = if chars[3] == 'A' { 'B' } else { 'A' };
        chars.into_iter().collect()
    };
    // When it is posted with its signature altered, its payload altered, a signature of nothing, and a made-up token
    let attempts = [format!("/api/attachments/upload/{payload}.{}", flipped(signature)), format!("/api/attachments/upload/{}.{signature}", flipped(payload)), format!("/api/attachments/upload/{payload}."), "/api/attachments/upload/abc.def".to_string(), "/api/attachments/upload/".to_string()];
    // Then each is 404 and nothing is asked
    for path in &attempts {
        assert_eq!(raw(&h, "POST", path, b"abc").status, 404, "{path}");
    }
    // And after the ten minutes the genuine URL is 404 too, and before them it was let in
    h.clock.advance_ms(600_000 - 1);
    assert_eq!(raw(&h, "POST", &url, b"abc").status, 204);
    let again = upload_url(&mut rpc, "file", "notes.txt", "text/plain", 3);
    h.clock.advance_ms(600_000);
    assert_eq!(raw(&h, "POST", again["relativeUrl"].as_str().unwrap(), b"abc").status, 404);
    assert_eq!(asked(&h).len(), 1);
}

#[test]
fn upload_urls_are_refused_for_what_the_pin_refuses_and_for_what_this_server_cannot_keep_and_the_most_it_keeps_is_let_through() {
    // Given a paired phone, and the most bytes the agent API keeps in one attachment
    let (h, mut rpc, _kept) = started();
    let most = agent_api_attachments::MAX_BYTES as usize;
    // When it asks for a URL for a type that is no image, an image of 11 MiB, an empty file, and a file one byte past what the agent API keeps
    let kind = rpc.call("attachments.createUploadUrl", json!({"type": "image", "name": "a.svg", "mimeType": "image/svg+xml", "sizeBytes": 10}));
    let big_image = rpc.call("attachments.createUploadUrl", json!({"type": "image", "name": "a.png", "mimeType": "image/png", "sizeBytes": 11 << 20}));
    let empty = rpc.call("attachments.createUploadUrl", json!({"type": "file", "name": "a.txt", "mimeType": "text/plain", "sizeBytes": 0}));
    let huge = rpc.call("attachments.createUploadUrl", json!({"type": "file", "name": "a.bin", "mimeType": "application/octet-stream", "sizeBytes": most + 1}));
    // Then the first three are the decoder's defects naming the field, and the last is a refusal that says what this server keeps
    for (exit, field) in [(&kind, "mimeType"), (&big_image, "sizeBytes"), (&empty, "sizeBytes")] {
        assert_eq!(exit["cause"][0]["_tag"], "Die", "{exit}");
        assert!(exit["cause"][0]["defect"].as_str().unwrap().contains(&format!("[\"{field}\"]")), "{exit}");
    }
    assert_eq!(failure(&huge)["_tag"], "UnsupportedOperationError");
    assert!(failure(&huge)["message"].as_str().unwrap().contains(&most.to_string()), "{huge}");
    assert!(asked(&h).is_empty());
    // When it asks for a file of exactly that many bytes, Then it gets a URL
    let at_most = rpc.call("attachments.createUploadUrl", json!({"type": "file", "name": "a.bin", "mimeType": "application/octet-stream", "sizeBytes": most}));
    assert!(success(&at_most)["relativeUrl"].is_string(), "{at_most}");
}

#[test]
fn an_asset_url_for_an_attachment_streams_its_bytes_in_chunks_with_its_type_and_every_other_resource_or_token_is_refused() {
    // Given an attachment the agent API keeps, longer than one chunk of the double
    let (h, mut rpc, kept) = started();
    let bytes = b"the attached note, longer than five bytes".to_vec();
    let id = {
        let made = upload_url(&mut rpc, "file", "../notes.txt", "text/plain", bytes.len());
        assert_eq!(raw(&h, "POST", made["relativeUrl"].as_str().unwrap(), &bytes).status, 204);
        made["attachmentId"].as_str().unwrap().to_string()
    };
    // When the phone asks for an asset URL, and fetches it
    let made = success(&rpc.call("assets.createUrl", json!({"resource": {"_tag": "attachment", "attachmentId": id}}))).clone();
    let url = made["relativeUrl"].as_str().unwrap().to_string();
    assert_eq!((made["expiresAt"].clone(), made["fileRevision"].clone()), (json!(START_MS + 3_600_000), json!(format!("sha-{}", bytes.len()))));
    assert!(url.starts_with("/api/assets/") && url.ends_with("/..%2Fnotes.txt"), "{url}");
    let got = raw(&h, "GET", &url, b"");
    // Then the bytes are all there, though they came in chunks, with the stored type, a length, and a download disposition that carries no path
    assert_eq!((got.status, got.body.clone()), (200, bytes.clone()), "{}", got.head);
    let head = got.head.to_ascii_lowercase();
    assert!(head.contains("content-type: text/plain") && head.contains(&format!("content-length: {}", bytes.len())) && head.contains("content-disposition: attachment") && head.contains("x-content-type-options: nosniff"), "{head}");
    assert!(head.contains("filename=\".._notes.txt\""), "{head}");
    // When the resource is not an attachment, Then it is refused with the reason and the agent API is asked nothing more
    let before = asked(&h).len();
    let other = rpc.call("assets.createUrl", json!({"resource": {"_tag": "workspace-file", "threadId": "t1", "path": "a.txt"}}));
    assert_eq!(failure(&other)["_tag"], "UnsupportedOperationError");
    assert!(failure(&other)["message"].as_str().unwrap().contains("attachment assets only"), "{other}");
    assert_eq!(asked(&h).len(), before);
    // When the attachment is not there, Then it is the pin's AssetAttachmentNotFoundError
    let missing = rpc.call("assets.createUrl", json!({"resource": {"_tag": "attachment", "attachmentId": "nope"}}));
    assert_eq!((failure(&missing)["_tag"].clone(), failure(&missing)["resource"]["attachmentId"].clone()), (json!("AssetAttachmentNotFoundError"), json!("nope")));
    // And an altered token, an upload token used as an asset token, and a URL after its hour are 404; an attachment deleted since is 404
    let token = url["/api/assets/".len()..].split('/').next().unwrap().to_string();
    let altered = format!("/api/assets/{}A/x", token);
    assert_eq!(raw(&h, "GET", &altered, b"").status, 404);
    let upload = upload_url(&mut rpc, "file", "x.txt", "text/plain", 1);
    let swapped = format!("/api/assets/{}/x", upload["relativeUrl"].as_str().unwrap().rsplit('/').next().unwrap());
    assert_eq!(raw(&h, "GET", &swapped, b"").status, 404);
    assert_eq!(raw(&h, "GET", "/api/assets/nodot/x", b"").status, 404);
    kept.lock().unwrap().remove(&id);
    assert_eq!(raw(&h, "GET", &url, b"").status, 404);
    h.clock.advance_ms(3_600_000);
    assert_eq!(raw(&h, "GET", &url, b"").status, 404);
}

#[test]
fn an_inline_asset_is_inline_only_for_a_safe_image_and_a_pending_attachment_is_deleted_but_a_kept_one_is_not() {
    // Given an image and a text file kept by the agent API
    let (h, mut rpc, kept) = started();
    let mut ids = Vec::new();
    for (name, mime, kind) in [("a.png", "image/png", "image"), ("a.html", "text/html", "file")] {
        let made = upload_url(&mut rpc, kind, name, mime, 4);
        assert_eq!(raw(&h, "POST", made["relativeUrl"].as_str().unwrap(), b"1234").status, 204);
        ids.push(made["attachmentId"].as_str().unwrap().to_string());
    }
    // When each is asked for inline
    let mut dispositions = Vec::new();
    for id in &ids {
        let made = success(&rpc.call("assets.createUrl", json!({"resource": {"_tag": "attachment", "attachmentId": id, "disposition": "inline"}}))).clone();
        dispositions.push(raw(&h, "GET", made["relativeUrl"].as_str().unwrap(), b"").head.to_ascii_lowercase());
    }
    // Then the image is inline and the HTML is a download
    assert!(dispositions[0].contains("content-disposition: inline") && dispositions[1].contains("content-disposition: attachment"), "{dispositions:?}");
    // When the pending image is deleted, Then the agent API is asked to drop it and it is gone
    success(&rpc.call("attachments.delete", json!({"attachmentId": ids[0]})));
    assert!(asked(&h).iter().any(|(m, p)| m == "attachment.delete" && p["id"] == json!(ids[0])));
    assert!(!kept.lock().unwrap().contains_key(&ids[0]));
    // When an id that is no pending upload is deleted, Then it succeeds and the agent API is asked nothing
    let before = asked(&h).len();
    success(&rpc.call("attachments.delete", json!({"attachmentId": "att-persisted"})));
    assert_eq!(asked(&h).len(), before);
}

#[test]
fn chat_attachments_sent_as_data_urls_are_kept_under_ids_and_answered_as_the_messages_attachments() {
    // Given a paired phone and an image as a data URL
    let (h, mut rpc, kept) = started();
    let bytes = vec![137u8, 80, 78, 71, 1, 2, 3];
    let url = format!("data:image/png;base64,{}", STANDARD.encode(&bytes));
    // When it persists the image with an id of its own and one without
    let sent = |id: Option<&str>, url: &str, size: usize, mime: &str| {
        let mut upload = json!({"type": "image", "name": "shot.png", "mimeType": mime, "sizeBytes": size, "dataUrl": url});
        if let Some(id) = id {
            upload["id"] = json!(id);
        }
        json!({"threadId": "t1", "messageId": "m1", "attachments": [upload]})
    };
    let kept_with_id = success(&rpc.call("assets.persistChatAttachments", sent(Some("mine"), &url, bytes.len(), "image/png"))).clone();
    let kept_without = success(&rpc.call("assets.persistChatAttachments", sent(None, &url, bytes.len(), "image/png"))).clone();
    // Then each is a ChatImageAttachment, the first under the id given and the second under one this server made, and the bytes are the agent API's
    assert_eq!(kept_with_id["attachments"][0], json!({"type": "image", "id": "mine", "name": "shot.png", "mimeType": "image/png", "sizeBytes": 7}));
    let made = kept_without["attachments"][0]["id"].as_str().unwrap().to_string();
    assert!(made.starts_with("att-"));
    let held: Vec<Vec<u8>> = {
        let store = kept.lock().unwrap();
        vec![store["mine"].1.clone(), store[&made].1.clone()]
    };
    assert_eq!(held, vec![bytes.clone(), bytes.clone()]);
    // When the size is not the data's, the data is no base64, or the type is no image, Then each is the pin's PersistChatAttachmentsError and nothing more is kept
    let before = asked(&h).len();
    for (bad, says) in [(sent(None, &url, 8, "image/png"), "holds 7 bytes"), (sent(None, "data:image/png;base64,***", 3, "image/png"), "not base64"), (sent(None, "plain text", 3, "image/png"), "not a base64 data URL"), (sent(None, &url, 7, "text/plain"), "only images")] {
        let exit = rpc.call("assets.persistChatAttachments", bad);
        assert_eq!(failure(&exit)["_tag"], "PersistChatAttachmentsError", "{exit}");
        assert!(failure(&exit)["message"].as_str().unwrap().contains(says), "{exit}");
    }
    assert_eq!(asked(&h).len(), before);
}

#[test]
fn a_message_with_attachments_carries_their_ids_into_the_turn_and_shows_them_on_its_row_and_a_malformed_one_is_refused() {
    // Given a thread and two attachments the phone uploaded
    let (h, mut rpc, _kept) = started();
    let image = json!({"type": "image", "id": "pending-a1", "name": "shot.png", "mimeType": "image/png", "sizeBytes": 4});
    let file = json!({"type": "file", "id": "pending-b2", "name": "notes.txt", "mimeType": "text/plain", "sizeBytes": 9});
    // When a message naming them is dispatched
    let exit = rpc.call("orchestration.dispatchCommand", json!({"type": "message.dispatch", "commandId": "c-1", "threadId": "t1", "message": {"messageId": "m-1", "text": "look at these", "attachments": [image, file]}}));
    success(&exit);
    let projection = settled(&mut rpc, "t1");
    // Then the turn was started with their ids in order, and the message's row shows them as the phone sent them
    let turns = h.agent.turns.lock().unwrap().clone();
    assert_eq!((turns.len(), turns[0].prompt.as_str(), turns[0].attachments.clone()), (1, "look at these", vec!["pending-a1".to_string(), "pending-b2".to_string()]));
    let row = projection["messages"].as_array().unwrap().iter().find(|m| m["id"] == "m-1").expect("the message row").clone();
    assert_eq!(row["attachments"], json!([image, file]));
    // When a message names an attachment with no usable id, or too many, Then the command is refused and no turn starts
    let bad = rpc.call("orchestration.dispatchCommand", json!({"type": "message.dispatch", "commandId": "c-2", "threadId": "t1", "message": {"messageId": "m-2", "text": "x", "attachments": [{"type": "image", "id": "no spaces allowed", "name": "a", "mimeType": "image/png", "sizeBytes": 1}]}}));
    assert!(failure(&bad).to_string().contains("every attachment needs an id"), "{bad}");
    let many: Vec<Value> = (0..101).map(|i| json!({"type": "file", "id": format!("f{i}"), "name": "a", "mimeType": "text/plain", "sizeBytes": 1})).collect();
    let too_many = rpc.call("orchestration.dispatchCommand", json!({"type": "message.dispatch", "commandId": "c-3", "threadId": "t1", "message": {"messageId": "m-3", "text": "x", "attachments": many}}));
    assert!(failure(&too_many).to_string().contains("up to 100 files"), "{too_many}");
    assert_eq!(h.agent.turns.lock().unwrap().len(), 1);
}
