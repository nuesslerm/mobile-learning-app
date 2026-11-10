use crate::doubles::{ev, fails, idle, says, Prompt, Run, Script};
use crate::harness::{delete, get, post, service, Env, Options};
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn tool(session: &str, name: &str, call: &str, state: Value) -> Value {
    ev("message.part.updated", json!({ "sessionID": session, "part": { "id": format!("part-{call}"), "type": "tool", "tool": name, "callID": call, "state": state, "messageID": "m", "sessionID": session } }))
}

/// The first prompt fails with `message`, every later one answers `text`.
fn fails_once_then(message: &'static str, text: &'static str) -> Script {
    let turn = AtomicUsize::new(0);
    Arc::new(move |p: &Prompt| if turn.fetch_add(1, Ordering::SeqCst) == 0 { vec![fails(&p.session, message)] } else { vec![says(&p.session, text), idle(&p.session)] })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_prompt_when_a_one_shot_run_is_requested_then_the_ladder_is_walked_each_attempt_judged_and_the_run_logged() {
    let env = Env::new(&[("OPENCODE_LADDER_IGNORE_STATS", "1")]);
    let repo = env.dir("repo");
    std::fs::create_dir(repo.join(".git")).unwrap();
    let (run_log, ledger) = (env.home.join("runs.jsonl"), env.home.join("pools.json"));
    env.set("OPENCODE_DISPATCH_RUN_LOG", &run_log.to_string_lossy());
    env.set("OPENCODE_POOL_LEDGER", &ledger.to_string_lossy());
    let svc = service(&env, Options::default());
    let oc = &svc.opencode;
    let cwd = repo.to_string_lossy().into_owned();
    let run = |body: Value| {
        let mut body = body;
        body["cwd"] = json!(cwd);
        body["auto"] = json!(true);
        post("/runs").secret().json(body)
    };
    let rows = || -> Vec<Value> { std::fs::read_to_string(&run_log).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect() };
    svc.commands.answer(|r: &Run| (r.program == "git").then(|| r.ok("")).flatten());

    // Given no secret, a wrong one, or none configured at all, when a run is requested, then 401 before the body is looked at.
    for refused in [post("/runs").json(json!({ "not": "valid" })), post("/runs").header("x-bot-secret", "nope").json(json!({ "not": "valid" }))] {
        let answer = svc.send(refused).await;
        assert_eq!((answer.status, answer.json()), (StatusCode::UNAUTHORIZED, json!({ "error": "Unauthorized", "code": "UNAUTHORIZED" })));
    }
    let open = crate::harness::service(&env, Options { secret: None, ..Options::default() });
    assert_eq!(open.send(post("/runs").header("x-bot-secret", "").json(json!({ "prompt": "p", "cwd": "/tmp" }))).await.status, StatusCode::UNAUTHORIZED);

    // Given bodies that break the contract, when a run is requested with the secret, then 400 says what is wrong.
    let invalid = [
        (json!({ "cwd": "/tmp" }), "missing field `prompt`"),
        (json!({ "prompt": "p", "cwd": "" }), "cwd: invalid length 0, expected a string of at least 1 character"),
        (json!({ "prompt": "p", "cwd": "/tmp", "cli": "nope" }), "cli: unknown variant `nope`, expected one of `opencode`, `claude`, `codex`"),
        (json!({ "prompt": "p", "cwd": "/tmp", "models": [1] }), "models[0]: invalid type: integer `1`, expected a string"),
    ];
    for (body, error) in invalid {
        let answer = svc.send(post("/runs").secret().json(body)).await;
        assert_eq!((answer.status, answer.json()), (StatusCode::BAD_REQUEST, json!({ "error": error, "code": "INVALID_REQUEST" })));
    }

    // Given a pinned model that answers, when a run is requested, then that model serves it in the directory and the result carries
    // the text and the attempt.
    oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "hello there"), idle(&p.session)]));
    let served = svc.send(run(json!({ "prompt": "p1", "shape": "document", "models": ["pp/m1"] }))).await;
    assert_eq!(served.status, StatusCode::OK);
    let attempt = json!({ "index": 0, "choice": { "model": "pp/m1" }, "verdict": { "ok": true } });
    let mut body = served.json();
    let session = body.as_object_mut().unwrap().remove("session");
    let events = body.as_object_mut().unwrap().remove("events");
    assert!(events.is_none_or(|e| e.as_array().is_some_and(|e| e.iter().all(|e| e["type"] == json!("text") && e["sessionID"] == session.clone().unwrap()))), "the turn as opencode run prints it");
    assert!(session.is_some_and(|s| s.as_str().is_some_and(|s| !s.is_empty())), "the answer names the session that served it");
    assert_eq!(body, json!({ "ok": true, "output": { "text": "hello there", "toolEvents": [], "structured": true }, "attempt": attempt, "attempts": [attempt] }));
    let prompt = oc.find("POST", "*/prompt_async")[0].body.clone().unwrap();
    assert_eq!((prompt["model"].clone(), prompt["parts"].clone()), (json!({ "providerID": "pp", "modelID": "m1" }), json!([{ "type": "text", "text": "p1" }])));
    assert_eq!(oc.find("POST", "/session")[0].directory(), Some(cwd.as_str()));

    // Given no models, when a run is requested (with earlier runs set aside by the environment), then the first rung of the shape's
    // ladder serves it, with its pool.
    std::fs::write(&run_log, "").unwrap();
    oc.forget_requests();
    let laddered = svc.send(run(json!({ "prompt": "p2", "shape": "document" }))).await.json();
    assert_eq!(laddered["attempt"], json!({ "index": 0, "choice": { "model": "opencode-go/deepseek-v4.1-flash", "pool": "opencode-go" }, "verdict": { "ok": true } }));
    assert_eq!(oc.find("POST", "*/prompt_async")[0].body.as_ref().unwrap()["model"], json!({ "providerID": "opencode-go", "modelID": "deepseek-v4.1-flash" }));

    // Given an implementer run whose models only talk, then each is judged to have written nothing and the failure carries the last
    // output; its log row says it wrote nothing and escalated, with no model.
    std::fs::remove_file(&run_log).unwrap();
    oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "I would change it like this"), idle(&p.session)]));
    let talked = svc.send(run(json!({ "prompt": "p3", "models": ["pp/m1", "pp/m2"] }))).await.json();
    let nothing = json!({ "ok": false, "failureClass": "wrote-nothing", "reason": "run completed without a file write or diff" });
    assert_eq!(
        talked,
        json!({ "ok": false, "attempts": [
            { "index": 0, "choice": { "model": "pp/m1" }, "verdict": nothing },
            { "index": 1, "choice": { "model": "pp/m2" }, "verdict": nothing },
        ], "lastOutput": { "text": "I would change it like this", "toolEvents": [], "structured": true } })
    );
    let row = &rows()[0];
    assert_eq!((row["ok"].clone(), row["wroteNothing"].clone(), row["reason"].clone(), row["escalated"].clone(), row["shape"].clone()), (json!(false), json!(true), json!("wrote-nothing"), json!(true), json!("implementer:unit")));
    assert_eq!(row["toolCalls"].as_f64(), Some(0.0));
    assert!(row.get("model").is_none());

    // Given a model that writes a file, when an implementer run is requested, then the tool event and the diff of that file come back.
    let file = repo.join("a.txt").to_string_lossy().into_owned();
    let written = file.clone();
    oc.on_prompt(Arc::new(move |p: &Prompt| vec![tool(&p.session, "write", "call-write", json!({ "status": "completed", "input": { "filePath": written, "content": "x" }, "output": "ok" })), says(&p.session, "done"), idle(&p.session)]));
    let diffed = file.clone();
    svc.commands.answer(move |r: &Run| (r.program == "git").then(|| if r.args.ends_with(&["--".to_string(), diffed.clone()]) { r.ok(" a.txt | 1 +\n 1 file changed, 1 insertion(+)\n") } else { r.ok("") }).flatten());
    let wrote = svc.send(run(json!({ "prompt": "p4", "models": ["pp/m1"] }))).await.json();
    assert_eq!(wrote["ok"], true);
    assert_eq!(wrote["output"]["toolEvents"], json!([{ "tool": "write", "status": "completed", "input": { "filePath": file, "content": "x" } }]));
    assert_eq!(wrote["output"]["diffStat"], "a.txt | 1 +\n 1 file changed, 1 insertion(+)");
    assert!(svc.commands.of("git").last().unwrap().is("git", &["-C", &cwd, "diff", "--stat", "--", &file]));

    // Given tool calls reported under other names, as growing snapshots, and as file changes, then they are merged per call, named
    // in lower case, given a filePath, a file change becomes one patch per path, and a pending call or one with neither an id nor
    // input is left out.
    let (edited, patched) = (file.clone(), file.clone());
    oc.on_prompt(Arc::new(move |p: &Prompt| {
        let s = p.session.as_str();
        vec![
            tool(s, "Edit", "c1", json!({ "status": "running", "input": { "file_path": edited, "oldString": "a" } })),
            tool(s, "Edit", "c1", json!({ "status": "completed", "output": "ok" })),
            tool(s, "write", "c2", json!({ "status": "pending", "input": { "filePath": edited } })),
            tool(s, "fileChange", "c3", json!({ "status": "completed", "input": { "changes": [{ "path": patched }, { "path": "/w/b" }, { "kind": "x" }] } })),
            tool(s, "bash", "c4", json!({ "status": "failed", "input": { "command": "ls" } })),
            ev("message.part.updated", json!({ "sessionID": s, "part": { "id": "anon-1", "type": "tool", "tool": "bash", "state": { "status": "completed" }, "messageID": "m", "sessionID": s } })),
            ev("message.part.updated", json!({ "sessionID": s, "part": { "id": "anon-2", "type": "tool", "tool": "bash", "state": { "status": "completed", "input": { "command": "pwd" } }, "messageID": "m", "sessionID": s } })),
            says(s, "edited"),
            idle(s),
        ]
    }));
    let merged = svc.send(run(json!({ "prompt": "p4b", "shape": "document", "models": ["pp/m1"] }))).await.json();
    assert_eq!(
        merged["output"]["toolEvents"],
        json!([
            { "tool": "edit", "status": "completed", "input": { "file_path": file, "oldString": "a", "filePath": file } },
            { "tool": "patch", "status": "completed", "input": { "changes": [{ "path": file }, { "path": "/w/b" }, { "kind": "x" }], "filePath": file } },
            { "tool": "patch", "status": "completed", "input": { "changes": [{ "path": file }, { "path": "/w/b" }, { "kind": "x" }], "filePath": "/w/b" } },
            { "tool": "bash", "status": "error", "input": { "command": "ls" } },
            { "tool": "bash", "status": "completed", "input": { "command": "pwd" } },
        ])
    );

    // Given a model that fails and one that answers, then the failure is a crash with its reason, the next model serves the run, and
    // one row is appended to the run log with the prompt's hash, the attempts and the outcome.
    std::fs::remove_file(&run_log).unwrap();
    oc.on_prompt(fails_once_then("boom", "second ok"));
    let recovered = svc.send(run(json!({ "prompt": "a prompt for the log", "shape": "document", "models": ["pp/m1", "pp/m2"] }))).await.json();
    assert_eq!(recovered["ok"], true);
    assert_eq!(recovered["output"]["text"], "second ok");
    assert_eq!(recovered["attempts"], json!([
        { "index": 0, "choice": { "model": "pp/m1" }, "verdict": { "ok": false, "failureClass": "crash", "reason": "opencode session error: boom" } },
        { "index": 1, "choice": { "model": "pp/m2" }, "verdict": { "ok": true } },
    ]));
    let logged = rows();
    assert_eq!(logged.len(), 1);
    let row = &logged[0];
    for (key, value) in [
        ("caller", json!("dispatch")), ("cwd", json!(cwd)), ("model", json!("pp/m2")), ("ok", json!(true)), ("promptChars", json!(20)), ("promptHead", json!("a prompt for the log")),
        ("promptSha256", json!("d262086447562631cb0a9775611763c1bf2a86ef0facf627c33f793ba2d07327")), ("outputChars", json!(9)), ("summaryChars", json!(9)), ("toolCalls", json!(0)),
        ("wroteNothing", json!(false)), ("shape", json!("document")), ("reason", json!("ok")), ("escalated", json!(false)),
        ("attempts", json!([{ "model": "pp/m1", "capped": false, "reason": "crash" }, { "model": "pp/m2", "capped": false, "reason": "ok" }])),
    ] {
        if key == "attempts" {
            let got: Vec<Value> = row[key].as_array().unwrap().iter().map(|a| json!({ "model": a["model"], "capped": a["capped"], "reason": a["reason"] })).collect();
            assert_eq!(Value::Array(got), value);
        } else if value.is_number() {
            assert_eq!(row[key].as_f64(), value.as_f64(), "{key}");
        } else {
            assert_eq!(row[key], value, "{key}");
        }
    }
    assert!(row["runId"].is_string() && row["durationMs"].is_number());
    assert!(row["ts"].as_str().unwrap().ends_with('Z'));

    // Given a model that fails midway, then what it said so far is in the last output with the failure.
    oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "half an answ"), fails(&p.session, "cut off")]));
    let cut = svc.send(run(json!({ "prompt": "p11", "shape": "document", "models": ["pp/m1"] }))).await.json();
    assert_eq!(cut["lastOutput"]["text"], "half an answ");
    assert_eq!(cut["lastOutput"]["error"], json!({ "failureClass": "crash", "message": "opencode session error: cut off" }));

    // Given a run pinned to a CLI this build does not serve, then the attempt is a crash that names why and opencode is not asked.
    oc.forget_requests();
    let unserved = svc.send(run(json!({ "prompt": "p6", "shape": "document", "models": ["anthropic/claude-x"], "cli": "codex" }))).await.json();
    assert_eq!((unserved["ok"].clone(), unserved["attempts"].as_array().unwrap().len()), (json!(false), 1));
    assert_eq!(unserved["attempts"][0]["verdict"]["failureClass"], "crash");
    assert_eq!(unserved["lastOutput"]["error"]["failureClass"], "crash");
    assert!(unserved["lastOutput"]["error"]["message"].as_str().unwrap().starts_with("Agent CLI \"codex\" is not available in this image"));
    let claude = svc.send(run(json!({ "prompt": "p6b", "shape": "document", "models": ["anthropic/claude-x"] }))).await.json();
    assert!(claude["lastOutput"]["error"]["message"].as_str().unwrap().starts_with("Agent CLI \"claude\" is not available in this image"));
    assert!(oc.find("POST", "*/prompt_async").is_empty());

    // Given no run log named by the environment, when a run is requested in a directory inside a checkout, then its row lands
    // under the checkout's root.
    env.unset("OPENCODE_DISPATCH_RUN_LOG");
    std::fs::create_dir_all(repo.join("src/deep")).unwrap();
    oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "nested"), idle(&p.session)]));
    svc.send(post("/runs").secret().json(json!({ "prompt": "p12", "shape": "document", "models": ["pp/m1"], "cwd": repo.join("src/deep") }))).await;
    let default_log = std::fs::read_to_string(repo.join(".agents/tmp/opencode-dispatch-runs.jsonl")).unwrap();
    assert_eq!(default_log.lines().count(), 1);
    env.set("OPENCODE_DISPATCH_RUN_LOG", &run_log.to_string_lossy());

    // Given a shape with no ladder, then nothing is tried; given a pool that is cold in the ledger, then its models are left out.
    let exhausted = svc.send(run(json!({ "prompt": "p7", "shape": "no-such-shape" }))).await;
    assert_eq!((exhausted.status, exhausted.json()), (StatusCode::OK, json!({ "ok": false, "attempts": [], "reason": "ladder-exhausted" })));
    let cold_until = time::OffsetDateTime::now_utc() + time::Duration::hours(1);
    let cold_until = cold_until.format(&time::format_description::well_known::Rfc3339).unwrap();
    std::fs::write(&ledger, json!({ "kimi": { "coldUntil": cold_until, "learnedResetMs": 18_000_000, "lastCapAt": "2026-01-01T00:00:00.000Z" } }).to_string()).unwrap();
    oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "served"), idle(&p.session)]));
    let warm = svc.send(run(json!({ "prompt": "p8", "shape": "document", "models": ["kimi-code-plan-global/kimi-for-coding", "pp/m1"] }))).await.json();
    assert_eq!(warm["attempts"], json!([{ "index": 0, "choice": { "model": "pp/m1" }, "verdict": { "ok": true } }]));

    // And one pinned model is run even when its pool is cold, as `opencode run -m` would
    let pinned = svc.send(run(json!({ "prompt": "p9", "shape": "document", "models": ["kimi-code-plan-global/kimi-for-coding"] }))).await.json();
    assert_eq!(pinned["attempts"], json!([{ "index": 0, "choice": { "model": "kimi-code-plan-global/kimi-for-coding", "pool": "kimi" }, "verdict": { "ok": true } }]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_the_shared_cooldown_ledger_is_unavailable_when_a_run_is_requested_then_it_is_refused_with_why_and_nothing_runs() {
    // Given a service whose cooldown ledger cannot be opened
    let env = Env::new(&[("OPENCODE_LADDER_IGNORE_STATS", "1")]);
    let repo = env.dir("repo");
    std::fs::create_dir(repo.join(".git")).unwrap();
    let run_log = env.home.join("runs.jsonl");
    env.set("OPENCODE_DISPATCH_RUN_LOG", &run_log.to_string_lossy());
    let svc = service(&env, Options { ledger_error: Some("POOL_COOLDOWNS_ORDS_BASE_URL must be set"), ..Options::default() });
    // When a run is requested
    let answer = svc.send(post("/runs").secret().json(json!({ "prompt": "p", "cwd": repo.to_string_lossy() }))).await;
    // Then it is refused with the reason, and no attempt was made or logged
    assert_eq!((answer.status, answer.json()), (StatusCode::SERVICE_UNAVAILABLE, json!({ "error": "POOL_COOLDOWNS_ORDS_BASE_URL must be set", "code": "LEDGER_UNAVAILABLE" })));
    assert!(!run_log.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_attached_text_and_binary_files_and_no_cwd_when_a_run_is_requested_then_it_runs_in_a_staging_directory_holding_them_that_is_kept_for_the_session() {
    // Given an ask root, a run log outside it, and a model that answers once it has seen the staged file
    let env = Env::new(&[("OPENCODE_LADDER_IGNORE_STATS", "1")]);
    let asks = env.dir("asks");
    env.set("RCA_ASK_ROOT", &asks.to_string_lossy());
    env.set("OPENCODE_DISPATCH_RUN_LOG", &env.home.join("runs.jsonl").to_string_lossy());
    env.set("OPENCODE_POOL_LEDGER", &env.home.join("pools.json").to_string_lossy());
    let svc = service(&env, Options::default());
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let root = asks.clone();
    let log = Arc::clone(&seen);
    svc.opencode.on_prompt(Arc::new(move |p: &Prompt| {
        let staged = std::fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
        log.lock().unwrap().push(std::fs::read_to_string(staged.join("docs/a.md")).unwrap_or_default());
        log.lock().unwrap().push(format!("{:?}", std::fs::read(staged.join("shots/a.png")).unwrap_or_default()));
        vec![says(&p.session, "summary"), idle(&p.session)]
    }));
    // When a run is requested with a file and no cwd
    let answer = svc.send(post("/runs").secret().json(json!({ "prompt": "p", "shape": "document", "models": ["pp/m1"], "files": [{ "path": "docs/a.md", "content": "alpha" }, { "path": "shots/a.png", "contentBase64": "iVBORw==" }] }))).await;
    // Then the model saw both files in a directory under the ask root, the run answered, and the directory is kept for the session it made
    assert_eq!((answer.status, answer.json()["output"]["text"].clone()), (StatusCode::OK, json!("summary")));
    assert_eq!(*seen.lock().unwrap(), vec!["alpha".to_string(), "[137, 80, 78, 71]".to_string()]);
    let directory = svc.opencode.find("POST", "/session")[0].directory().map(str::to_string).unwrap();
    let session = answer.json()["session"].as_str().unwrap().to_string();
    assert!(directory.starts_with(asks.to_str().unwrap()));
    assert_eq!(std::fs::read_to_string(std::path::Path::new(&directory).join(".rca-session")).unwrap(), session);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_file_path_that_leaves_the_directory_or_a_cwd_or_bad_content_beside_files_when_a_run_is_requested_then_it_is_refused_before_any_attempt() {
    // Given a service with an ask root
    let env = Env::new(&[]);
    let asks = env.dir("asks");
    env.set("RCA_ASK_ROOT", &asks.to_string_lossy());
    let svc = service(&env, Options::default());
    // When files climb out, or a cwd comes with files
    for body in [
        json!({ "prompt": "p", "files": [{ "path": "../x", "content": "c" }] }),
        json!({ "prompt": "p", "files": [{ "path": "/etc/x", "content": "c" }] }),
        json!({ "prompt": "p", "cwd": "/tmp", "files": [{ "path": "a", "content": "c" }] }),
        json!({ "prompt": "p", "files": [{ "path": "a", "content": "c", "contentBase64": "Yw==" }] }),
        json!({ "prompt": "p", "files": [{ "path": "a", "contentBase64": "not base64!" }] }),
    ] {
        let answer = svc.send(post("/runs").secret().json(body)).await;
        // Then each is a 400 and nothing was prompted
        assert_eq!((answer.status, answer.json()["code"].clone()), (StatusCode::BAD_REQUEST, json!("INVALID_REQUEST")));
    }
    assert!(svc.opencode.find("POST", "*/prompt_async").is_empty());
    assert_eq!(std::fs::read_dir(&asks).unwrap().count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_run_that_rewrote_created_and_removed_staged_files_when_it_answers_then_the_changes_come_back_and_the_log_and_cooldowns_are_readable() {
    // Given three staged files and a model that rewrites one, creates one and removes one
    let env = Env::new(&[("OPENCODE_LADDER_IGNORE_STATS", "1")]);
    let asks = env.dir("asks");
    env.set("RCA_ASK_ROOT", &asks.to_string_lossy());
    let (run_log, pools) = (env.home.join("runs.jsonl"), env.home.join("pools.json"));
    env.set("OPENCODE_DISPATCH_RUN_LOG", &run_log.to_string_lossy());
    env.set("OPENCODE_POOL_LEDGER", &pools.to_string_lossy());
    let svc = service(&env, Options::default());
    let root = asks.clone();
    svc.opencode.on_prompt(Arc::new(move |p: &Prompt| {
        let staged = std::fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
        std::fs::write(staged.join("keep.md"), "rewritten").unwrap();
        std::fs::write(staged.join("new/made.md"), "created").unwrap_or_else(|_| {
            std::fs::create_dir_all(staged.join("new")).unwrap();
            std::fs::write(staged.join("new/made.md"), "created").unwrap();
        });
        std::fs::remove_file(staged.join("gone.md")).unwrap();
        vec![says(&p.session, "done"), idle(&p.session)]
    }));
    let files = json!([{ "path": "keep.md", "content": "original" }, { "path": "same.md", "content": "untouched" }, { "path": "gone.md", "content": "bye" }]);

    // When the run is requested
    let answer = svc.send(post("/runs").secret().json(json!({ "prompt": "p", "shape": "document", "models": ["pp/m1"], "files": files }))).await;

    // Then only what changed comes back
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["changes"], json!([{ "path": "gone.md", "deleted": true }, { "path": "keep.md", "content": "rewritten" }, { "path": "new/made.md", "content": "created" }]));

    // And the run log is served as text, newest rows first-limited, and a cold pool is warmed again through the ledger
    let log = svc.send(get("/runs/log?limit=1").secret()).await.json();
    assert_eq!(log["log"].as_str().unwrap().lines().count(), 1);
    std::fs::write(&pools, json!({ "kimi": { "coldUntil": "2999-01-01T00:00:00.000Z", "learnedResetMs": 1, "lastCapAt": "2026-10-27T10:00:00.000Z" } }).to_string()).unwrap();
    let cleared = svc.send(post("/runs/cooldowns/clear").secret().json(json!({ "pool": "kimi" }))).await;
    assert_eq!((cleared.status, cleared.json()), (StatusCode::OK, json!({ "pool": "kimi", "wasCold": true })));
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&pools).unwrap()).unwrap();
    assert!(after["kimi"]["coldUntil"].as_str().unwrap() < "2999");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_session_when_a_later_turn_names_it_then_it_continues_there_with_the_same_files_and_only_new_changes_come_back() {
    // Given a first run that staged a file and a model that rewrites it
    let env = Env::new(&[("OPENCODE_LADDER_IGNORE_STATS", "1")]);
    let asks = env.dir("asks");
    env.set("RCA_ASK_ROOT", &asks.to_string_lossy());
    env.set("OPENCODE_DISPATCH_RUN_LOG", &env.home.join("runs.jsonl").to_string_lossy());
    env.set("OPENCODE_POOL_LEDGER", &env.home.join("pools.json").to_string_lossy());
    let svc = service(&env, Options::default());
    let root = asks.clone();
    let turn = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&turn);
    svc.opencode.on_prompt(Arc::new(move |p: &Prompt| {
        let dir = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).find(|d| d.is_dir()).unwrap();
        let n = counter.fetch_add(1, Ordering::SeqCst);
        let note = std::fs::read_to_string(dir.join("notes.md")).unwrap_or_default();
        std::fs::write(dir.join("notes.md"), format!("{note}turn {n};")).unwrap();
        vec![says(&p.session, &format!("saw [{note}]")), idle(&p.session)]
    }));
    let first = svc.send(post("/runs").secret().json(json!({ "prompt": "one", "shape": "document", "models": ["pp/m1"], "files": [{ "path": "notes.md", "content": "start;" }, { "path": "keep.md", "content": "k" }] }))).await.json();
    let session = first["session"].as_str().unwrap().to_string();
    assert_eq!(first["output"]["text"], json!("saw [start;]"));

    // When a second turn names the session, with a new file and without resending the others
    let second = svc.send(post("/runs").secret().json(json!({ "prompt": "two", "shape": "document", "models": ["pp/m2"], "session": session, "files": [{ "path": "extra.md", "content": "e" }] }))).await.json();

    // Then it ran in the same session and directory, saw the first turn's write, and only what the second turn changed is returned
    assert_eq!(second["session"], json!(session));
    assert_eq!(second["output"]["text"], json!("saw [start;turn 0;]"));
    let prompts = svc.opencode.find("POST", "*/prompt_async");
    assert_eq!(prompts.len(), 2);
    assert!(prompts[1].path.contains(&session), "{}", prompts[1].path);
    assert_eq!(svc.opencode.find("POST", "/session").len(), 1, "the second turn created no session");
    assert_eq!(second["changes"], json!([{ "path": "notes.md", "content": "start;turn 0;turn 1;" }]));

    // And a session nobody here made is refused before any attempt
    let unknown = svc.send(post("/runs").secret().json(json!({ "prompt": "x", "session": "ses_nope" }))).await;
    assert_eq!((unknown.status, unknown.json()["code"].clone()), (StatusCode::BAD_REQUEST, json!("INVALID_REQUEST")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_agent_variant_title_and_no_auto_when_a_run_is_requested_then_opencode_gets_them_with_the_run_rules_and_the_edit_still_returns() {
    // Given a model that writes a file
    let env = Env::new(&[("OPENCODE_LADDER_IGNORE_STATS", "1")]);
    let asks = env.dir("asks");
    env.set("RCA_ASK_ROOT", &asks.to_string_lossy());
    env.set("OPENCODE_DISPATCH_RUN_LOG", &env.home.join("runs.jsonl").to_string_lossy());
    env.set("OPENCODE_POOL_LEDGER", &env.home.join("pools.json").to_string_lossy());
    let svc = service(&env, Options::default());
    let root = asks.clone();
    svc.opencode.on_prompt(Arc::new(move |p: &Prompt| {
        let dir = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).find(|d| d.is_dir()).unwrap();
        std::fs::write(dir.join("a.md"), "changed").unwrap();
        vec![says(&p.session, "ok"), idle(&p.session)]
    }));
    // what `opencode run` creates its session with: no question, no planning
    let run_rules = json!([
        { "permission": "question", "action": "deny", "pattern": "*" },
        { "permission": "plan_enter", "action": "deny", "pattern": "*" },
        { "permission": "plan_exit", "action": "deny", "pattern": "*" },
    ]);

    // When a run without auto names an agent, a variant and a title
    let answer = svc.send(post("/runs").secret().json(json!({ "prompt": "p", "shape": "document", "models": ["pp/m1"], "auto": false, "agent": "plan", "variant": "high", "title": "the title", "files": [{ "path": "a.md", "content": "a" }] }))).await.json();

    // Then the session was created titled with what opencode would ask about rejected, the prompt carried agent and variant, and the edit returns
    let created = svc.opencode.find("POST", "/session")[0].body.clone().unwrap();
    assert_eq!(created["title"], json!("the title"));
    assert_eq!(created["permission"], run_rules);
    let prompt = svc.opencode.find("POST", "*/prompt_async")[0].body.clone().unwrap();
    assert_eq!((prompt["agent"].clone(), prompt["variant"].clone()), (json!("plan"), json!("high")));
    assert_eq!(answer["changes"], json!([{ "path": "a.md", "content": "changed" }]));

    // And a run with auto or without the field creates its session the same way: what opencode asks is answered, not ruled on in advance
    svc.opencode.forget_requests();
    svc.send(post("/runs").secret().json(json!({ "prompt": "p", "shape": "document", "models": ["pp/m1"], "auto": true }))).await;
    svc.send(post("/runs").secret().json(json!({ "prompt": "p", "shape": "document", "models": ["pp/m1"] }))).await;
    let created: Vec<Value> = svc.opencode.find("POST", "/session").iter().map(|r| r.body.clone().unwrap()["permission"].clone()).collect();
    assert_eq!(created, vec![run_rules.clone(), run_rules]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_session_whose_kept_directory_has_expired_when_the_next_run_starts_then_opencode_is_told_to_drop_it() {
    // Given a service whose kept sessions expire at once, and a first run that made a session
    let env = Env::new(&[]);
    let asks = env.dir("asks");
    env.set("RCA_ASK_ROOT", &asks.to_string_lossy());
    env.set("RCA_SESSION_TTL_HOURS", "0");
    let svc = service(&env, Options::default());
    svc.opencode.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "ok"), idle(&p.session)]));
    let first = svc.send(post("/runs").secret().json(json!({ "prompt": "p", "shape": "document", "models": ["pp/m1"], "files": [{ "path": "a.md", "content": "a" }] }))).await;
    let session = first.json()["session"].as_str().unwrap().to_string();
    assert!(svc.opencode.find("DELETE", &format!("/session/{session}")).is_empty(), "a session is not dropped by the run that makes it");

    // When the next run starts
    let _ = svc.send(post("/runs").secret().json(json!({ "prompt": "p", "shape": "document", "models": ["pp/m1"], "files": [{ "path": "b.md", "content": "b" }] }))).await;

    // Then opencode is asked to delete the expired session, so the server gives its memory back
    assert_eq!(svc.opencode.find("DELETE", &format!("/session/{session}")).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_sessions_of_two_callers_when_one_caller_purges_then_only_its_sessions_are_dropped_from_opencode_and_disk() {
    // Given two callers that each made a session
    let env = Env::new(&[]);
    let asks = env.dir("asks");
    env.set("RCA_ASK_ROOT", &asks.to_string_lossy());
    let svc = service(&env, Options::default());
    svc.opencode.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "ok"), idle(&p.session)]));
    let made = |caller: &str| json!({ "prompt": "p", "shape": "document", "models": ["pp/m1"], "caller": caller, "files": [{ "path": "a.md", "content": "a" }] });
    let mine = svc.send(post("/runs").secret().json(made("e2e-one"))).await.json()["session"].as_str().unwrap().to_string();
    let theirs = svc.send(post("/runs").secret().json(made("someone-else"))).await.json()["session"].as_str().unwrap().to_string();

    // When the first caller purges
    let answer = svc.send(delete("/runs/sessions?caller=e2e-one").secret()).await;

    // Then its session is deleted from opencode and its directory is gone, and the other caller's is untouched
    assert_eq!((answer.status, answer.json()["dropped"].clone()), (StatusCode::OK, json!(1)));
    assert_eq!(svc.opencode.find("DELETE", &format!("/session/{mine}")).len(), 1);
    assert!(svc.opencode.find("DELETE", &format!("/session/{theirs}")).is_empty());
    let kept: Vec<String> = std::fs::read_dir(&asks).unwrap().flatten().filter_map(|e| std::fs::read_to_string(e.path().join(".rca-session")).ok()).collect();
    assert_eq!(kept, vec![theirs]);

    // And a purge with no caller is refused
    assert_eq!(svc.send(delete("/runs/sessions").secret()).await.status, StatusCode::BAD_REQUEST);
}
