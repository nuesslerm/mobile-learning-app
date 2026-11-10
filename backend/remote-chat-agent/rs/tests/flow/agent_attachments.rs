//! `attachments` end to end: bytes kept on the service volume, written into the workspace by the real `rca-exec` (`put_blob`), and named by
//! `turn.start` so the scripted CLI the service starts can read them with its file tool. Every recorded exchange of the contract is replayed.
use crate::agent_threads::{chain, create, kinds, said};
use crate::harness::Env;
use base64::Engine;
use exec_testkit::script::Turn;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn put(id: &str, name: &str, bytes: &[u8]) -> Value {
    json!({"id": id, "name": name, "mime_type": "text/plain", "kind": "file", "data": b64(bytes)})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_an_attachment_sent_with_a_turn_when_the_cli_runs_then_the_executor_wrote_it_into_the_workspace_and_the_cli_read_it_with_its_file_tool() {
    // Given the real chain, an attachment kept under a name that tries to climb out, and a CLI whose turn reads the file where the prompt says it is
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    let bytes = b"the note the person attached\n";
    let mut agent = chain.agent().await;
    let kept = agent.ask("upload.put", put("att-note", "../../notes.txt", bytes)).await;
    assert_eq!(kept["result"]["attachment"]["size_bytes"], json!(bytes.len()), "{kept}");
    let at = ".rca/attachments/att-note/notes.txt";
    chain.script(vec![Turn::replaying(&[said("reading it")]).then_calling("k1", "read_file", json!({"path": at})).then_line(&said("read it"))]);
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    // When a turn names the attachment
    let started = agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent", "prompt": "summarize the attachment", "attachments": ["att-note"]})).await;
    let events = agent.events_through("turn.finished").await;
    // Then the executor wrote the bytes where the contract says, below the attachment directory and nowhere else in the repository
    assert!(started.get("result").is_some(), "{started}");
    assert_eq!(std::fs::read(chain.repo.path().join(at)).unwrap(), bytes);
    assert!(!chain.repo.path().join("notes.txt").exists() && !chain.repo.path().parent().unwrap().join("notes.txt").exists());
    // And the directory ignores itself, so the repository's status shows nothing of it
    assert_eq!(chain.repo.git(&["status", "--porcelain"]), "");
    // And the CLI was started with the prompt and then the path, and the thread records only what the person said
    let prompt = chain.cli.invocations("claude").iter().flat_map(|start| start.argv.clone()).find(|a| a.starts_with("summarize the attachment")).expect("the prompt reached the CLI");
    assert!(prompt.ends_with(&format!("\n- {at}")), "{prompt}");
    assert_eq!(events[1].data["text"], json!("summarize the attachment"));
    // And the CLI's read of the file through the executor returned the bytes
    assert_eq!(kinds(&events), ["turn.started", "message.delta", "message.delta", "tool.started", "tool.output", "tool.finished", "message.delta", "turn.finished"]);
    assert_eq!((events[3].data["tool"].clone(), events[3].data["args"].clone()), (json!("read_file"), json!({"path": at})));
    let output = serde_json::to_string(&events[4].data).unwrap();
    assert!(output.contains("the note the person attached"), "{output}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_naming_an_attachment_nobody_kept_or_a_stage_repeated_when_asked_then_the_first_is_refused_before_any_cli_starts_and_the_second_is_the_same_file() {
    // Given the chain and one kept attachment
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    let mut agent = chain.agent().await;
    agent.ask("upload.put", put("att-one", "one.txt", b"1")).await;
    create(&mut agent, "t1").await;
    // When a turn names an id nobody kept, Then it is ATTACHMENT_NOT_FOUND, no CLI started, and the thread recorded nothing
    let refused = agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent", "prompt": "p", "attachments": ["att-one", "nope"]})).await;
    assert_eq!(refused["error"], json!({"code": "ATTACHMENT_NOT_FOUND", "message": "no attachment has the id nope"}));
    assert!(chain.cli.invocations("claude").is_empty());
    assert!(!chain.repo.path().join(".rca").exists(), "nothing is written when an id is unknown");
    assert_eq!(agent.ask("thread.history", json!({"thread": "t1"})).await["result"]["events"], json!([]));
    // When the attachment is staged twice
    let first = agent.ask("attachment.stage", json!({"workspace": "w1", "ids": ["att-one"]})).await;
    let second = agent.ask("attachment.stage", json!({"workspace": "w1", "ids": ["att-one"]})).await;
    // Then both answer the same path, the file is unchanged, and it is readable only by its owner
    assert_eq!(first["result"], second["result"]);
    let file = chain.repo.path().join(".rca/attachments/att-one/one.txt");
    assert_eq!(std::fs::read(&file).unwrap(), b"1");
    assert_eq!(std::fs::metadata(env.home.join(".local/state/rca/attachments/att-one/blob")).unwrap().permissions().mode() & 0o777, 0o600);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_attachments_kept_by_one_service_when_it_restarts_then_the_new_one_still_serves_their_bytes_and_ids_cannot_leave_the_volume() {
    // Given an attachment kept by one service
    let env = Env::process(&[]);
    let first = crate::harness::service(&env, crate::harness::Options::default());
    let port = first.listen().await;
    let mut agent = crate::mapper::Agent::open(port, &first.mint("flow mapper").await).await;
    agent.ask("upload.put", put("att-keep", "keep.bin", &[0, 1, 2, 255])).await;
    // When a second service starts on the same home (a restart)
    let second = crate::harness::service(&env, crate::harness::Options::default());
    let port = second.listen().await;
    let mut again = crate::mapper::Agent::open(port, &second.mint("flow mapper").await).await;
    let read = again.ask("attachment.get", json!({"id": "att-keep"})).await;
    // Then it reads the same bytes, and ids that would name another place are refused before the volume is touched
    assert_eq!(read["result"]["data"], json!(b64(&[0, 1, 2, 255])), "{read}");
    for bad in ["../providers", "a/b", ".hidden", ""] {
        let refused = again.ask("attachment.get", json!({"id": bad})).await;
        assert_eq!(refused["error"]["code"], json!("INVALID_PARAMS"), "{bad}: {refused}");
    }
    assert_eq!(std::fs::read_dir(env.home.join(".local/state/rca/attachments")).unwrap().count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn every_method_of_the_attachments_contract_has_recorded_exchanges_that_the_service_answers_as_recorded_over_the_real_executor() {
    // Given the recorded exchanges of the attachments crate and its methods
    let recorded = agent_api_attachments::testing::fixtures();
    let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
    named.sort_unstable();
    let mut methods: Vec<&str> = agent_api_attachments::METHODS.iter().map(|m| m.name()).collect();
    methods.sort_unstable();
    assert_eq!(named, methods);
    for fixture in &recorded {
        for exchange in &fixture.exchanges {
            // When an exchange's setup and request are made on a service and executor of their own
            let env = Env::process(&[]);
            let chain = chain(&env).await;
            let mut agent = chain.agent().await;
            for before in &exchange.before {
                let answered = agent.ask(before["method"].as_str().unwrap(), before["params"].clone()).await;
                assert!(answered.get("result").is_some(), "{}: the setup {before} failed: {answered}", exchange.name);
            }
            let mut answer = agent.ask(exchange.request["method"].as_str().unwrap(), exchange.request["params"].clone()).await;
            answer["id"] = exchange.response["id"].clone();
            // Then the answer is the recorded one, and the executor actions it recorded are ones the table declares for the method
            assert_eq!(exchange.masked(answer), exchange.masked(exchange.response.clone()), "{}: {}", fixture.method, exchange.name);
            let declared: Vec<&str> = rca_group_attachments::SERVED.iter().find(|s| s.method.name() == fixture.method).map(|s| s.actions.to_vec()).unwrap();
            assert!(exchange.actions.iter().all(|a| declared.contains(&a.as_str())), "{}: {} calls {:?}, the table declares {declared:?}", fixture.method, exchange.name, exchange.actions);
        }
    }
}
