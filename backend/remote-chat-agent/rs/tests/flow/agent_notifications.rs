//! `service.notifications` and `service.panes` end to end: a turn of a scripted CLI the real executor serves finishes, and the journal holds the
//! entry for it, a stream open at the time is pushed it, and the pane of its thread is done and unread until it is marked read, then cleared, and
//! comes back with the next turn. Every recorded exchange of the two sub-groups is replayed.
use crate::agent_threads::{chain, create, said, start_turn};
use crate::harness::{agent_api, Env};
use agent_api_proto::testing::Fixture;
use exec_testkit::script::Turn;
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_finishes_when_the_journal_and_the_panes_are_read_then_the_entry_the_pushed_event_and_the_pane_agree_and_the_marks_move_the_pane() {
    // Given the real chain, a CLI that answers two turns, and a mapper that follows the journal from now
    let env = Env::new(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("first")]), Turn::replaying(&[said("second")])]);
    let mut agent = chain.agent().await;
    let opened = agent.ask("notification.subscribe", json!({})).await;
    assert_eq!((opened["result"]["topic"].clone(), opened["result"]["seq"].clone(), opened["result"]["reset"].clone()), (json!("notifications"), json!(0), json!(false)));
    let epoch = opened["result"]["epoch"].as_str().unwrap().to_string();
    create(&mut agent, "t1").await;
    let idle = agent.ask("pane.list", json!({})).await;
    assert_eq!((idle["result"]["panes"][0]["pane"].clone(), idle["result"]["panes"][0]["state"].clone(), idle["result"]["panes"][0]["unread"].clone()), (json!("thread:t1"), json!("idle"), json!(false)));
    // When a turn runs to its end
    start_turn(&mut agent, "t1").await;
    let pushed = agent.event().await;
    // Then the stream was pushed the entry, numbered 1, naming the thread, its title and workspace
    assert_eq!((pushed.kind.as_str(), pushed.seq), ("notification.posted", 1), "{pushed:?}");
    let field = |name: &str| pushed.data.get(name).cloned().unwrap_or(Value::Null);
    assert_eq!((field("cause"), field("title"), field("thread_id"), field("workspace")), (json!("turn.completed"), json!("Notes"), json!("t1"), json!("w1")), "{pushed:?}");
    // And the journal answers the same entry since 0, nothing since 1, and everything kept for a caller of another run
    let since = agent.ask("notification.since", json!({"after_seq": 0, "epoch": epoch})).await;
    assert_eq!(since["result"]["notifications"][0]["id"], pushed.data["id"]);
    assert_eq!(agent.ask("notification.since", json!({"after_seq": 1, "epoch": epoch})).await["result"]["notifications"], json!([]));
    assert_eq!(agent.ask("notification.since", json!({"after_seq": 1, "epoch": "an-earlier-run"})).await["result"]["notifications"].as_array().map(Vec::len), Some(1));
    // And the pane of the thread is done, unread and clearable, in its workspace
    let done = &agent.ask("pane.list", json!({"workspace": "w1"})).await["result"]["panes"][0];
    assert_eq!((done["state"].clone(), done["unread"].clone(), done["clearable"].clone(), done["listed"].clone()), (json!("done"), json!(true), json!(true), json!(true)));
    // And a read, an unread and a clear move it, and the next turn lists it again as working
    let marked = |agent_answer: Value| agent_answer["result"]["marked"].clone();
    assert_eq!(marked(agent.ask("pane.mark", json!({"panes": ["thread:t1"], "mark": "read"})).await), json!(1));
    assert_eq!(agent.ask("pane.list", json!({})).await["result"]["panes"][0]["unread"], json!(false));
    assert_eq!(marked(agent.ask("pane.mark", json!({"panes": ["thread:t1"], "mark": "clear"})).await), json!(1));
    assert_eq!(agent.ask("pane.list", json!({})).await["result"]["panes"][0]["listed"], json!(false));
    start_turn(&mut agent, "t1").await;
    let second = agent.event().await;
    assert_eq!((second.seq, second.data["cause"].clone()), (2, json!("turn.completed")));
    let again = &agent.ask("pane.list", json!({})).await["result"]["panes"][0];
    assert_eq!((again["listed"].clone(), again["state"].clone(), again["unread"].clone()), (json!(true), json!("done"), json!(true)));
}

async fn replay(fixtures: Vec<Fixture>, declared: &[&str]) {
    let mut named: Vec<&str> = fixtures.iter().map(|f| f.method.as_str()).collect();
    named.sort_unstable();
    let mut methods = declared.to_vec();
    methods.sort_unstable();
    assert_eq!(named, methods);
    for fixture in &fixtures {
        for exchange in &fixture.exchanges {
            let env = Env::new(&[]);
            let chain = chain(&env).await;
            let mut agent = chain.agent().await;
            for before in &exchange.before {
                let answered = agent.ask(before["method"].as_str().unwrap(), before["params"].clone()).await;
                assert!(answered.get("result").is_some(), "{}: the setup {before} failed: {answered}", exchange.name);
            }
            let mut answer = agent.ask(exchange.request["method"].as_str().unwrap(), exchange.request["params"].clone()).await;
            answer["id"] = exchange.response["id"].clone();
            assert_eq!(exchange.masked(answer), exchange.masked(exchange.response.clone()), "{}: {}", fixture.method, exchange.name);
            assert!(exchange.actions.is_empty(), "{}: {} calls an executor, the group asks none", fixture.method, exchange.name);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn every_method_of_the_notifications_and_panes_contracts_has_recorded_exchanges_that_the_service_answers_as_recorded() {
    // Given the recorded exchanges of the two sub-groups and their methods, none of which asks an executor
    let notifications: Vec<&str> = agent_api_service::notifications::METHODS.iter().map(|m| m.name()).collect();
    let panes: Vec<&str> = agent_api_service::panes::METHODS.iter().map(|m| m.name()).collect();
    // Then each method has one fixture and the service answers every exchange as recorded
    replay(agent_api_service::testing::group_fixtures("notifications"), &notifications).await;
    replay(agent_api_service::testing::group_fixtures("panes"), &panes).await;
    for group in [agent_api_service::notifications::GROUP, agent_api_service::panes::GROUP] {
        assert!(agent_api(&Env::detached()).groups.served().any(|s| s.method.group() == group && s.actions.is_empty()), "{} is registered", group.as_str());
    }
}
