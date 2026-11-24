//! `voice.*` through the chain: the phone reads the speech model, selects it, posts a recording and gets the words the real rca-service's engine
//! heard (here a fake HTTP sidecar named by `STT_URL`), and without an engine the same routes say so. A call counts for its row only when
//! `speech.status` or `speech.transcribe` answered in its window.
use crate::chain::last_window;
use crate::rig::*;
use crate::support::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// A speech sidecar that answers every recording with `text`, keeping each request body it was sent.
struct Sidecar {
    url: String,
    bodies: Arc<Mutex<Vec<Vec<u8>>>>,
}

fn sidecar(text: &'static str) -> Sidecar {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port for the sidecar");
    let url = format!("http://{}/transcribe", listener.local_addr().unwrap());
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = bodies.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 8192];
            let (mut head_end, mut length) = (None, 0usize);
            loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..n]);
                if head_end.is_none() {
                    if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                        head_end = Some(at + 4);
                        let head = String::from_utf8_lossy(&buffer[..at]).to_lowercase();
                        length = head.lines().find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse().ok())).unwrap_or(0);
                    }
                }
                if head_end.is_some_and(|h| buffer.len() >= h + length) {
                    break;
                }
            }
            seen.lock().unwrap().push(buffer[head_end.unwrap_or(0)..].to_vec());
            let body = json!({"text": text}).to_string();
            let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        }
    });
    Sidecar { url, bodies }
}

fn all_scopes() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

fn answered(case: &exec_testkit::Case, method: &str) {
    let (agent, _) = last_window(case);
    assert!(agent.iter().any(|(m, ok)| m == method && *ok), "{method} did not answer in the call's window: {agent:?}");
}

fn recording(samples: &[f32]) -> Vec<u8> {
    let options = json!({"speechLanguage": "en", "speechTranslateToEnglish": false, "speechCustomWords": [], "speechRemoveFillerWords": true}).to_string();
    let mut body = (options.len() as u32).to_le_bytes().to_vec();
    body.extend(options.as_bytes());
    body.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
    body
}

fn post(chain: &Chain, phone: &Phone, path: &str, content_type: &str, body: &[u8]) -> Reply {
    let mut stream = std::net::TcpStream::connect(chain.addr).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(60))).unwrap();
    stream.write_all(format!("POST {path} HTTP/1.1\r\nHost: t3\r\nConnection: close\r\nAuthorization: {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n", phone.bearer, body.len()).as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut all = String::new();
    let _ = stream.read_to_string(&mut all);
    let (head, body) = all.split_once("\r\n\r\n").expect("a response head");
    Reply { status: head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap(), body: serde_json::from_str(body).unwrap_or(Value::Null), head: head.to_ascii_lowercase() }
}

fn authorized(phone: &Phone) -> [(&str, &str); 1] {
    [("Authorization", phone.bearer.as_str())]
}

#[test]
fn the_phone_reads_the_speech_model_selects_it_and_posts_a_recording_and_gets_the_text_the_services_engine_heard() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain with a speech engine at the service, and a phone paired with every scope
        let engine = sidecar("open the settings");
        let chain = Chain::start_with_env(&[("STT_URL", &engine.url)]);
        let phone = Phone::connect_with(case, &chain, &all_scopes());
        // When the phone reads the voice status and the model list
        let status = http_call(case, &chain, &["http:voice.status"], ("GET", "/api/voice/status", &authorized(&phone), ""), 200);
        answered(case, "speech.status");
        let models = http_call(case, &chain, &["http:voice.models"], ("GET", "/api/voice/models", &authorized(&phone), ""), 200);
        answered(case, "speech.status");
        // Then the service's engine is the one ready model
        assert_eq!((status.body["supported"].clone(), status.body["modelId"].clone()), (json!(true), json!("rca-whisper")), "{:?}", status.body);
        assert_eq!((models.body["models"][0]["id"].clone(), models.body["models"][0]["state"].clone()), (json!("rca-whisper"), json!("installed")), "{:?}", models.body);
        // And selecting it, downloading it and cancelling the download each answer the list
        let json_headers = |phone: &Phone| [("Authorization", phone.bearer.clone()), ("Content-Type", "application/json".to_string())];
        for (row, path) in [("http:voice.selectModel", "/api/voice/models/select"), ("http:voice.downloadModel", "/api/voice/models/download"), ("http:voice.cancelModelDownload", "/api/voice/models/cancel")] {
            let headers = json_headers(&phone);
            let borrowed: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let listed = http_call(case, &chain, &[row], ("POST", path, &borrowed, r#"{"modelId":"rca-whisper"}"#), 200);
            assert_eq!(listed.body["models"][0]["id"], "rca-whisper", "{row}: {:?}", listed.body);
        }
        // When a recording is posted
        let samples = [0.0f32, 0.25, -0.25, 0.5];
        let heard = case.call(&chain.tap, &rows(&["http:voice.transcribe"]), || {
            let reply = post(&chain, &phone, "/api/voice/transcribe", "application/octet-stream", &recording(&samples));
            (reply.status == 200, reply)
        });
        // Then the service transcribed it once and the phone has the words
        answered(case, "speech.transcribe");
        assert_eq!(heard.body, json!({"text": "open the settings"}), "{}", chain.evidence());
        let bodies = engine.bodies.lock().unwrap().clone();
        assert_eq!(bodies.len(), 1);
        // And the sidecar was sent a WAV file holding the samples as 16-bit integers
        let at = bodies[0].windows(4).position(|w| w == b"RIFF").expect("the sidecar was sent a WAV");
        let data = &bodies[0][at + 44..at + 44 + 8];
        assert_eq!(data.chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>(), [0, 8192, -8192, 16384]);
        // And a body that is no recording is refused as invalid audio without reaching the engine
        let invalid = post(&chain, &phone, "/api/voice/transcribe", "application/octet-stream", b"not a recording");
        assert_eq!((invalid.status, invalid.body["reason"].clone()), (400, json!("invalid_audio")), "{:?}", invalid.body);
        assert_eq!(engine.bodies.lock().unwrap().len(), 1);
    });
}

#[test]
fn an_engine_that_cannot_hear_the_recording_is_a_500_internal_error_that_names_no_cause() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain whose speech sidecar address answers nothing, and a paired phone
        let dead = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}/transcribe", listener.local_addr().unwrap())
        };
        let chain = Chain::start_with_env(&[("STT_URL", &dead)]);
        let phone = Phone::connect_with(case, &chain, &all_scopes());
        // When the phone posts a valid recording
        let refused = post(&chain, &phone, "/api/voice/transcribe", "application/octet-stream", &recording(&[0.0; 4]));
        // Then the service's engine failed on it and the phone gets a 500 with no cause in it
        assert_eq!((refused.status, refused.body["reason"].clone()), (500, json!("internal_error")), "{:?}\n{}", refused.body, chain.evidence());
        assert!(!refused.body.to_string().contains("127.0.0.1"), "{:?}", refused.body);
    });
}
