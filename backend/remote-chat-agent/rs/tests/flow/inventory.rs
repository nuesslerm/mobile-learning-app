use crate::doubles::Run;
use crate::harness::{get, post, service, Env, Options};
use axum::http::StatusCode;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_picker_when_it_asks_for_agent_clis_models_mcp_servers_skills_and_commands_then_the_service_answers_from_its_settings_and_opencode() {
    let env = Env::new(&[("OPENCODE_TEXT_MODEL", "prov-a/text-model"), ("OPENCODE_VISION_MODEL", "prov-b/vision-model"), ("OPENCODE_CWD", "/tmp")]);
    let svc = service(&env, Options::default());
    let oc = &svc.opencode;

    // When the agent CLIs are listed, then each backend is described, opencode is available and selected, and the others say whether
    // they are; the model preferences are the overrides and the defaults for the rest.
    let clis = svc.send(get("/api/agent-clis")).await.json();
    assert_eq!(clis["data"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect::<Vec<_>>(), ["opencode", "claude", "codex"]);
    assert_eq!(clis["data"][0], json!({ "id": "opencode", "description": "opencode serve, reached over HTTP. No heavy local dependency.", "available": true, "selected": true }));
    assert_eq!(clis["data"][1]["description"], "claude CLI, driven in-process via the Anthropic Agent SDK.");
    assert_eq!(clis["data"][2]["description"], "codex CLI, driven in-process via its app-server subprocess mode.");
    for other in &clis["data"].as_array().unwrap()[1..] {
        assert!(other["available"].is_boolean() && other["selected"] == false);
    }
    let preferences = svc.send(get("/api/models/preferences")).await;
    assert_eq!(preferences.json(), json!({ "text": "prov-a/text-model", "vision": "prov-b/vision-model", "complex": "kimi-code-plan-global/kimi-for-coding" }));

    // Given no single plain directory, when a proxy is asked, then 400 and opencode is not called.
    for path in ["/api/mcp", "/api/skills", "/api/commands", "/api/models", "/api/mcp?directory=", "/api/mcp?directory=a&directory=b", "/api/mcp?directory[]=a"] {
        let refused = svc.send(get(path)).await;
        assert_eq!((refused.status, refused.json()), (StatusCode::BAD_REQUEST, json!({ "error": "Missing required query parameter: directory" })), "{path}");
    }
    assert!(oc.requests().is_empty());

    // When MCP servers are listed for a directory, toggled, and skills, commands and models are listed, then opencode is asked for
    // that directory and its answers come back, the commands split into skills and user commands with a template.
    oc.respond("GET", "/mcp", 200, json!({ "github": { "status": "connected" } }));
    oc.respond("POST", "/mcp/github/connect", 200, json!(true));
    oc.respond("POST", "/mcp/github/disconnect", 200, json!(false));
    oc.respond("GET", "/command", 200, json!([
        { "name": "review", "source": "command", "description": "built in" },
        { "name": "plan-it", "source": "command", "description": "Plan work", "template": "plan $ARGS" },
        { "name": "deploy", "source": "skill", "description": "Ship it", "template": "do $ARGS" },
        { "name": "bare", "source": "skill" },
    ]));
    oc.respond("GET", "/api/model", 200, json!({ "data": [{ "id": "m1", "name": "Model One", "providerID": "p" }] }));
    assert_eq!(svc.send(get("/api/mcp?x=1&directory=%2Fwork%2Fa%20b")).await.json(), json!({ "github": { "status": "connected" } }));
    assert_eq!(svc.send(post("/api/mcp/github/connect")).await.json(), json!({ "ok": true }));
    assert_eq!(svc.send(post("/api/mcp/github/disconnect")).await.json(), json!({ "ok": false }));
    assert_eq!(svc.send(get("/api/skills?directory=%2Fw")).await.json(), json!({ "data": [{ "name": "deploy", "description": "Ship it", "template": "do $ARGS" }, { "name": "bare", "description": "", "template": "" }] }));
    assert_eq!(svc.send(get("/api/commands?directory=%2Fw")).await.json(), json!({ "data": [{ "name": "plan-it", "description": "Plan work" }] }));
    assert_eq!(svc.send(get("/api/models?directory=%2Fw")).await.json(), json!({ "data": [{ "id": "m1", "name": "Model One", "providerID": "p" }] }));
    let asked: Vec<(String, String, Option<String>)> = oc.requests().into_iter().map(|r| (r.method.clone(), r.path.clone(), r.directory().map(str::to_string))).collect();
    assert_eq!(
        asked,
        [
            ("GET".into(), "/mcp".into(), Some("/work/a b".into())),
            ("POST".into(), "/mcp/github/connect".into(), None),
            ("POST".into(), "/mcp/github/disconnect".into(), None),
            ("GET".into(), "/command".into(), Some("/w".into())),
            ("GET".into(), "/command".into(), Some("/w".into())),
            ("GET".into(), "/api/model".into(), Some("/w".into())),
        ]
    );

    // Given sessions opencode recorded, when the recent models are asked, then they come grouped and counted, most used first, named
    // from the live model list when it knows them; an empty history is an empty list.
    env.set("OPENCODE_DB_PATH", "/data/opencode.db");
    oc.respond("GET", "/api/model", 200, json!({ "data": [{ "id": "m-a", "name": "Model A" }] }));
    svc.commands.answer(|run: &Run| (run.program == "sqlite3").then(|| run.ok("p1|m-a|2\np2|m-b|1\n|m-c|1\n")).flatten());
    let recent = svc.send(get("/api/models/recent")).await;
    assert_eq!(recent.json(), json!({ "data": [{ "id": "m-a", "providerID": "p1", "name": "Model A", "count": 2 }, { "id": "m-b", "providerID": "p2", "name": "m-b", "count": 1 }] }));
    let query = &svc.commands.of("sqlite3")[0];
    assert_eq!(query.args[0], "/data/opencode.db");
    assert!(query.args[1].contains("GROUP BY providerID, modelID") && query.args[1].contains("ORDER BY cnt DESC"));
    assert_eq!(oc.find("GET", "/api/model").last().unwrap().directory(), Some("/tmp"));
    svc.commands.answer(|run: &Run| (run.program == "sqlite3").then(|| run.ok("")).flatten());
    assert_eq!(svc.send(get("/api/models/recent")).await.json(), json!({ "data": [] }));

    // When the database cannot be read (by default under the home directory), then 502 with an empty list.
    env.unset("OPENCODE_DB_PATH");
    svc.commands.answer(|run: &Run| (run.program == "sqlite3").then(|| run.fails("", "Error: unable to open database\n")).flatten());
    let unreadable = svc.send(get("/api/models/recent")).await;
    assert_eq!(unreadable.status, StatusCode::BAD_GATEWAY);
    assert_eq!(unreadable.json()["data"], json!([]));
    assert!(unreadable.json()["error"].as_str().unwrap().starts_with("Error: Command failed: sqlite3 "));
    assert_eq!(svc.commands.of("sqlite3").last().unwrap().args[0], format!("{}/.local/share/opencode/opencode.db", env.home.display()));

    // When opencode answers 500, then each proxy answers 502 naming the opencode call.
    for (url, path) in [("/api/mcp?directory=%2Fw", "/mcp"), ("/api/skills?directory=%2Fw", "/command"), ("/api/commands?directory=%2Fw", "/command"), ("/api/models?directory=%2Fw", "/api/model")] {
        oc.respond("GET", path, 500, json!({ "error": "boom" }));
        let failed = svc.send(get(url)).await;
        assert_eq!(failed.status, StatusCode::BAD_GATEWAY);
        assert_eq!(failed.json(), json!({ "error": format!("Error: opencode {path}?directory=%2Fw responded with 500") }));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_opencode_replies_to_the_mcp_calls_in_a_shape_the_contract_does_not_know_when_they_are_proxied_then_the_service_answers_502_and_a_valid_reply_passes() {
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let oc = &svc.opencode;
    let list = || get("/api/mcp?directory=%2Fw");

    // Given opencode answering the list with an array, a string, a status it has no name for, or a failure without its error, when the
    // list is asked for, then 502 `unexpected_mcp_reply` each time (the arrived probe: opencode was asked).
    for (n, reply) in [json!([]), json!("nope"), json!({ "github": { "status": "exploding" } }), json!({ "github": { "status": "failed" } }), json!({ "github": "connected" })].into_iter().enumerate() {
        oc.respond("GET", "/mcp", 200, reply.clone());
        let answered = svc.send(list()).await;
        assert_eq!((answered.status, answered.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "unexpected_mcp_reply" })), "{reply}");
        assert_eq!(oc.find("GET", "/mcp").len(), n + 1);
    }

    // Given the map opencode declares, when it is listed, then it passes as it is.
    let declared = json!({ "a": { "status": "connected" }, "b": { "status": "disabled" }, "c": { "status": "failed", "error": "boom" }, "d": { "status": "needs_auth" }, "e": { "status": "needs_client_registration", "error": "register" } });
    oc.respond("GET", "/mcp", 200, declared.clone());
    assert_eq!(svc.send(list()).await.json(), declared);

    // When a server is toggled and opencode answers something but a boolean, then 502; a boolean becomes `{ ok }`.
    oc.respond("POST", "/mcp/a/connect", 200, json!({ "status": "connected" }));
    let toggled = svc.send(post("/api/mcp/a/connect")).await;
    assert_eq!((toggled.status, toggled.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "unexpected_mcp_reply" })));
    assert_eq!(oc.find("POST", "/mcp/a/connect").len(), 1);
    oc.respond("POST", "/mcp/a/connect", 200, json!(true));
    assert_eq!(svc.send(post("/api/mcp/a/connect")).await.json(), json!({ "ok": true }));
}
