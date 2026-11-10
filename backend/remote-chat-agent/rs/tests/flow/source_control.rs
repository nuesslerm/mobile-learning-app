use crate::doubles::Run;
use crate::harness::{get, post, service, Env, Options};
use axum::http::StatusCode;
use serde_json::json;
use std::path::PathBuf;

const UNAUTHORIZED: &str = r#"{"error":"Unauthorized","code":"UNAUTHORIZED"}"#;
const FIELDS: &str = "number,title,url,state,headRefName,baseRefName,isDraft";

fn q(cwd: &str) -> String {
    format!("cwd={}", url::form_urlencoded::byte_serialize(cwd.as_bytes()).collect::<String>())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_the_retired_vcs_routes_when_called_with_the_secret_then_each_is_not_found_and_git_is_never_run() {
    // Given the service and the four routes git status, diff, commit and push had (now the `git` group of the agent API)
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let work = "/repo/work";
    // When each is called with the secret
    let answers = [
        svc.send(get(&format!("/api/vcs/status?{}", q(work))).secret()).await,
        svc.send(get(&format!("/api/vcs/diff?{}", q(work))).secret()).await,
        svc.send(post("/api/vcs/commit").secret().json(json!({ "cwd": work, "message": "m" }))).await,
        svc.send(post("/api/vcs/push").secret().json(json!({ "cwd": work }))).await,
    ];
    // Then each is not found and no command ran
    assert!(answers.iter().all(|a| a.status == StatusCode::NOT_FOUND), "{:?}", answers.iter().map(|a| a.status).collect::<Vec<_>>());
    assert!(svc.commands.runs().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_checkout_when_pull_requests_are_listed_viewed_and_created_then_gh_is_run_there_and_only_the_summary_comes_back() {
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let gh = &svc.commands;
    let cwd = "/repo";
    let calls = || -> Vec<(Option<PathBuf>, Vec<String>)> { gh.of("gh").into_iter().map(|r| (r.cwd, r.args)).collect() };
    let args = |list: &[&str]| -> Vec<String> { list.iter().map(|a| (*a).to_string()).collect() };
    let pr = json!({ "number": 41, "title": "Add a thing", "url": "https://github.com/o/r/pull/41", "state": "OPEN", "headRefName": "feat", "baseRefName": "main", "isDraft": true });
    let replies = |out: String| gh.answer(move |r: &Run| r.ok(&out));

    // Given no secret or a wrong one, or a request missing what it needs, then 401 or 400 and gh is never run.
    let refused = [
        svc.send(get(&format!("/api/source-control/pull-requests?{}", q(cwd)))).await,
        svc.send(get(&format!("/api/source-control/pull-requests/41?{}", q(cwd))).header("x-bot-secret", "wrong")).await,
        svc.send(post("/api/source-control/pull-requests").json(json!({}))).await,
    ];
    for answer in refused {
        assert_eq!((answer.status, answer.text()), (StatusCode::UNAUTHORIZED, UNAUTHORIZED.to_string()));
    }
    let no_cwd = svc.send(get("/api/source-control/pull-requests").secret()).await;
    assert_eq!((no_cwd.status, no_cwd.json()), (StatusCode::BAD_REQUEST, json!({ "error": "Missing required parameter: cwd", "code": "INVALID_REQUEST" })));
    for path in [format!("/api/source-control/pull-requests/abc?{}", q(cwd)), "/api/source-control/pull-requests/41".to_string()] {
        let answer = svc.send(get(&path).secret()).await;
        assert_eq!((answer.status, answer.json()), (StatusCode::BAD_REQUEST, json!({ "error": "Missing/invalid parameters: cwd, number", "code": "INVALID_REQUEST" })));
    }
    for body in [json!({ "head": "feat", "base": "main", "title": "t" }), json!({ "cwd": "/x", "base": "main", "title": "t" }), json!({ "cwd": "/x", "head": "feat", "title": "t" }), json!({ "cwd": "/x", "head": "feat", "base": "main" })] {
        let answer = svc.send(post("/api/source-control/pull-requests").secret().json(body)).await;
        assert_eq!((answer.status, answer.json()), (StatusCode::BAD_REQUEST, json!({ "error": "Missing required fields: cwd, head, base, title", "code": "INVALID_REQUEST" })));
    }
    assert!(calls().is_empty());

    // When pull requests are listed, then gh runs in the cwd and only the summary fields come back, a missing state as OPEN; a state
    // is passed on as given except all, which is dropped.
    replies(json!([{ "number": 41, "title": "Add a thing", "url": "https://github.com/o/r/pull/41", "state": "OPEN", "headRefName": "feat", "baseRefName": "main", "isDraft": true, "author": { "login": "someone" }, "labels": [] },
        { "number": 42, "title": "No state", "url": "https://github.com/o/r/pull/42", "headRefName": "b", "baseRefName": "main", "isDraft": false }]).to_string());
    let listed = svc.send(get(&format!("/api/source-control/pull-requests?{}", q(cwd))).secret()).await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json(), json!([pr, { "number": 42, "title": "No state", "url": "https://github.com/o/r/pull/42", "state": "OPEN", "headRefName": "b", "baseRefName": "main", "isDraft": false }]));
    for state in ["merged", "all", "bogus"] {
        svc.send(get(&format!("/api/source-control/pull-requests?{}&state={state}", q(cwd))).secret()).await;
    }
    assert_eq!(
        calls(),
        [
            (Some(PathBuf::from(cwd)), args(&["pr", "list", "--json", FIELDS])),
            (Some(PathBuf::from(cwd)), args(&["pr", "list", "--json", FIELDS, "--state", "merged"])),
            (Some(PathBuf::from(cwd)), args(&["pr", "list", "--json", FIELDS])),
            (Some(PathBuf::from(cwd)), args(&["pr", "list", "--json", FIELDS, "--state", "bogus"])),
        ]
    );

    // When gh fails or prints something that is not a list, then 502 SOURCE_CONTROL_FAILED with gh's stderr or the reason.
    gh.answer(|r: &Run| r.fails("", "gh: To get started with GitHub CLI, please run: gh auth login\n"));
    let failed = svc.send(get(&format!("/api/source-control/pull-requests?{}", q(cwd))).secret()).await;
    assert_eq!((failed.status, failed.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "gh: To get started with GitHub CLI, please run: gh auth login\n", "code": "SOURCE_CONTROL_FAILED" })));
    for out in ["not json at all", "{}"] {
        replies(out.to_string());
        let odd = svc.send(get(&format!("/api/source-control/pull-requests?{}", q(cwd))).secret()).await;
        assert_eq!((odd.status, odd.json()["code"].clone()), (StatusCode::BAD_GATEWAY, json!("SOURCE_CONTROL_FAILED")));
    }

    // When one pull request is asked by its number (as JavaScript reads one), then gh views it in the cwd; its failure is a 502.
    gh.forget();
    replies(pr.to_string());
    assert_eq!(svc.send(get(&format!("/api/source-control/pull-requests/41?{}", q(cwd))).secret()).await.json(), pr);
    svc.send(get(&format!("/api/source-control/pull-requests/0x10?{}", q(cwd))).secret()).await;
    assert_eq!(calls().iter().map(|(_, a)| a.clone()).collect::<Vec<_>>(), [args(&["pr", "view", "41", "--json", FIELDS]), args(&["pr", "view", "16", "--json", FIELDS])]);
    gh.answer(|r: &Run| r.fails("", "no pull requests found for branch\n"));
    let missing = svc.send(get(&format!("/api/source-control/pull-requests/41?{}", q(cwd))).secret()).await;
    assert_eq!((missing.status, missing.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "no pull requests found for branch\n", "code": "SOURCE_CONTROL_FAILED" })));

    // When a pull request is created, then gh gets the head, base, title and body (empty when none, --draft last when asked), and the
    // answer is 201 with the number read from the last line gh printed, 0 when it has none; gh's refusal is a 502.
    gh.forget();
    replies("https://github.com/o/r/pull/77\n".into());
    let created = svc.send(post("/api/source-control/pull-requests").secret().json(json!({ "cwd": cwd, "head": "feat", "base": "main", "title": "Add a thing", "body": "Because." }))).await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.json(), json!({ "number": 77, "title": "Add a thing", "url": "https://github.com/o/r/pull/77", "state": "OPEN", "headRefName": "feat", "baseRefName": "main", "isDraft": false }));
    replies("Warning: 1 uncommitted change\nhttps://github.com/o/r/pull/78\n".into());
    let draft = svc.send(post("/api/source-control/pull-requests").secret().json(json!({ "cwd": cwd, "head": "feat", "base": "main", "title": "Draft one", "draft": true }))).await.json();
    assert_eq!((draft["number"].clone(), draft["url"].clone(), draft["isDraft"].clone()), (json!(78), json!("https://github.com/o/r/pull/78"), json!(true)));
    assert_eq!(
        calls().iter().map(|(_, a)| a.clone()).collect::<Vec<_>>(),
        [
            args(&["pr", "create", "--head", "feat", "--base", "main", "--title", "Add a thing", "--body", "Because."]),
            args(&["pr", "create", "--head", "feat", "--base", "main", "--title", "Draft one", "--body", "", "--draft"]),
        ]
    );
    replies("created somewhere\n".into());
    let numberless = svc.send(post("/api/source-control/pull-requests").secret().json(json!({ "cwd": cwd, "head": "feat", "base": "main", "title": "t" }))).await;
    assert_eq!((numberless.status, numberless.json()["number"].clone(), numberless.json()["url"].clone()), (StatusCode::CREATED, json!(0), json!("created somewhere")));
    gh.answer(|r: &Run| r.fails("", "a pull request for branch \"feat\" already exists\n"));
    let exists = svc.send(post("/api/source-control/pull-requests").secret().json(json!({ "cwd": cwd, "head": "feat", "base": "main", "title": "t" }))).await;
    assert_eq!((exists.status, exists.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "a pull request for branch \"feat\" already exists\n", "code": "SOURCE_CONTROL_FAILED" })));
}
