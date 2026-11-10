use crate::doubles::Posted;
use crate::harness::{get, post, service, Env, Options};
use axum::http::StatusCode;
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_files_and_plans_in_the_fork_when_they_are_previewed_and_annotated_then_they_render_inside_the_fork_only_and_annotations_reach_telegram() {
    let env = Env::new(&[]);
    let fork = env.dir("fork");
    std::fs::create_dir_all(fork.join("docs")).unwrap();
    std::fs::create_dir_all(fork.join(".bot-state/plans")).unwrap();
    let markdown = "# Heading One\n\nSome *emphasis* here and **bold** and ~~gone~~.\n\n```ts\nconst a = 1 < 2 && *x*;\n```\n\nuse `a<b` here\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n- one\n- two\n\n[site](https://x.example)\n\n> quoted\n\n<script>alert(1)</script>\n\ninline <img src=x onerror=y> here\n\n[bad](javascript:alert(1)) and [ok](/docs/a.md)\n";
    std::fs::write(fork.join("docs/notes.md"), markdown).unwrap();
    std::fs::write(fork.join("docs/code.ts"), "const x = a < b && c > d;\n").unwrap();
    std::fs::write(fork.join(".bot-state/plans/p1.md"), "# plan one\n").unwrap();
    std::fs::write(fork.join(".bot-state/plans/my plan.md"), "# plan two\n").unwrap();
    std::fs::write(env.home.join("outside.md"), "# outside\n").unwrap();
    std::os::unix::fs::symlink(env.home.join("outside.md"), fork.join("docs/link.md")).unwrap();
    env.set("FORK_DIR", &fork.to_string_lossy());
    let svc = service(&env, Options::default());

    // When a markdown file is asked, then an html page renders it: headings, emphasis, escaped code, a table, a list, links that open
    // apart, a quote, and raw HTML shown as text with a script link going nowhere.
    let page = svc.send(get("/api/file/docs/notes.md?chatId=7")).await;
    assert_eq!((page.status, page.header("content-type")), (StatusCode::OK, Some("text/html; charset=utf-8")));
    let html = page.text();
    for rendered in [
        "<h1>Heading One</h1>",
        "<p>Some <em>emphasis</em> here and <strong>bold</strong> and <del>gone</del>.</p>",
        "<pre><code class=\"language-ts\">const a = 1 &lt; 2 &amp;&amp; *x*;\n</code></pre>",
        "<code>a&lt;b</code>",
        "<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody>\n<tr><td>1</td><td>2</td></tr>\n</tbody></table>",
        "<li>one</li>\n<li>two</li>",
        "<a href=\"https://x.example\" target=\"_blank\" rel=\"noopener\">site</a>",
        "<blockquote>\n<p>quoted</p>\n</blockquote>",
        "&lt;script&gt;",
        "<a href=\"#\" target=\"_blank\" rel=\"noopener\">bad</a>",
        "<a href=\"/docs/a.md\"",
        "📤 Send to Telegram",
    ] {
        assert!(html.contains(rendered), "{rendered} in {html}");
    }
    assert!(!html.contains("<script>alert") && !html.contains("<img"));
    assert!(svc.send(get("/api/file/docs/notes.md")).await.text().contains("(no chat context)"));

    // When a source file is asked, then it is shown escaped in a code block tagged with its extension; a missing path is a 404 naming
    // it, a path out of the fork (climbing or through a symlink) a 403, and a directory a 500.
    let code = svc.send(get("/api/file/docs/code.ts")).await;
    assert!(code.text().contains("<pre><code class=\"language-ts\">const x = a &lt; b &amp;&amp; c &gt; d;\n</code></pre>"));
    let nothing = svc.send(get("/api/file/docs/nothing.md")).await;
    assert_eq!((nothing.status, nothing.text()), (StatusCode::NOT_FOUND, "File not found: docs/nothing.md".to_string()));
    for climb in ["/api/file/..%2f..%2f..%2fetc%2fpasswd", "/api/file/docs/link.md"] {
        let answer = svc.send(get(climb)).await;
        assert_eq!((answer.status, answer.text()), (StatusCode::FORBIDDEN, "Forbidden".to_string()), "{climb}");
    }
    let directory = svc.send(get("/api/file/docs")).await;
    assert_eq!((directory.status, directory.text()), (StatusCode::INTERNAL_SERVER_ERROR, "Could not read file".to_string()));

    // When a plan is asked, then it redirects to its file preview keeping the chat and thread (empty when absent, the path encoded);
    // a plan that does not exist is a 404 naming it and an id that climbs a 403.
    let redirect = svc.send(get("/api/plan/p1?chatId=7&threadId=9")).await;
    assert_eq!((redirect.status, redirect.header("location")), (StatusCode::FOUND, Some("/api/file/.bot-state/plans/p1.md?chatId=7&threadId=9")));
    assert_eq!(svc.send(get("/api/plan/p1")).await.header("location"), Some("/api/file/.bot-state/plans/p1.md?chatId=&threadId="));
    assert_eq!(svc.send(get("/api/plan/my%20plan")).await.header("location"), Some("/api/file/.bot-state/plans/my%20plan.md?chatId=&threadId="));
    let absent = svc.send(get("/api/plan/nope")).await;
    assert_eq!((absent.status, absent.text()), (StatusCode::NOT_FOUND, "Plan nope not found".to_string()));
    let climbing = svc.send(get("/api/plan/..%2F..%2F..%2Fescape")).await;
    assert_eq!((climbing.status, climbing.text()), (StatusCode::FORBIDDEN, "Forbidden".to_string()));

    // When annotations are posted to a file or a plan, then a post that is not an annotation is a 404, none a 400, no Telegram token
    // a 500, and with one each reaches the chat and thread as one HTML message.
    let not_annotate = svc.send(post("/api/file/docs/notes.md").json(json!({}))).await;
    assert_eq!((not_annotate.status, not_annotate.json()), (StatusCode::NOT_FOUND, json!({ "error": "Not found" })));
    let file_note = json!({ "annotations": [{ "selection": "emphasis <b>", "comment": "why?" }], "chatId": 1, "threadId": 5 });
    let plan_note = json!({ "annotations": [{ "selection": "x", "comment": "y" }], "chatId": 1 });
    for (path, empty_error, untold_error) in [
        ("/api/file/docs/notes.md/annotate", "No annotations", "TELEGRAM_BOT_TOKEN not configured — cannot forward to Telegram"),
        ("/api/plan/p1/annotate", "No annotations", "TELEGRAM_BOT_TOKEN not configured"),
    ] {
        let none = svc.send(post(path).json(json!({ "annotations": [], "chatId": 1 }))).await;
        assert_eq!((none.status, none.json()), (StatusCode::BAD_REQUEST, json!({ "error": empty_error })));
        let untold = svc.send(post(path).json(file_note.clone())).await;
        assert_eq!((untold.status, untold.json()), (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": untold_error })));
    }
    env.set("TELEGRAM_BOT_TOKEN", "bot-token");
    svc.outbound.answer(|_: &Posted| Ok((200, json!({ "ok": true }))));
    assert_eq!(svc.send(post("/api/file/docs/notes.md/annotate").json(file_note)).await.json(), json!({ "ok": true }));
    assert_eq!(svc.send(post("/api/plan/p1/annotate").json(plan_note)).await.json(), json!({ "ok": true }));
    let messages: Vec<Value> = svc.outbound.posts().into_iter().map(|p| p.json.unwrap()).collect();
    assert_eq!(
        messages,
        [
            json!({ "chat_id": "1", "text": "📝 <b>Annotations on docs/notes.md</b>\n\n1. <blockquote>emphasis &lt;b&gt;</blockquote>\n💬 why?", "parse_mode": "HTML", "message_thread_id": "5" }),
            json!({ "chat_id": "1", "text": "📝 <b>Plan annotations (plan p1)</b>\n\n1. <blockquote>x</blockquote>\n💬 y", "parse_mode": "HTML" }),
        ]
    );
}

