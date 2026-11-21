//! Dictation through the chain: the phone lists the speech model, sets dictation up, streams PCM and finishes, and the real rca-service sends one
//! recording to its speech engine (here a fake HTTP sidecar named by `STT_URL`) and the text comes back. A row counts only when `speech.status` or
//! `speech.transcribe` answered in its call's window.
use crate::agent_sessions::answered;
use crate::rig::*;
use base64::Engine;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// A speech sidecar that answers every recording with `text`, keeping the length of each request body it was sent.
struct Sidecar {
    url: String,
    bodies: Arc<Mutex<Vec<usize>>>,
}

fn sidecar(text: &'static str) -> Sidecar {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port for the sidecar");
    let url = format!("http://{}/transcribe", listener.local_addr().unwrap());
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = bodies.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
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
            seen.lock().unwrap().push(buffer.len() - head_end.unwrap_or(0));
            let body = json!({"text": text}).to_string();
            let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        }
    });
    Sidecar { url, bodies }
}

fn pcm(samples: usize) -> String {
    base64::engine::general_purpose::STANDARD.encode(vec![1u8; samples * 2])
}

#[test]
fn the_phone_lists_the_speech_model_sets_dictation_up_streams_audio_and_gets_the_text_the_services_engine_heard() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain with a speech engine at the service, and a paired phone
        let engine = sidecar("open the settings");
        let chain = Chain::start_with_env(None, &[("STT_URL", &engine.url)]);
        let mut phone = Phone::connect(case, &chain);
        // When the phone lists the speech models
        let listed = phone.call(case, &chain, &["rpc:speech.models.list"], "speech.models.list", json!({}));
        // Then the service's engine is the one ready model, and the service was asked its status
        answered(case, "speech.status");
        assert_eq!(listed["result"]["models"][0]["id"], "rca-whisper", "{listed}");
        assert_eq!(listed["result"]["models"][0]["status"], "ready", "{listed}");
        // And a dictation refused while it is off, set up, started, fed two chunks and finished
        let off = phone.refused(case, &chain, &["rpc:speech.dictation.start"], "speech.dictation.start", json!({"dictationId": "d1"}));
        assert_eq!(off["error"]["message"], "voice_dictation_disabled", "{off}");
        let set = phone.call(case, &chain, &["rpc:speech.dictation.setup"], "speech.dictation.setup", json!({"enabled": true, "modelId": "rca-whisper"}));
        assert_eq!(set["result"]["enabled"], true, "{set}");
        let started = phone.call(case, &chain, &["rpc:speech.dictation.start"], "speech.dictation.start", json!({"dictationId": "d1"}));
        assert_eq!(started["result"]["modelId"], "rca-whisper", "{started}");
        for seq in 0..2u64 {
            let fed = phone.call(case, &chain, &["rpc:speech.dictation.chunk"], "speech.dictation.chunk", json!({"dictationId": "d1", "audioBase64": pcm(1600), "sampleRate": 16000, "seq": seq}));
            assert_eq!(fed["result"]["dictationId"], "d1", "{fed}");
        }
        let done: Value = phone.call(case, &chain, &["rpc:speech.dictation.finish"], "speech.dictation.finish", json!({"dictationId": "d1"}));
        // Then the service transcribed once, and the phone has the words
        answered(case, "speech.transcribe");
        assert_eq!(done["result"], json!({"dictationId": "d1", "text": "open the settings"}), "{done}");
        assert_eq!(engine.bodies.lock().unwrap().len(), 1);
        // And a dictation that is cancelled sends nothing to the engine
        phone.call(case, &chain, &["rpc:speech.dictation.start"], "speech.dictation.start", json!({"dictationId": "d2"}));
        let cancelled = phone.call(case, &chain, &["rpc:speech.dictation.cancel"], "speech.dictation.cancel", json!({"dictationId": "d2"}));
        assert_eq!(cancelled["result"]["dictationId"], "d2", "{cancelled}");
        assert_eq!(engine.bodies.lock().unwrap().len(), 1);
        // And the one model cannot be deleted, and downloading it starts nothing to wait for
        let deleted = phone.refused(case, &chain, &["rpc:speech.models.delete"], "speech.models.delete", json!({"modelId": "rca-whisper"}));
        assert_eq!(deleted["error"]["message"], "voice_model_delete_failed", "{deleted}");
        let download = phone.call(case, &chain, &["rpc:speech.models.download"], "speech.models.download", json!({"modelId": "rca-whisper"}));
        assert_eq!(download["result"], json!({"started": true}), "{download}");
    });
}
