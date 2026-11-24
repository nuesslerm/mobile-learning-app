use crate::{resolve_engine, Engine, Speech};
use rca_service_core::application::settings::SttSettings;
use rca_service_core::testing::MapEnv;
use remote_chat_agent_core::application::ports::SpeechToText;
use remote_chat_agent_core::testing::{CommandsPort, FakeCommands, FakeOutbound, OutboundPort};
use serde_json::json;
use std::sync::Arc;

fn speech(dir: &std::path::Path) -> (Speech, Arc<FakeOutbound>, Arc<MapEnv>) {
    let outbound = FakeOutbound::new();
    let env = Arc::new(MapEnv::new(dir.to_path_buf(), None, dir.to_path_buf()));
    let speech = Speech::new(Arc::new(OutboundPort(outbound.clone())), Arc::new(CommandsPort(FakeCommands::new())), env.clone());
    (speech, outbound, env)
}

fn recording(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("clip.webm");
    std::fs::write(&path, b"audio").unwrap();
    path
}

#[test]
fn given_a_sidecar_url_when_the_engine_resolves_then_the_sidecar_wins_over_groq() {
    let stt = SttSettings { url: Some(" http://sidecar:9000/stt ".into()), engine: Some("groq".into()), groq_api_key: Some("k".into()), ..Default::default() };
    assert_eq!(resolve_engine(&stt), Engine::Sidecar("http://sidecar:9000/stt".into()));
}

#[test]
fn given_groq_with_a_key_when_the_engine_resolves_then_the_default_model_is_used() {
    let stt = SttSettings { engine: Some("groq".into()), groq_api_key: Some("k".into()), ..Default::default() };
    assert_eq!(resolve_engine(&stt), Engine::Groq { api_key: "k".into(), model: "whisper-large-v3-turbo".into() });
}

#[test]
fn given_groq_without_a_key_when_the_engine_resolves_then_whisper_cpp_is_used() {
    let stt = SttSettings { engine: Some("groq".into()), ..Default::default() };
    assert_eq!(resolve_engine(&stt), Engine::WhisperCpp);
}

#[tokio::test]
async fn given_a_sidecar_when_a_recording_is_transcribed_then_the_audio_is_posted_and_the_transcript_returned() {
    let dir = tempfile::tempdir().unwrap();
    let (speech, outbound, env) = speech(dir.path());
    env.set("STT_URL", "http://sidecar/stt");
    outbound.answer(|_| Ok((200, json!({ "transcript": " hello there ", "ms": 12, "threads": 2 }))));
    let result = speech.transcribe(&recording(dir.path())).await.unwrap();
    assert_eq!((result.transcript.as_str(), result.whisper_ms, result.threads, result.model_name.as_str()), ("hello there", 12, 2, "sidecar"));
    let posts = outbound.to("http://sidecar/stt");
    assert_eq!((posts.len(), posts[0].form[0].0.as_str(), posts[0].form[0].2.clone()), (1, "audio", b"audio".to_vec()));
}

#[tokio::test]
async fn given_groq_when_a_recording_is_transcribed_then_the_bearer_key_and_model_are_sent() {
    let dir = tempfile::tempdir().unwrap();
    let (speech, outbound, env) = speech(dir.path());
    env.set("STT_ENGINE", "groq");
    env.set("GROQ_API_KEY", "secret");
    outbound.answer(|_| Ok((200, json!({ "text": "hi" }))));
    let result = speech.transcribe(&recording(dir.path())).await.unwrap();
    assert_eq!(result.model_name, "groq:whisper-large-v3-turbo");
    let post = &outbound.posts()[0];
    assert_eq!(post.headers, vec![("Authorization".to_string(), "Bearer secret".to_string())]);
    assert!(post.form.iter().any(|(name, _, bytes)| name == "model" && bytes == b"whisper-large-v3-turbo"));
}

#[tokio::test]
async fn given_an_engine_error_status_when_transcribed_then_the_error_names_the_engine_and_status() {
    let dir = tempfile::tempdir().unwrap();
    let (speech, outbound, env) = speech(dir.path());
    env.set("STT_URL", "http://sidecar/stt");
    outbound.answer(|_| Ok((500, json!({ "error": "boom" }))));
    let error = speech.transcribe(&recording(dir.path())).await.unwrap_err();
    assert!(error.message.starts_with("sidecar STT 500:"), "{}", error.message);
}

#[tokio::test]
async fn given_a_missing_recording_when_transcribed_then_a_whisper_error_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let (speech, _, _) = speech(dir.path());
    let error = speech.transcribe(&dir.path().join("none.webm")).await.unwrap_err();
    assert_eq!(error.name, "WhisperError");
    assert!(error.message.starts_with("Audio file not found"));
}

#[tokio::test]
async fn given_an_engine_that_fails_once_when_transcribed_then_the_recording_is_posted_again_and_the_second_answer_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let (speech, outbound, env) = speech(dir.path());
    env.set("STT_URL", "http://sidecar/stt");
    let calls = std::sync::atomic::AtomicUsize::new(0);
    outbound.answer(move |_| if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 { Ok((503, json!({ "error": "busy" }))) } else { Ok((200, json!({ "transcript": "hello" }))) });
    let result = speech.transcribe(&recording(dir.path())).await.unwrap();
    assert_eq!((result.transcript.as_str(), outbound.to("http://sidecar/stt").len()), ("hello", 2));
}

#[tokio::test]
async fn given_an_engine_that_rejects_the_audio_when_transcribed_then_it_is_not_asked_again() {
    let dir = tempfile::tempdir().unwrap();
    let (speech, outbound, env) = speech(dir.path());
    env.set("STT_URL", "http://sidecar/stt");
    outbound.answer(|_| Ok((400, json!({ "error": "bad audio" }))));
    let error = speech.transcribe(&recording(dir.path())).await.unwrap_err();
    assert_eq!((error.message.starts_with("sidecar STT 400:"), outbound.to("http://sidecar/stt").len()), (true, 1));
}
