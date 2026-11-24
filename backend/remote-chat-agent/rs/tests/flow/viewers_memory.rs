use crate::harness::{get, service, Env, Options};
use axum::http::StatusCode;
use remote_chat_agent_core::application::ports::WorkspaceFiles;
use remote_chat_agent_core::testing::MemoryFiles;
use std::path::Path;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_fork_held_in_memory_when_its_files_are_previewed_then_the_routes_answer_from_the_port_and_touch_no_disk() {
    // Given the service over a `MemoryFiles` fork: a markdown file, a source file and a file outside the fork, none of them on the disk
    let files = Arc::new(MemoryFiles::new());
    for dir in ["/memfork/docs", "/elsewhere"] {
        files.create_dir(Path::new(dir)).unwrap();
    }
    files.write(Path::new("/memfork/docs/notes.md"), b"# In memory\n\nSome *text*.\n").unwrap();
    files.write(Path::new("/memfork/docs/code.ts"), b"const x = a < b;\n").unwrap();
    files.write(Path::new("/elsewhere/secret.md"), b"# secret\n").unwrap();
    let env = Env::new(&[]);
    env.set("FORK_DIR", "/memfork");
    let svc = service(&env, Options { files: Some(files), ..Options::default() });

    // When the markdown file is asked, then it renders from the port
    let page = svc.send(get("/api/file/docs/notes.md")).await;
    assert_eq!(page.status, StatusCode::OK);
    let html = page.text();
    assert!(html.contains("<h1>In memory</h1>") && html.contains("<em>text</em>"), "{html}");

    // And a source file is shown escaped, a missing path is a 404 naming it and a path that climbs out of the fork is a 403
    let code = svc.send(get("/api/file/docs/code.ts")).await;
    assert!(code.text().contains("<pre><code class=\"language-ts\">const x = a &lt; b;\n</code></pre>"));
    let nothing = svc.send(get("/api/file/docs/nothing.md")).await;
    assert_eq!((nothing.status, nothing.text()), (StatusCode::NOT_FOUND, "File not found: docs/nothing.md".to_string()));
    let climb = svc.send(get("/api/file/..%2f..%2felsewhere%2fsecret.md")).await;
    assert_eq!((climb.status, climb.text()), (StatusCode::FORBIDDEN, "Forbidden".to_string()));

    // And nothing was written to the real disk
    assert!(!Path::new("/memfork").exists());
}
