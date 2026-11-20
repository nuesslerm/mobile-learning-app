//! `voice.*` through the real router and HTTP routes over an agent double that offers the service's speech engine (or none): what each route
//! checks before it asks the agent API anything, the model shape, the one WAV `speech.transcribe` receives and the refusals of a bad recording.
use super::support::*;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use t3_core::ports::{AgentError, Op};

type Heard = Arc<Mutex<Vec<Value>>>;

fn all_scopes() -> PairingRequest {
    PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() }
}

/// A server whose service has a speech engine that hears `text`, or none; with the `speech.transcribe` params it was sent.
fn started(engine: Option<&'static str>) -> (Harness, String, Heard) {
    let h = Harness::start();
    let heard: Heard = Arc::default();
    let kept = heard.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| {
        let Op::Speech { method, params } = op else { return None };
        Some(match (*method, engine) {
            ("speech.status", Some(_)) => Ok(json!({"available": true, "engine": "whisper"})),
            ("speech.status", None) => Ok(json!({"available": false})),
            (_, Some(text)) => {
                kept.lock().unwrap().push(params.clone());
                Ok(json!({"text": format!("  {text}\n")}))
            }
            (_, None) => Err(AgentError::Refused { code: "SPEECH_UNAVAILABLE".into(), message: "no engine".into() }),
        })
    }));
    let bearer = bearer_of(h.addr, &h.pair(all_scopes()).code);
    (h, bearer, heard)
}

fn get(h: &Harness, path: &str, bearer: Option<&str>) -> Reply {
    let auth = bearer.map(|b| format!("Bearer {b}"));
    let headers: Vec<(&str, &str)> = auth.as_deref().map(|a| ("Authorization", a)).into_iter().collect();
    http(h.addr, "GET", path, &headers, "")
}

fn post_json(h: &Harness, path: &str, bearer: Option<&str>, body: &str) -> Reply {
    let auth = bearer.map(|b| format!("Bearer {b}"));
    let mut headers = vec![("Content-Type", "application/json")];
    if let Some(a) = auth.as_deref() {
        headers.push(("Authorization", a));
    }
    http(h.addr, "POST", path, &headers, body)
}

fn asked(h: &Harness, method: &str) -> usize {
    h.agent.ops.lock().unwrap().iter().filter(|op| matches!(op, Op::Speech { method: m, .. } if *m == method)).count()
}

fn options() -> Value {
    json!({"speechLanguage": "en", "speechTranslateToEnglish": false, "speechCustomWords": [], "speechRemoveFillerWords": true})
}

/// The body of a transcribe request: the options' length, the options, then the samples as little-endian Float32.
fn recording(options: &Value, samples: &[f32]) -> Vec<u8> {
    let text = options.to_string();
    let mut body = (text.len() as u32).to_le_bytes().to_vec();
    body.extend(text.as_bytes());
    body.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
    body
}

fn transcribe(h: &Harness, bearer: Option<&str>, content_type: &str, body: &[u8]) -> Reply {
    let mut stream = std::net::TcpStream::connect(h.addr).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let auth = bearer.map(|b| format!("Authorization: Bearer {b}\r\n")).unwrap_or_default();
    stream.write_all(format!("POST /api/voice/transcribe HTTP/1.1\r\nHost: t3\r\nConnection: close\r\nContent-Type: {content_type}\r\n{auth}Content-Length: {}\r\n\r\n", body.len()).as_bytes()).unwrap();
    let _ = stream.write_all(body);
    let mut all = Vec::new();
    let _ = stream.read_to_end(&mut all);
    let at = all.windows(4).position(|w| w == b"\r\n\r\n").expect("a response head");
    let head = String::from_utf8_lossy(&all[..at]).to_ascii_lowercase();
    Reply { status: head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap(), head, body: serde_json::from_slice(&all[at + 4..]).unwrap_or(Value::Null) }
}

fn reason(answer: &Reply) -> Option<&str> {
    answer.body["reason"].as_str()
}

#[test]
fn with_an_engine_the_status_and_the_model_list_show_the_one_ready_rca_model() {
    // Given a service with a speech engine and a bearer
    let (h, bearer, _) = started(Some("hello"));
    // When the phone reads the status and the model list
    let status = get(&h, "/api/voice/status", Some(&bearer));
    let models = get(&h, "/api/voice/models", Some(&bearer));
    let benchmarked = get(&h, "/api/voice/models?benchmarks=1", Some(&bearer));
    // Then the status is ready with the engine's model, and the list holds that model alone, installed and active
    assert_eq!((status.status, status.body["supported"].clone(), status.body["state"].clone(), status.body["modelId"].clone()), (200, json!(true), json!("ready"), json!("rca-whisper")), "{:?}", status.body);
    assert_eq!(models.status, 200);
    let rows = models.body["models"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{:?}", models.body);
    assert_eq!((rows[0]["id"].clone(), rows[0]["state"].clone(), rows[0]["active"].clone(), rows[0]["size"].clone()), (json!("rca-whisper"), json!("installed"), json!(true), json!(0)));
    assert_eq!(benchmarked.body, models.body);
    // And the service was asked its status and nothing was transcribed
    assert_eq!(asked(&h, "speech.transcribe"), 0);
}

#[test]
fn without_an_engine_the_status_says_unsupported_and_the_model_list_is_empty() {
    // Given a service with no speech engine
    let (h, bearer, _) = started(None);
    // When the phone reads the status and the model list
    let status = get(&h, "/api/voice/status", Some(&bearer));
    let models = get(&h, "/api/voice/models", Some(&bearer));
    // Then the status is unsupported with a reason and no model is listed
    assert_eq!((status.status, status.body["supported"].clone()), (200, json!(false)), "{:?}", status.body);
    assert!(status.body["reason"].as_str().is_some_and(|r| !r.is_empty()), "{:?}", status.body);
    assert_eq!((models.status, models.body), (200, json!({"models": []})));
}

#[test]
fn a_benchmarks_query_other_than_one_is_a_bare_400_before_the_agent_is_asked() {
    // Given a service with an engine
    let (h, bearer, _) = started(Some("hello"));
    // When the model list is read with benchmarks=2
    let refused = get(&h, "/api/voice/models?benchmarks=2", Some(&bearer));
    // Then it is a 400 with no body, as the pin's schema answers
    assert_eq!((refused.status, refused.body), (400, Value::Null));
    assert_eq!(asked(&h, "speech.status"), 0);
}

#[test]
fn selecting_the_one_model_changes_nothing_and_any_other_id_is_invalid_command() {
    // Given a service with an engine
    let (h, bearer, _) = started(Some("hello"));
    // When the engine's model is selected, another id is selected, and a body without a model id is posted
    let same = post_json(&h, "/api/voice/models/select", Some(&bearer), r#"{"modelId":"rca-whisper"}"#);
    let other = post_json(&h, "/api/voice/models/select", Some(&bearer), r#"{"modelId":"tiny.en"}"#);
    let bare = post_json(&h, "/api/voice/models/select", Some(&bearer), r#"{"model":"rca-whisper"}"#);
    // Then the first answers the model list, the second is a 400 invalid_command, the third a bare 400
    assert_eq!((same.status, same.body["models"][0]["id"].clone()), (200, json!("rca-whisper")), "{:?}", same.body);
    assert_eq!((other.status, reason(&other)), (400, Some("invalid_command")), "{:?}", other.body);
    assert_eq!((bare.status, bare.body), (400, Value::Null));
}

#[test]
fn downloading_the_installed_model_is_done_already_and_without_an_engine_nothing_downloads() {
    // Given a service with an engine, and one without
    let (h, bearer, _) = started(Some("hello"));
    let (none, none_bearer, _) = started(None);
    // When the engine's model, another id, and (without an engine) the engine's model are downloaded
    let done = post_json(&h, "/api/voice/models/download", Some(&bearer), r#"{"modelId":"rca-whisper"}"#);
    let other = post_json(&h, "/api/voice/models/download", Some(&bearer), r#"{"modelId":"tiny.en"}"#);
    let unavailable = post_json(&none, "/api/voice/models/download", Some(&none_bearer), r#"{"modelId":"rca-whisper"}"#);
    // Then the first lists the model, the second is invalid_command, and the third speech_unavailable
    assert_eq!((done.status, done.body["models"][0]["state"].clone()), (200, json!("installed")), "{:?}", done.body);
    assert_eq!((other.status, reason(&other)), (400, Some("invalid_command")));
    assert_eq!((unavailable.status, reason(&unavailable)), (400, Some("speech_unavailable")));
}

#[test]
fn cancelling_a_download_answers_the_list_for_any_id_and_refuses_a_body_without_one() {
    // Given a service with an engine, and one without
    let (h, bearer, _) = started(Some("hello"));
    let (none, none_bearer, _) = started(None);
    // When a download is cancelled for the engine's id, for an unknown id, with no engine, and with no id
    let known = post_json(&h, "/api/voice/models/cancel", Some(&bearer), r#"{"modelId":"rca-whisper"}"#);
    let unknown = post_json(&h, "/api/voice/models/cancel", Some(&bearer), r#"{"modelId":"nope"}"#);
    let off = post_json(&none, "/api/voice/models/cancel", Some(&none_bearer), r#"{"modelId":"rca-whisper"}"#);
    let bare = post_json(&h, "/api/voice/models/cancel", Some(&bearer), "{}");
    // Then the first two answer the list with the model, the third an empty list, and the last a bare 400
    assert_eq!((known.status, known.body["models"].as_array().map(Vec::len)), (200, Some(1)));
    assert_eq!((unknown.status, unknown.body), (200, known.body.clone()));
    assert_eq!((off.status, off.body), (200, json!({"models": []})));
    assert_eq!((bare.status, bare.body), (400, Value::Null));
}

#[test]
fn every_route_is_401_without_a_bearer_or_with_a_bad_one_and_403_without_its_scope() {
    // Given a service with an engine, a bearer that can only read, and one of the default scopes
    let (h, _, _) = started(Some("hello"));
    let read_only = bearer_of(h.addr, &h.pair(PairingRequest { scopes: vec!["orchestration:read".into()], ..PairingRequest::default() }).code);
    let default = bearer_of(h.addr, &h.pair(PairingRequest::default()).code);
    let sound = recording(&options(), &[0.0; 4]);
    // When each route is called with no bearer, a made-up bearer, the read-only bearer, and (for the settings routes) the default bearer
    for path in ["/api/voice/status", "/api/voice/models"] {
        // Then the read routes need orchestration:operate
        assert_eq!(get(&h, path, None).status, 401, "{path}");
        assert_eq!(get(&h, path, Some("made-up")).status, 401, "{path}");
        let low = get(&h, path, Some(&read_only));
        assert_eq!((low.status, low.body["requiredPermission"].clone()), (403, json!("orchestration:operate")), "{path} {:?}", low.body);
    }
    for path in ["/api/voice/models/select", "/api/voice/models/download", "/api/voice/models/cancel"] {
        // And the model routes need settings:write, which the default pairing lacks
        let body = r#"{"modelId":"rca-whisper"}"#;
        assert_eq!(post_json(&h, path, None, body).status, 401, "{path}");
        assert_eq!(post_json(&h, path, Some("made-up"), body).status, 401, "{path}");
        assert_eq!(post_json(&h, path, Some(&default), body).status, 403, "{path}");
    }
    // And transcribe needs orchestration:operate
    assert_eq!(transcribe(&h, None, "application/octet-stream", &sound).status, 401);
    assert_eq!(transcribe(&h, Some("made-up"), "application/octet-stream", &sound).status, 401);
    assert_eq!(transcribe(&h, Some(&read_only), "application/octet-stream", &sound).status, 403);
    // And the engine was never asked to transcribe
    assert_eq!(asked(&h, "speech.transcribe"), 0);
}

#[test]
fn a_json_route_given_another_content_type_and_transcribe_given_json_are_415() {
    // Given a service with an engine
    let (h, bearer, _) = started(Some("hello"));
    let auth = format!("Bearer {bearer}");
    // When a model route is posted as text and transcribe as JSON
    let select = http(h.addr, "POST", "/api/voice/models/select", &[("Authorization", &auth), ("Content-Type", "text/plain")], r#"{"modelId":"rca-whisper"}"#);
    let sound = transcribe(&h, Some(&bearer), "application/json", &recording(&options(), &[0.0; 4]));
    // Then both are 415 and the engine heard nothing
    assert_eq!((select.status, sound.status), (415, 415));
    assert_eq!(asked(&h, "speech.transcribe"), 0);
}

#[test]
fn a_recording_reaches_the_engine_as_one_16khz_mono_wav_of_its_samples_and_its_text_comes_back_trimmed() {
    // Given a service whose engine hears " hello " and a recording of four samples
    let (h, bearer, heard) = started(Some("hello"));
    let samples = [0.0f32, 0.5, -0.5, 1.0];
    // When the recording is posted
    let answer = transcribe(&h, Some(&bearer), "application/octet-stream", &recording(&options(), &samples));
    // Then the phone gets the words without the engine's padding
    assert_eq!((answer.status, answer.body.clone()), (200, json!({"text": "hello"})), "{}", answer.head);
    // And the engine got one call with a WAV of format wav: RIFF header, PCM mono 16 kHz 16-bit, and the samples as 16-bit integers
    let sent = heard.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["format"], "wav");
    let wav = STANDARD.decode(sent[0]["audio"].as_str().unwrap()).unwrap();
    assert_eq!((&wav[0..4], &wav[8..16]), (&b"RIFF"[..], &b"WAVEfmt "[..]));
    let field = |at: usize, n: usize| wav[at..at + n].iter().rev().fold(0usize, |acc, b| acc << 8 | *b as usize);
    assert_eq!((field(20, 2), field(22, 2), field(24, 4), field(34, 2), field(40, 4)), (1, 1, 16_000, 16, 8));
    let pcm: Vec<i16> = wav[44..].chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    assert_eq!(pcm, [0, 16384, -16384, 32767]);
    assert_eq!(field(4, 4), wav.len() - 8);
}

#[test]
fn a_recording_the_pin_would_refuse_is_invalid_audio_and_the_engine_is_not_asked() {
    // Given a service with an engine
    let (h, bearer, _) = started(Some("hello"));
    let good = options();
    let mut truncated = recording(&good, &[0.0; 4]);
    truncated.pop();
    let with = |key: &str, value: Value| {
        let mut o = good.clone();
        o[key] = value;
        o
    };
    let nan = f32::NAN;
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("no body", Vec::new()),
        ("an options length with nothing after it", 5u32.to_le_bytes().to_vec()),
        ("options and no audio", recording(&good, &[])),
        ("options that are not JSON", [&3u32.to_le_bytes()[..], b"{x}", &[0u8; 4]].concat()),
        ("a length past the body", [&900u32.to_le_bytes()[..], b"{}", &[0u8; 4]].concat()),
        ("a sample that is cut short", truncated),
        ("a sample past 1", recording(&good, &[0.0, 1.5])),
        ("a sample that is not a number", recording(&good, &[0.0, nan])),
        ("a language that is too short", recording(&with("speechLanguage", json!("e")), &[0.0; 4])),
        ("a switch that is not a boolean", recording(&with("speechRemoveFillerWords", json!("yes")), &[0.0; 4])),
        ("a dictionary that repeats a spelling", recording(&with("speechCustomWords", json!([{"term": "Rca", "aliases": ["rca"]}])), &[0.0; 4])),
    ];
    // When each is posted, Then it is a 400 invalid_audio
    for (name, body) in cases {
        let answer = transcribe(&h, Some(&bearer), "application/octet-stream", &body);
        assert_eq!((answer.status, reason(&answer)), (400, Some("invalid_audio")), "{name}: {:?}", answer.body);
    }
    // And a valid dictionary with a different alias is let through
    let spelled = recording(&with("speechCustomWords", json!([{"term": "Rca", "aliases": ["arka"]}])), &[0.0; 4]);
    assert_eq!(transcribe(&h, Some(&bearer), "application/octet-stream", &spelled).status, 200);
    assert_eq!(asked(&h, "speech.transcribe"), 1, "only the valid recording reached the engine");
}

#[test]
fn a_body_over_the_limit_is_invalid_audio_behind_the_bearer_and_never_read() {
    // Given a service with an engine and a body one byte past the most a recording holds
    let (h, bearer, _) = started(Some("hello"));
    let over = 16_000 * 4 * 300 + 128_000 + 4 + 1;
    // When it is posted with a bearer and without one (only the head is sent: the server does not read the body)
    let head = |auth: &str| format!("POST /api/voice/transcribe HTTP/1.1\r\nHost: t3\r\nConnection: close\r\nContent-Type: application/octet-stream\r\n{auth}Content-Length: {over}\r\n\r\n");
    let ask = |auth: String| {
        let mut stream = std::net::TcpStream::connect(h.addr).unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        stream.write_all(head(&auth).as_bytes()).unwrap();
        let mut all = String::new();
        let _ = stream.read_to_string(&mut all);
        all
    };
    let with_bearer = ask(format!("Authorization: Bearer {bearer}\r\n"));
    let without = ask(String::new());
    // Then the first is a 400 invalid_audio and the second a 401
    assert!(with_bearer.starts_with("HTTP/1.1 400") && with_bearer.contains("invalid_audio"), "{with_bearer}");
    assert!(without.starts_with("HTTP/1.1 401"), "{without}");
    assert_eq!(asked(&h, "speech.transcribe"), 0);
}

#[test]
fn a_recording_is_speech_unavailable_without_an_engine_and_internal_when_the_engine_fails() {
    // Given a service with no engine, and one whose engine fails on a recording
    let (none, none_bearer, _) = started(None);
    let (failing, failing_bearer, _) = started(Some("x"));
    *failing.agent.responder.lock().unwrap() = Some(Box::new(|op| {
        let Op::Speech { method, .. } = op else { return None };
        Some(if *method == "speech.status" { Ok(json!({"available": true})) } else { Err(AgentError::Failed("sidecar down".into())) })
    }));
    let body = recording(&options(), &[0.0; 4]);
    // When a valid recording is posted to each
    let off = transcribe(&none, Some(&none_bearer), "application/octet-stream", &body);
    let broken = transcribe(&failing, Some(&failing_bearer), "application/octet-stream", &body);
    // Then the first is speech_unavailable and the second a 500 internal error that names no cause
    assert_eq!((off.status, reason(&off)), (400, Some("speech_unavailable")), "{:?}", off.body);
    assert_eq!((broken.status, reason(&broken)), (500, Some("internal_error")), "{:?}", broken.body);
    assert_eq!(asked(&none, "speech.transcribe"), 0);
}
