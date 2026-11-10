use crate::doubles::{ev, fails, idle, says, Posted, Prompt, Run, Script};
use crate::harness::{eventually, post, service, Env, Form, Options, Service};
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const CODE: &str = "class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        left = 0\n        right = len(s) - 1\n        while left < right:\n            pass";
const MODEL: &str = "kimi-code-plan-global/kimi-for-coding-highspeed";
const ORDS: &str = "https://ords.example/edit_events";
const TOOLS: [&str; 17] = ["bash", "edit", "multiedit", "write", "read", "grep", "glob", "list", "patch", "todowrite", "todoread", "webfetch", "websearch", "task", "question", "skill", "lsp_diagnostics"];

fn alternative(over: Value) -> Value {
    let mut alt = json!({ "id": "alt-1", "label": "Return on mismatch", "summary": "Replaces pass with an early return.", "confidence": 0.9, "commands": [{ "type": "replaceLines", "startLine": 6, "endLine": 6, "newText": "            return False" }] });
    for (key, value) in over.as_object().unwrap() {
        alt[key] = value.clone();
    }
    alt
}

fn reply(alternatives: Value) -> String {
    json!({ "transcript": "return false", "alternatives": alternatives }).to_string()
}

/// The planner's model answering `text` with the provider's token accounting.
fn answers(text: String) -> Script {
    Arc::new(move |p: &Prompt| {
        let s = p.session.as_str();
        let info = json!({ "id": format!("m-{s}"), "role": "assistant", "sessionID": s, "tokens": { "input": 100, "output": 20, "reasoning": 0, "cache": { "read": 60, "write": 10 } } });
        vec![ev("message.updated", json!({ "sessionID": s, "info": info })), says(s, &text), idle(s)]
    })
}

async fn row(svc: &Service, edit_id: &str) -> Value {
    let find = || svc.outbound.to(ORDS).into_iter().filter_map(|p| p.json).find(|r| r["edit_id"] == edit_id);
    eventually(&format!("the row of {edit_id} reached the table"), || find().is_some()).await;
    find().unwrap()
}

fn matches_row(row: &Value, expected: Value) {
    for (key, value) in expected.as_object().unwrap() {
        assert_eq!(&row[key], value, "{key} of {row}");
    }
}

fn reason(answer: &Value, index: usize) -> Value {
    answer["alternatives"][index].get("invalidReason").cloned().unwrap_or(Value::Null)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_spoken_instruction_and_code_when_a_voice_edit_is_planned_then_the_planner_turn_answers_validated_alternatives_and_every_turn_leaves_a_latency_row() {
    let env = Env::new(&[("EDIT_STATS_ORDS_BASE_URL", "PLACEHOLDER_FILL_ME"), ("HEALTH_REPORTS_ORDS_PASSWORD", "secret-pw")]);
    let cwd = env.dir("planner").to_string_lossy().into_owned();
    env.set("OPENCODE_CWD", &cwd);
    let stat = json!({ "editId": "e1", "source": "voice", "problemId": "p1", "platform": "ios", "transcribeWallMs": 120, "planWallMs": 800, "postConfirmWaitMs": 5, "speculativeHit": true, "localFastPath": false, "outcome": "auto", "pickedIndex": null, "alternatives": 2, "transcriptChars": 40, "codeCharsBefore": 900, "codeCharsAfter": 910, "error": null, "extra": "dropped" });

    // Given no stats table (a placeholder counts as none), when a client's row is posted, then 202 says it was not stored.
    let untabled = service(&env, Options::default());
    let unstored = untabled.send(post("/api/edit-stats").json(stat.clone())).await;
    assert_eq!((unstored.status, unstored.json()), (StatusCode::ACCEPTED, json!({ "ok": true, "stored": false })));
    env.set("EDIT_STATS_ORDS_BASE_URL", ORDS);
    env.set("HEALTH_REPORTS_ORDS_USER", "rpt");
    let svc = service(&env, Options::default());
    let oc = &svc.opencode;
    let plan = |body: Value| {
        let mut full = json!({ "currentCode": CODE, "language": "python" });
        for (key, value) in body.as_object().unwrap() {
            full[key] = value.clone();
        }
        post("/api/voice-plan").json(full)
    };

    // Given a stats table, when client rows are posted, then each is 202 stored and reaches the table with a Basic credential, its
    // flags as 0 and 1, the server's host and time, and no unknown field; an error outcome is not ok; a row that breaks a rule is a
    // 400 naming the first field that does.
    let stored = svc.send(post("/api/edit-stats").json(stat.clone())).await;
    assert_eq!((stored.status, stored.json()), (StatusCode::ACCEPTED, json!({ "ok": true, "stored": true })));
    let mut failing = stat.clone();
    failing["editId"] = json!("failing");
    failing["outcome"] = json!("error");
    failing["error"] = json!("timeout");
    svc.send(post("/api/edit-stats").json(failing)).await;
    let client = row(&svc, "e1").await;
    matches_row(&client, json!({ "side": "client", "route": "turn", "source": "voice", "ok": 1, "problem_id": "p1", "platform": "ios", "transcribe_wall_ms": 120, "plan_wall_ms": 800,
        "post_confirm_wait_ms": 5, "speculative_hit": 1, "local_fast_path": 0, "outcome": "auto", "picked_index": null, "alternatives": 2, "transcript_chars": 40,
        "code_chars": 900, "code_chars_after": 910, "error_text": null, "ffmpeg_ms": null }));
    assert!(client["host"].is_string() && client.get("extra").is_none());
    assert!(client["ts"].as_str().unwrap().len() == 24 && client["ts"].as_str().unwrap().ends_with('Z'));
    matches_row(&row(&svc, "failing").await, json!({ "ok": 0, "outcome": "error", "error_text": "timeout" }));
    let posted = svc.outbound.to(ORDS);
    assert_eq!((posted[0].url.as_str(), posted[0].basic_auth.clone()), ("https://ords.example/edit_events/", Some(("rpt".to_string(), "secret-pw".to_string()))));
    let broken = [
        (json!({ "outcome": "maybe", "editId": "", "platform": 5 }), "editId: invalid length 0, expected a string of 1 to 64 characters"),
        (json!({ "outcome": "maybe" }), "outcome: unknown variant `maybe`, expected one of `auto`, `picked`, `dismissed`, `error`, `cancelled`"),
        (json!({ "alternatives": 100 }), "alternatives: invalid value: integer `100`, expected an integer from 0 to 99"),
        (json!({ "pickedIndex": 1.5 }), "pickedIndex: invalid type: floating point `1.5`, expected i64"),
    ];
    for (over, message) in broken {
        let mut body = stat.clone();
        for (key, value) in over.as_object().unwrap() {
            body[key] = value.clone();
        }
        let refused = svc.send(post("/api/edit-stats").json(body)).await;
        assert_eq!((refused.status, refused.json()), (StatusCode::BAD_REQUEST, json!({ "error": "invalid_edit_stat", "message": message })));
    }
    let not_a_row = svc.send(post("/api/edit-stats").json(json!([1]))).await;
    assert_eq!(not_a_row.json()["message"], "invalid type: sequence, expected a map");

    // Given no transcript or no code, when a plan is asked, then 400 and opencode is not called.
    for (body, error) in [(json!({ "transcript": "" }), "Missing transcript"), (json!({ "transcript": "x", "currentCode": "" }), "Missing currentCode")] {
        let refused = svc.send(plan(body)).await;
        assert_eq!((refused.status, refused.json()), (StatusCode::BAD_REQUEST, json!({ "error": error })));
    }
    assert!(oc.requests().is_empty());

    // When a plan is asked, then the alternatives come back validated, the planner was asked in its directory for exactly the
    // numbered code with every tool off in its own agent, and the turn's row carries the planner, the tokens and the counts.
    oc.on_prompt(answers(reply(json!([alternative(json!({}))]))));
    let planned = svc.send(plan(json!({ "transcript": "replace pass with return false", "editId": "turn-77", "source": "text" }))).await;
    assert_eq!(planned.status, StatusCode::OK);
    let mut valid = alternative(json!({}));
    valid["valid"] = json!(true);
    assert_eq!(planned.json(), json!({ "transcript": "return false", "alternatives": [valid] }));
    let probe = &oc.find("GET", "/find/file")[0];
    assert_eq!((probe.directory(), probe.query.get("query").map(String::as_str)), (Some(cwd.as_str()), Some("__remote_chat_agent_probe__")));
    assert_eq!((oc.find("POST", "/session")[0].directory(), oc.find("GET", "/event")[0].directory()), (Some(cwd.as_str()), Some(cwd.as_str())));
    let prompt = &oc.find("POST", "*/prompt_async")[0];
    assert_eq!(prompt.directory(), Some(cwd.as_str()));
    let sent = prompt.body.clone().unwrap();
    assert_eq!((sent["model"].clone(), sent["agent"].clone()), (json!({ "providerID": "kimi-code-plan-global", "modelID": "kimi-for-coding-highspeed" }), json!("voice-planner")));
    assert_eq!(sent["parts"], json!([{ "type": "text", "text": "Language: python\nCode:\n1| class Solution:\n2|     def isPalindrome(self, s: str) -> bool:\n3|         left = 0\n4|         right = len(s) - 1\n5|         while left < right:\n6|             pass\nInstruction: \"replace pass with return false\"\nJSON:" }]));
    assert!(sent["system"].as_str().unwrap().starts_with("You turn one spoken code-editing instruction into the smallest edit"));
    let mut tools: Vec<&str> = sent["tools"].as_object().unwrap().keys().map(String::as_str).collect();
    tools.sort_unstable();
    let mut expected_tools = TOOLS.to_vec();
    expected_tools.sort_unstable();
    assert_eq!(tools, expected_tools);
    assert!(sent["tools"].as_object().unwrap().values().all(|v| v == false));
    let first = row(&svc, "turn-77").await;
    matches_row(&first, json!({ "side": "server", "route": "plan", "source": "text", "ok": 1, "status": 200, "planner_model": format!("{MODEL}@v2+voice-planner"), "session_reused": 0,
        "prompt_tokens": 100, "cache_read": 60, "cache_write": 10, "alternatives": 1, "invalid_count": 0, "retried": 0, "code_chars": CODE.len(), "language": "python", "transcript_chars": 30 }));
    assert!(first["ms"].is_number() && first["planner_ms"].is_number());

    // When the next plan is asked, then the held session is prompted again and none is created.
    oc.forget_requests();
    svc.send(plan(json!({ "transcript": "x", "editId": "turn-78" }))).await;
    assert!(oc.find("POST", "/session").is_empty());
    assert_eq!(oc.find("POST", "*/prompt_async")[0].path, "/session/oc-ses-1/prompt_async");
    assert_eq!(row(&svc, "turn-78").await["session_reused"], 1);

    // When the answer comes fenced, in prose, with raw line breaks in its strings or with the new lines under another field's
    // name, then it is still read; with no language the code is typescript; python block headers get a pass body.
    let ask = |answer: String, body: Value| {
        oc.on_prompt(answers(answer));
        svc.send(plan(body))
    };
    let json_text = reply(json!([alternative(json!({}))]));
    for wrapped in [format!("Here you go:\n```json\n{json_text}\n```\nDone."), format!("Sure. {json_text} Hope it helps.")] {
        assert_eq!(ask(wrapped, json!({ "transcript": "x" })).await.json()["alternatives"].as_array().unwrap().len(), 1);
    }
    let raw = r#"{"transcript":"x","alternatives":[{"id":"alt-1","label":"l","summary":"s","confidence":0.9,"commands":[{"type":"replaceLines","startLine":6,"endLine":6,"newText":"            if a:
                return False"}]}]}"#;
    assert_eq!(ask(raw.into(), json!({ "transcript": "x" })).await.json()["alternatives"][0]["commands"], json!([{ "type": "replaceLines", "startLine": 6, "endLine": 6, "newText": "            if a:\n                return False" }]));
    let drifted = reply(json!([alternative(json!({ "commands": [{ "type": "insertAfterLine", "line": 3, "newText": "        n = 0" }, { "type": "replaceLines", "startLine": 6, "endLine": 6, "text": "            return n" }] }))]));
    let renamed = ask(drifted, json!({ "transcript": "x" })).await.json();
    assert_eq!(renamed["alternatives"][0]["valid"], true);
    assert_eq!(renamed["alternatives"][0]["commands"], json!([{ "type": "insertAfterLine", "line": 3, "text": "        n = 0" }, { "type": "replaceLines", "startLine": 6, "endLine": 6, "newText": "            return n" }]));
    oc.forget_requests();
    ask(json_text.clone(), json!({ "transcript": "x", "language": null })).await;
    assert!(oc.find("POST", "*/prompt_async")[0].body.as_ref().unwrap()["parts"][0]["text"].as_str().unwrap().starts_with("Language: typescript\n"));
    let header = reply(json!([alternative(json!({ "commands": [{ "type": "insertAfterLine", "line": 4, "text": "        if left < right:" }] }))]));
    let python = ask(header.clone(), json!({ "transcript": "x", "language": "python" })).await.json();
    let typescript = ask(header, json!({ "transcript": "x", "language": "typescript" })).await.json();
    assert_eq!(python["alternatives"][0]["commands"], json!([{ "type": "insertAfterLine", "line": 4, "text": "        if left < right:\n            pass" }]));
    assert_eq!(typescript["alternatives"][0]["commands"], json!([{ "type": "insertAfterLine", "line": 4, "text": "        if left < right:" }]));

    // When alternatives break a rule, then each is flagged with its reason and the rest are untouched; a request for a whole
    // solution allows only a first step; an alternative the schema refuses is dropped, and with none left the plan fails.
    let seven: Vec<String> = (0..7).map(|i| format!("        x{i} = {i}")).collect();
    let flagged = ask(
        reply(json!([
            alternative(json!({ "id": "ok" })),
            alternative(json!({ "id": "big", "commands": [{ "type": "insertAfterLine", "line": 3, "text": seven.join("\n") }] })),
            alternative(json!({ "id": "overlap", "commands": [{ "type": "replaceLines", "startLine": 6, "endLine": 6, "newText": "a" }, { "type": "deleteLines", "startLine": 6, "endLine": 6 }] })),
            alternative(json!({ "id": "rename", "commands": [{ "type": "renameSymbol", "position": 0, "newName": "Other" }] })),
        ])),
        json!({ "transcript": "return false" }),
    )
    .await
    .json();
    let verdicts: Vec<(Value, Value)> = flagged["alternatives"].as_array().unwrap().iter().map(|a| (a["id"].clone(), a["valid"].clone())).collect();
    assert_eq!(verdicts, [(json!("ok"), json!(true)), (json!("big"), json!(false)), (json!("overlap"), json!(false)), (json!("rename"), json!(true))]);
    assert_eq!(reason(&flagged, 1), "this step writes 8 lines at once (limit 6) -- say it as smaller steps, e.g. declare the variables, then add the loop, then the condition");
    let overlap = reason(&flagged, 2).as_str().unwrap().to_string();
    assert!(overlap.starts_with("command range [") && overlap.ends_with("both refer to the code before any command ran"), "{overlap}");
    let four: Vec<String> = (0..4).map(|i| format!("        y{i} = {i}")).collect();
    let step = reply(json!([alternative(json!({ "commands": [{ "type": "insertAfterLine", "line": 3, "text": four.join("\n") }] }))]));
    assert_eq!(reason(&ask(step.clone(), json!({ "transcript": "write a two pointer solution" })).await.json(), 0), "this step writes 5 lines at once (limit 3) -- say it as smaller steps, e.g. declare the variables, then add the loop, then the condition");
    assert_eq!(reason(&ask(step, json!({ "transcript": "add some variables" })).await.json(), 0), Value::Null);
    let kept = ask(reply(json!([alternative(json!({ "id": "good" })), alternative(json!({ "id": "bad", "commands": [{ "type": "insertAfterLine", "line": 2 }] }))])), json!({ "transcript": "x" })).await;
    assert_eq!((kept.status, kept.json()["alternatives"].as_array().unwrap().len()), (StatusCode::OK, 1));
    let refused = ask(reply(json!([alternative(json!({ "commands": [{ "type": "insertAfterLine", "line": 2 }] }))])), json!({ "transcript": "x" })).await;
    assert_eq!((refused.status, refused.json()["error"].clone()), (StatusCode::INTERNAL_SERVER_ERROR, json!("voice_plan_failed")));
    assert!(refused.json()["message"].as_str().unwrap().starts_with("Schema validation failed for every alternative: alternatives[0].commands[0]: missing field `text`"));
    let none = ask(reply(json!([])), json!({ "transcript": "x", "editId": "bad-1" })).await;
    assert_eq!((none.status, none.json()), (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "voice_plan_failed", "message": "LLM returned no alternatives" })));
    matches_row(&row(&svc, "bad-1").await, json!({ "route": "plan", "ok": 0, "status": 500, "error_code": "no_alternatives", "error_text": "LLM returned no alternatives" }));

    // When the answer is not JSON, then one more turn is spent on the same session and its answer used; twice is a 500 with the
    // parse failure and the raw text.
    oc.forget_requests();
    let turn = Arc::new(AtomicUsize::new(0));
    let (counted, good) = (Arc::clone(&turn), json_text.clone());
    oc.on_prompt(Arc::new(move |p: &Prompt| (answers(if counted.fetch_add(1, Ordering::SeqCst) == 0 { "I cannot do that".into() } else { good.clone() }))(p)));
    let retried = svc.send(plan(json!({ "transcript": "x", "editId": "retry-1" }))).await;
    assert_eq!(retried.status, StatusCode::OK);
    let paths: Vec<String> = oc.find("POST", "*/prompt_async").into_iter().map(|r| r.path).collect();
    assert_eq!((paths.len(), paths[0] == paths[1]), (2, true));
    assert_eq!(row(&svc, "retry-1").await["retried"], 1);
    let garbled = ask("still no json".into(), json!({ "transcript": "x" })).await;
    assert_eq!(garbled.status, StatusCode::INTERNAL_SERVER_ERROR);
    let message = garbled.json()["message"].as_str().unwrap().to_string();
    assert!(message.starts_with("Failed to parse LLM response as JSON: ") && message.contains("Raw:\nstill no json"), "{message}");

    // When the held session dies under a turn, then a fresh one is started and answers; when every session fails, or a turn ends
    // having said nothing, the plan is a 500 saying so.
    oc.forget_requests();
    let held = oc.find("POST", "/session").len();
    let good = json_text.clone();
    oc.on_prompt(Arc::new(move |p: &Prompt| if p.session == "oc-ses-1" { vec![fails(&p.session, "context overflow")] } else { (answers(good.clone()))(p) }));
    let replaced = svc.send(plan(json!({ "transcript": "x", "editId": "replaced-1" }))).await;
    assert_eq!((replaced.status, oc.find("POST", "/session").len()), (StatusCode::OK, held + 1));
    assert_eq!(row(&svc, "replaced-1").await["session_reused"], 0);
    oc.on_prompt(Arc::new(|p: &Prompt| vec![fails(&p.session, "rate limited")]));
    let failed = svc.send(plan(json!({ "transcript": "x" }))).await;
    assert_eq!((failed.status, failed.json()), (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "voice_plan_failed", "message": "LLM stream failed: opencode session error: rate limited" })));
    oc.on_prompt(Arc::new(|p: &Prompt| vec![ev("message.updated", json!({ "sessionID": p.session, "info": { "id": "m", "role": "assistant", "sessionID": p.session } })), idle(&p.session)]));
    let empty = svc.send(plan(json!({ "transcript": "x" }))).await.json();
    assert!(empty["message"].as_str().unwrap().starts_with("LLM stream failed: opencode finished the turn without emitting any content"));

    // When opencode cannot see the directory, then 500 names it and nothing is prompted.
    oc.forget_requests();
    oc.respond("GET", "/find/file", 500, json!({ "error": "NotFound" }));
    let blind = svc.send(plan(json!({ "transcript": "x" }))).await;
    assert_eq!(blind.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(blind.json()["message"].as_str().unwrap().starts_with(&format!("LLM stream failed: the agent backend cannot see the working directory {cwd} ")));
    assert!(oc.find("POST", "*/prompt_async").is_empty());
    oc.respond("GET", "/find/file", 200, json!([]));

    // When the model never answers within the turn budget, then 504 says so and opencode is told to stop that session.
    env.set("VOICE_EDIT_TURN_TIMEOUT_MS", "400");
    oc.on_prompt(Arc::new(|_: &Prompt| Vec::new()));
    let slow = svc.send(plan(json!({ "transcript": "x", "editId": "slow-1" }))).await;
    assert_eq!((slow.status, slow.json()), (StatusCode::GATEWAY_TIMEOUT, json!({ "error": "voice_plan_failed", "message": "Planner did not answer within 400ms" })));
    let stuck = oc.find("POST", "*/prompt_async").last().unwrap().path.replace("/prompt_async", "/abort");
    eventually("the stuck session was aborted", || !oc.find("POST", &stuck).is_empty()).await;
    matches_row(&row(&svc, "slow-1").await, json!({ "ok": 0, "status": 504, "error_code": "timeout" }));
    env.unset("VOICE_EDIT_TURN_TIMEOUT_MS");

    // Given the first prompt variant and no planner agent, when a plan is asked, then the code is fenced, the system prompt is the
    // older one and no agent is named.
    env.set("VOICE_EDIT_PROMPT_VARIANT", "v1");
    env.set("VOICE_EDIT_AGENT", "");
    oc.forget_requests();
    ask(json_text, json!({ "transcript": "do it", "currentCode": "a", "editId": "v1-1" })).await;
    let sent = oc.find("POST", "*/prompt_async")[0].body.clone().unwrap();
    assert_eq!(sent["parts"][0]["text"], "Language: python\n\nCurrent code:\n```\na\n```\n\nTranscript: \"do it\"\n\nOutput ONLY the JSON object.");
    assert!(sent["system"].as_str().unwrap().starts_with("You are a surgical code editor.") && sent.get("agent").is_none());
    assert_eq!(row(&svc, "v1-1").await["planner_model"], format!("{MODEL}@v1"));
    env.unset("VOICE_EDIT_PROMPT_VARIANT");
    env.unset("VOICE_EDIT_AGENT");

    // Given a session limit of two turns, when three plans are asked of a fresh service, then the third starts a new session.
    env.set("VOICE_EDIT_SESSION_MAX_TURNS", "2");
    let turns = service(&env, Options::default());
    turns.opencode.on_prompt(answers(reply(json!([alternative(json!({}))]))));
    for _ in 0..3 {
        turns.send(post("/api/voice-plan").json(json!({ "transcript": "return false", "currentCode": CODE }))).await;
    }
    let paths: Vec<String> = turns.opencode.find("POST", "*/prompt_async").into_iter().map(|r| r.path).collect();
    assert_eq!(paths, ["/session/oc-ses-1/prompt_async", "/session/oc-ses-1/prompt_async", "/session/oc-ses-2/prompt_async"]);
}

/// ffmpeg that writes its WAV, and whisper.cpp that prints a timing line and the words.
fn whisper_cpp(env: &Env, quiet: bool) -> impl Fn(&Run) -> Option<Result<remote_chat_agent_core::application::ports::Output, remote_chat_agent_core::application::ports::ExecError>> + Send + Sync + 'static {
    let (ffmpeg, whisper) = (env.home.join("ffmpeg").to_string_lossy().into_owned(), env.home.join("bin/whisper-main").to_string_lossy().into_owned());
    move |run: &Run| {
        if run.program == ffmpeg {
            std::fs::write(run.args.last().unwrap(), b"RIFFxxxxWAVE").unwrap();
            run.ok("")
        } else if run.program == whisper {
            run.ok(if quiet { "[00:00:00.000 --> 00:00:01.000]  x\n" } else { "[00:00:00.000 --> 00:00:02.000]  ignored timing line\n  delete the print\n statement  \n" })
        } else {
            None
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_recording_when_it_is_transcribed_or_planned_in_one_request_then_the_configured_engine_hears_it_and_every_call_leaves_a_latency_row() {
    let env = Env::new(&[("STT_URL", " http://stt.local/transcribe "), ("STT_ENGINE", "groq"), ("EDIT_STATS_ORDS_BASE_URL", ORDS), ("HEALTH_REPORTS_ORDS_PASSWORD", "secret-pw"), ("OPENCODE_CWD", "/tmp")]);
    let svc = service(&env, Options::default());
    let heard = |status: u16, body: Value| svc.outbound.answer(move |p: &Posted| if p.url.contains("ords") { Ok((201, json!({}))) } else { Ok((status, body.clone())) });
    heard(200, json!({ "transcript": "delete the print statement", "ms": 3, "model": "fake-whisper", "threads": 2 }));
    let clip = |fields: &[(&str, &str)], bytes: &[u8]| fields.iter().fold(Form::default(), |form, (k, v)| form.field(k, v)).file("audio", "clip.wav", "audio/wav", bytes);
    let stt = || svc.outbound.posts().into_iter().filter(|p| !p.url.contains("ords")).collect::<Vec<_>>();

    // Given no recording, when a transcription is asked, then 400 and the engine is not called.
    let none = svc.send(post("/api/voice-transcribe").form(Form::default().field("editId", "x"))).await;
    assert_eq!((none.status, none.json()), (StatusCode::BAD_REQUEST, json!({ "error": "No audio file uploaded" })));
    assert!(stt().is_empty());

    // Given a sidecar (which wins over Groq), when recordings are transcribed, then it gets each as the audio field named by the
    // last segment of the filename field (recording.wav without one), with no credential, and the transcript comes back.
    let answer = svc.send(post("/api/voice-transcribe").form(clip(&[("filename", "take-3.m4a")], &[0u8; 1234]))).await;
    assert_eq!((answer.status, answer.json()), (StatusCode::OK, json!({ "transcript": "delete the print statement" })));
    svc.send(post("/api/voice-transcribe").form(clip(&[], &[0u8; 10]))).await;
    svc.send(post("/api/voice-transcribe").form(clip(&[("filename", "../../escaped.wav")], &[0u8; 10]))).await;
    let sent = stt();
    assert_eq!(sent[0].url, "http://stt.local/transcribe");
    assert!(sent[0].headers.is_empty() && sent[0].basic_auth.is_none());
    let files: Vec<(String, Option<String>, usize)> = sent.iter().map(|p| (p.form[0].0.clone(), p.form[0].1.clone(), p.form[0].2.len())).collect();
    assert_eq!(files, [("audio".into(), Some("take-3.m4a".into()), 1234), ("audio".into(), Some("recording.wav".into()), 10), ("audio".into(), Some("escaped.wav".into()), 10)]);

    // When the sidecar fails, hears nothing or cannot be reached, then 500 voice_transcribe_failed says which.
    for (status, body, message) in [(503, json!({ "detail": "model loading" }), "sidecar STT 503: {\"detail\":\"model loading\"}"), (200, json!({ "transcript": "   ", "ms": 1 }), "sidecar STT returned no transcript")] {
        heard(status, body);
        let failed = svc.send(post("/api/voice-transcribe").form(clip(&[], &[0u8; 10]))).await;
        assert_eq!((failed.status, failed.json()), (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "voice_transcribe_failed", "message": message })));
    }
    svc.outbound.answer(|p: &Posted| if p.url.contains("ords") { Ok((201, json!({}))) } else { Err("error sending request for url (http://stt.local/transcribe)".into()) });
    let unreachable = svc.send(post("/api/voice-transcribe").form(clip(&[], &[0u8; 10]))).await;
    assert_eq!(unreachable.json()["message"], "fetch failed: error sending request for url (http://stt.local/transcribe)");

    // When recordings with and without a clip number are transcribed, then their rows are told apart, carry the engine's own timing,
    // a usable client id or a server-made one, and a known source; a failed one's row says not ok with the error.
    heard(200, json!({ "transcript": "delete the print statement", "ms": 3, "model": "fake-whisper", "threads": 2 }));
    let long_id = "x".repeat(65);
    for fields in [vec![("editId", "live-1"), ("source", "voice"), ("chunkIndex", "4")], vec![("editId", "whole-1"), ("source", "nonsense")], vec![("editId", "hex-1"), ("chunkIndex", "0x10")], vec![("editId", "half-1"), ("chunkIndex", "1.5")], vec![("editId", long_id.as_str()), ("chunkIndex", "-1")]] {
        let bytes = if fields[0].1 == long_id { vec![0u8; 777] } else { vec![0u8; 1234] };
        svc.send(post("/api/voice-transcribe").form(clip(&fields, &bytes))).await;
    }
    matches_row(&row(&svc, "live-1").await, json!({ "side": "server", "route": "transcribe-chunk", "chunk_index": 4, "source": "voice", "ok": 1, "status": 200, "ffmpeg_ms": 0, "whisper_ms": 3,
        "whisper_model": "fake-whisper", "whisper_threads": 2, "audio_bytes": 1234, "transcript_chars": 26 }));
    matches_row(&row(&svc, "whole-1").await, json!({ "route": "transcribe", "chunk_index": null, "source": "voice" }));
    matches_row(&row(&svc, "hex-1").await, json!({ "route": "transcribe-chunk", "chunk_index": 16 }));
    matches_row(&row(&svc, "half-1").await, json!({ "route": "transcribe", "chunk_index": null }));
    eventually("the anonymous row reached the table", || svc.outbound.to(ORDS).iter().any(|p| p.json.as_ref().unwrap()["audio_bytes"] == 777)).await;
    let anonymous = svc.outbound.to(ORDS).into_iter().map(|p| p.json.unwrap()).find(|r| r["audio_bytes"] == 777).unwrap();
    let id = anonymous["edit_id"].as_str().unwrap();
    let parts: Vec<&str> = id.splitn(3, '-').collect();
    assert!(parts[0] == "srv" && parts[1].chars().all(|c| c.is_ascii_alphanumeric()) && parts[2].len() == 6, "{id}");
    heard(500, json!("boom"));
    svc.send(post("/api/voice-transcribe").form(clip(&[("editId", "fail-1")], &[0u8; 10]))).await;
    matches_row(&row(&svc, "fail-1").await, json!({ "route": "transcribe", "ok": 0, "status": 500, "error_code": "Error", "error_text": "sidecar STT 500: \"boom\"" }));

    // When a recording and code are sent in one request, then the transcript is planned and the alternatives come back, and its row
    // is the whole turn; no recording or no code is a 400, and a planner that fails is voice_edit_failed.
    heard(200, json!({ "transcript": "delete the print statement", "ms": 3, "model": "fake-whisper", "threads": 2 }));
    let edit = |fields: &[(&str, &str)], with_audio: bool| {
        let form = fields.iter().fold(Form::default(), |form, (k, v)| form.field(k, v));
        post("/api/voice-edit").form(if with_audio { form.file("audio", "clip.wav", "audio/wav", &[0u8; 1234]) } else { form })
    };
    let no_audio = svc.send(edit(&[("currentCode", CODE)], false)).await;
    let no_code = svc.send(edit(&[], true)).await;
    assert_eq!((no_audio.status, no_audio.json()), (StatusCode::BAD_REQUEST, json!({ "error": "No audio file uploaded" })));
    assert_eq!((no_code.status, no_code.json()), (StatusCode::BAD_REQUEST, json!({ "error": "Missing currentCode" })));
    svc.opencode.on_prompt(answers(json!({ "transcript": "delete the print statement", "alternatives": [alternative(json!({}))] }).to_string()));
    let combined = svc.send(edit(&[("currentCode", CODE), ("language", "python"), ("editId", "combined-1")], true)).await;
    let mut valid = alternative(json!({}));
    valid["valid"] = json!(true);
    assert_eq!((combined.status, combined.json()), (StatusCode::OK, json!({ "transcript": "delete the print statement", "alternatives": [valid] })));
    assert!(svc.opencode.find("POST", "*/prompt_async")[0].body.as_ref().unwrap()["parts"][0]["text"].as_str().unwrap().contains("Instruction: \"delete the print statement\""));
    matches_row(&row(&svc, "combined-1").await, json!({ "route": "edit", "source": "voice", "ok": 1, "status": 200, "whisper_model": "fake-whisper", "audio_bytes": 1234,
        "planner_model": format!("{MODEL}@v2+voice-planner"), "prompt_tokens": 100, "alternatives": 1, "language": "python" }));
    svc.opencode.on_prompt(answers(reply(json!([]))));
    let unplanned = svc.send(edit(&[("currentCode", CODE)], true)).await;
    assert_eq!((unplanned.status, unplanned.json()), (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "voice_edit_failed", "message": "LLM returned no alternatives" })));

    // Given Groq instead of a sidecar, when a recording is transcribed, then it is posted to Groq's endpoint with the key as a bearer
    // credential, the file field, the model, English and JSON; without a key whisper.cpp is used instead.
    env.unset("STT_URL");
    env.set("GROQ_API_KEY", " gk-1 ");
    heard(200, json!({ "text": "from groq" }));
    let groq = svc.send(post("/api/voice-transcribe").form(clip(&[("editId", "groq-1")], &[0u8; 10]))).await;
    assert_eq!(groq.json(), json!({ "transcript": "from groq" }));
    let to_groq = svc.outbound.to("groq").pop().unwrap();
    assert_eq!((to_groq.url.as_str(), to_groq.headers.clone()), ("https://api.groq.com/openai/v1/audio/transcriptions", vec![("Authorization".to_string(), "Bearer gk-1".to_string())]));
    let fields: Vec<(String, Option<String>, String)> = to_groq.form.iter().map(|(n, f, b)| (n.clone(), f.clone(), if f.is_some() { String::new() } else { String::from_utf8_lossy(b).into_owned() })).collect();
    assert_eq!(fields, [("file".into(), Some("recording.wav".into()), String::new()), ("model".into(), None, "whisper-large-v3-turbo".into()), ("language".into(), None, "en".into()), ("response_format".into(), None, "json".into())]);
    assert_eq!(row(&svc, "groq-1").await["whisper_model"], "groq:whisper-large-v3-turbo");

    // Given whisper.cpp and ffmpeg, when a recording is transcribed, then it is re-encoded to 16 kHz mono inside a temporary
    // directory (whatever the filename says), run with the model and the thread count, and the transcript is the lines that are not
    // timings; a whisper.cpp that prints nothing, or a binary or model that is not there, is a 500 saying so.
    env.unset("GROQ_API_KEY");
    std::fs::create_dir_all(env.home.join("bin")).unwrap();
    std::fs::write(env.home.join("bin/whisper-main"), "").unwrap();
    std::fs::write(env.home.join("model.bin"), "model").unwrap();
    for (name, value) in [("FFMPEG_PATH", env.home.join("ffmpeg")), ("WHISPER_CPP_BINARY", env.home.join("bin/whisper-main")), ("WHISPER_MODEL_PATH", env.home.join("model.bin"))] {
        env.set(name, &value.to_string_lossy());
    }
    env.set("WHISPER_MODEL_NAME", "tiny.en");
    env.set("WHISPER_THREADS", "3");
    svc.commands.answer(whisper_cpp(&env, false));
    let local = svc.send(post("/api/voice-transcribe").form(clip(&[("editId", "w-1"), ("filename", "../../escaped.wav")], &[0u8; 500]))).await;
    assert_eq!((local.status, local.json()), (StatusCode::OK, json!({ "transcript": "delete the print statement" })));
    let ffmpeg = &svc.commands.runs()[0];
    let input = std::path::Path::new(&ffmpeg.args[4]);
    assert_eq!(ffmpeg.args.iter().map(String::as_str).enumerate().filter(|(i, _)| ![4, 11].contains(i)).map(|(_, a)| a).collect::<Vec<_>>(), ["-y", "-loglevel", "error", "-i", "-ar", "16000", "-ac", "1", "-f", "wav"]);
    assert!(input.starts_with(std::env::temp_dir()) && input.parent().unwrap().file_name().unwrap().to_string_lossy().starts_with("voice-edit-") && input.ends_with("escaped.wav"), "{}", input.display());
    assert!(ffmpeg.args[11].ends_with("input.wav"));
    let whisper = &svc.commands.runs()[1];
    assert_eq!(whisper.args, ["-m", &env.home.join("model.bin").to_string_lossy(), "-f", &ffmpeg.args[11], "-l", "en", "-t", "3", "-nt"]);
    matches_row(&row(&svc, "w-1").await, json!({ "route": "transcribe", "ok": 1, "whisper_model": "tiny.en", "whisper_threads": 3, "audio_bytes": 500, "transcript_chars": 26 }));
    assert!(row(&svc, "w-1").await["ffmpeg_ms"].is_number());
    svc.commands.answer(whisper_cpp(&env, true));
    let quiet = svc.send(post("/api/voice-transcribe").form(clip(&[], &[0u8; 10]))).await;
    assert_eq!((quiet.status, quiet.json()), (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "voice_transcribe_failed", "message": "No transcript produced" })));
    env.set("WHISPER_MODEL_PATH", &env.home.join("missing.bin").to_string_lossy());
    let no_model = svc.send(post("/api/voice-transcribe").form(clip(&[], &[0u8; 10]))).await;
    assert_eq!(no_model.json()["message"], format!("whisper model not found at {}. Set WHISPER_MODEL_PATH to a ggml model.", env.home.join("missing.bin").display()));
    env.set("WHISPER_CPP_BINARY", "/nowhere/whisper-cli");
    let no_binary = svc.send(post("/api/voice-transcribe").form(clip(&[], &[0u8; 10]))).await;
    assert_eq!(no_binary.json()["message"], "whisper.cpp is not built: no binary at /nowhere/whisper-cli. Set WHISPER_CPP_BINARY to the whisper.cpp binary.");
}

// The TypeScript voice-edit DSL is the reference of the Rust port: `wf mimic` recorded what it answers into a cassette, and replays that
// against `examples/mimic_voice_dsl.rs`, the port behind a `mimic-driver/1` process. The binary comes from WF_MIMIC_BIN, or is built from `clis`.
const SET: &str = "tests/cassettes/voice-edit-dsl";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn cargo_json(args: &[&str], dir: &Path) -> Vec<Value> {
    let out = Command::new(env!("CARGO")).args(args).arg("--message-format=json").current_dir(dir).output().unwrap();
    assert!(out.status.success(), "cargo {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect()
}

/// The port's driver: the example is built here (`cargo test` builds none), and cargo names the executable.
fn candidate() -> PathBuf {
    let built = cargo_json(&["build", "-p", "rca-service-core", "--example", "mimic_voice_dsl"], root());
    built.iter().find_map(|m| m["executable"].as_str().map(PathBuf::from)).expect("cargo names the example's executable")
}

/// `wf-mimic` built from the clis workspace into a build directory of its own, behind a lock so two test binaries do not build at once.
fn mimic_bin() -> PathBuf {
    if let Some(bin) = std::env::var_os("WF_MIMIC_BIN") {
        return PathBuf::from(bin);
    }
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| std::env::current_exe().unwrap().ancestors().nth(3).unwrap().to_path_buf());
    let build_dir = target.join("wf-mimic-build");
    std::fs::create_dir_all(&build_dir).unwrap();
    let lock = std::fs::File::create(build_dir.join(".build.lock")).unwrap();
    lock.lock().unwrap();
    let manifest = root().join("../../../clis/Cargo.toml");
    let status = Command::new(env!("CARGO")).args(["build", "--locked", "--quiet", "-p", "wf-mimic", "--bin", "wf-mimic", "--manifest-path"]).arg(manifest).arg("--target-dir").arg(&build_dir).status().unwrap();
    assert!(status.success(), "building wf-mimic failed");
    build_dir.join("debug").join("wf-mimic")
}

fn mimic(args: &[&str]) -> (i32, Value) {
    let bin = mimic_bin();
    let out = Command::new(&bin).arg("mimic").args(args).stdin(Stdio::null()).output().unwrap_or_else(|e| panic!("{} could not start: {e}", bin.display()));
    let text = String::from_utf8_lossy(&out.stdout);
    (out.status.code().unwrap_or(-1), serde_json::from_str(&text).unwrap_or_else(|e| panic!("wf-mimic {args:?} printed no JSON ({e}): {text}")))
}

/// A `mimic.toml` naming the port's driver (and the same driver with a planted defect) as candidates of the set.
fn candidates(dir: &Path) -> String {
    let exe = candidate().display().to_string();
    let file = dir.join("candidates.toml");
    std::fs::write(&file, format!("[candidate.port]\ncommand = [{exe:?}]\n\n[candidate.defective]\ncommand = [{exe:?}, \"--tweak\"]\n")).unwrap();
    file.display().to_string()
}

fn recorded() -> usize {
    std::fs::read_to_string(root().join(SET).join("interactions.jsonl")).unwrap().lines().count()
}

#[test]
fn given_the_typescript_librarys_cassette_when_the_voice_edit_port_replays_it_then_every_recorded_answer_matches() {
    // Given the cassette of what the TypeScript library answers for generated inputs: too many cases to plan one by one
    let dir = tempfile::tempdir().unwrap();
    let candidates = candidates(dir.path());
    let set = root().join(SET).display().to_string();

    // When the port replays every recorded input, Then each case is clean, whatever the cassette holds
    let (code, replay) = mimic(&["replay", &set, "--target", &candidates, "--candidate", "port"]);
    assert_eq!((code, replay["severity"].clone()), (0, json!("none")), "{replay}");
    assert_eq!((replay["summary"]["total"].as_u64(), replay["summary"]["clean"].as_u64()), (Some(recorded() as u64), Some(recorded() as u64)), "{replay}");

    // And a port with a planted defect is found, which shows the replay reached it
    let (code, defective) = mimic(&["replay", &set, "--target", &candidates, "--candidate", "defective"]);
    assert_eq!((code, defective["severity"].clone()), (1, json!("breaking")), "{defective}");

    // And for a generated property case it breaks, the report names the smallest stored input of its shrink ladder that still breaks
    let inputs: std::collections::BTreeMap<String, usize> = std::fs::read_to_string(root().join(SET).join("interactions.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .map(|i| (i["id"].as_str().unwrap().to_string(), i["input"].to_string().len()))
        .collect();
    let shrunk = defective["shrunk"].as_array().unwrap();
    assert!(!shrunk.is_empty(), "the planted defect reached no property case: {defective}");
    for s in shrunk {
        let (case, minimal) = (s["case"].as_str().unwrap(), s["minimal"].as_str().unwrap());
        assert!(inputs[minimal] <= inputs[case], "{minimal} is not a shrink of {case}");
        assert_eq!(s["input"].to_string().len(), inputs[minimal]);
    }
}

#[test]
fn given_the_cassette_when_the_typescript_library_changes_without_a_new_recording_then_verify_fails_until_it_is_reverted() {
    // Given the set beside a copy of the library it was recorded from, in a git repository of its own
    let repo = tempfile::tempdir().unwrap();
    let library = root().join("../../../frontend/libraries/voice-edit-dsl");
    let listed = Command::new("git").args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"]).current_dir(&library).output().unwrap();
    for name in String::from_utf8_lossy(&listed.stdout).split('\0').filter(|n| !n.is_empty()) {
        let to = repo.path().join("library").join(name);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(library.join(name), to).unwrap();
    }
    let set = repo.path().join("set");
    std::fs::create_dir_all(&set).unwrap();
    for entry in std::fs::read_dir(root().join(SET)).unwrap().flatten() {
        let (from, to) = (entry.path(), set.join(entry.file_name()));
        if from.is_dir() {
            std::fs::create_dir_all(&to).unwrap();
            for case in std::fs::read_dir(&from).unwrap().flatten() {
                std::fs::copy(case.path(), to.join(case.file_name())).unwrap();
            }
        } else {
            std::fs::copy(from, to).unwrap();
        }
    }
    let config = std::fs::read_to_string(set.join("mimic.toml")).unwrap();
    let pin = config.lines().find(|l| l.starts_with("pin_tree")).expect("the set pins a tree");
    std::fs::write(set.join("mimic.toml"), config.replace(pin, "pin_tree = \"../library/src\"")).unwrap();
    assert!(Command::new("git").arg("init").arg("--quiet").arg(repo.path()).status().unwrap().success());
    let at = set.display().to_string();
    let pin_check = |report: &Value| report["sets"][0]["checks"].as_array().unwrap().iter().find(|c| c["name"] == "pin_hash").unwrap()["status"].clone();

    // When nothing changed, Then the pin is current
    let (code, fresh) = mimic(&["verify", &at]);
    assert_eq!((code, pin_check(&fresh)), (0, json!("ok")), "{fresh}");

    // When a rule of the library changes and the set is not recorded again, Then verify names the pin
    let validator = repo.path().join("library/src/use-cases/validator.ts");
    let original = std::fs::read_to_string(&validator).unwrap();
    std::fs::write(&validator, format!("{original}\n// planted\n")).unwrap();
    let (code, stale) = mimic(&["verify", &at]);
    assert_eq!((code, pin_check(&stale)), (1, json!("stale")), "{stale}");

    // And reverting the change makes the recorded set current again
    std::fs::write(&validator, original).unwrap();
    let (code, reverted) = mimic(&["verify", &at]);
    assert_eq!((code, pin_check(&reverted)), (0, json!("ok")), "{reverted}");

    // And a change to the library's own tests or tooling, which the port does not follow, leaves the recording current
    let test_file = repo.path().join("library/tests").join(format!("{}.test.ts", "planted"));
    std::fs::create_dir_all(test_file.parent().unwrap()).unwrap();
    std::fs::write(&test_file, "// a unit test\n").unwrap();
    std::fs::write(repo.path().join("library/vitest.config.ts"), "// planted\n").unwrap();
    let (code, tests_only) = mimic(&["verify", &at]);
    assert_eq!((code, pin_check(&tests_only)), (0, json!("ok")), "{tests_only}");
}

#[test]
fn given_the_committed_cassette_when_it_is_verified_against_the_library_in_this_tree_then_it_is_current() {
    // Given the set and the library as they are in this tree, When verify reads the pin, Then the recording is of this library
    let (code, report) = mimic(&["verify", &root().join(SET).display().to_string()]);
    assert_eq!((code, report["ok"].clone()), (0, json!(true)), "{report}");
}
