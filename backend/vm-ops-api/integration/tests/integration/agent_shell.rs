//! The agent-shell writes of plan section 3 ("Writes"): exec-script start and cancel, and the credential writes. Each file
//! lands by the Engine's archive PUT, whose tar the fake Engine reads into entries (name, mode, owner, bytes) and which is
//! also compared byte for byte with the tar Node writes, but for its mtime and checksum.
use crate::{
    engine_calls, exec_creates, expect_json, expect_node_tars, expect_not_running, post, sent, with_engine, AGENT_SHELL,
    EXITED_AGENT_SHELL, JOB, NO_AGENT_SHELL,
};
use base64::Engine as _;
use regex::Regex;
use serde_json::{json, Value};
use std::time::Duration;
use vm_ops_api_integration::{assert_golden, auth, case, now_ms, Archive, Exec, Reply, Service, TarEntry};

const EXEC_SCRIPT: &str = "/services/agent-shell/exec-script";
const CREDENTIALS: &str = "/agent-shell/credentials";
/// The clean-up every start runs first: job dirs older than a day.
const FIND: &str = "find /tmp/exec-jobs -mindepth 1 -maxdepth 1 -mtime +1 -exec rm -rf {} + 2>/dev/null || true";
const UUID_V4: &str = r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";
const KNOWN_TOOLS: [&str; 5] = ["codex", "claude", "gcloud", "gemini", "opencode"];
/// Each tool's dir and file under agent-shell's `/workspace/home`, as the credentials route writes them.
const TOOLS: [(&str, &str, &str); 5] = [
    ("codex", "/workspace/home/.codex", "auth.json"),
    ("claude", "/workspace/home/.claude", ".credentials.json"),
    ("gcloud", "/workspace/home/.config/gcloud", "credentials.db"),
    ("gemini", "/workspace/home/.gemini", "oauth_creds.json"),
    ("opencode", "/workspace/home/.opencode/data", "auth.json"),
];

fn cancel_path(job: &str) -> String {
    format!("{EXEC_SCRIPT}/{job}/cancel")
}

/// A POST with an empty body, signed as wf signs it.
fn post_empty(svc: &Service, target: &str) -> Reply {
    sent(svc, "POST", target, &auth("POST", target, &now_ms().to_string(), b""))
}

/// A dir as `encodeURIComponent` writes it into the archive PUT's query: these dirs need only their slashes escaped.
fn query_dir(dir: &str) -> String {
    dir.replace('/', "%2F")
}

/// The credentials body the vnc-auth relay posts: the tool and its file's bytes in standard base64.
fn credential(tool: &str, bytes: &[u8]) -> String {
    json!({ "tool": tool, "content": base64::engine::general_purpose::STANDARD.encode(bytes) }).to_string()
}

/// agent-shell's execs for the credential writes, each exiting 0.
fn credential_execs(svc: &Service) {
    svc.world()
        .fake_engine()
        .exec(Exec::prefix(&["mkdir", "-p", "-m", "700"], "^/workspace/home/").answer("", 0))
        .exec(Exec::prefix(&["chown", "1001:1001"], "^/workspace/home/").answer("", 0));
}

/// The body headers Node's `docker.ts` sends with each recorded request: none for a GET, else its method's content type
/// (JSON for exec create and start, a tar for the archive PUT) and the body's exact length, never chunked.
fn node_framing(svc: &Service) -> Vec<Value> {
    let requests = svc.world().engine_requests();
    let framed = |i: usize, kind: &str| {
        json!({ "content-type": kind, "content-length": svc.world().fake_engine().request_body(i).len().to_string() })
    };
    let each = requests.iter().enumerate().map(|(i, r)| match r["method"].as_str() {
        Some("GET") => json!({}),
        Some("PUT") => framed(i, "application/x-tar"),
        _ => framed(i, "application/json"),
    });
    each.collect()
}

case!(exec_script_start_lands_the_script_by_tar_and_starts_it_detached, ["POST /services/agent-shell/exec-script"], {
    // Given agent-shell, where the clean-up, the job dir's mkdir and the detached start each exit 0
    let w = with_engine(AGENT_SHELL);
    w.fake_engine()
        .exec(Exec::exact(&["sh", "-c", FIND]).answer("", 0))
        .exec(Exec::prefix(&["mkdir", "-p", "-m", "700"], r"^/tmp/exec-jobs/[0-9a-f-]{36}$").answer("", 0))
        .exec(Exec::prefix(&["setsid", "sh", "-c"], r" sh /tmp/exec-jobs/[0-9a-f-]{36} 1800$").answer("", 0));
    let svc = w.start();
    // When a script with non-ASCII text and a byte that is no UTF-8 is posted
    let script = b"#!/bin/sh\necho \"h\xc3\xa9llo \xe2\x82\xac\"\necho \xff\nexit 3\n";
    let r = post(&svc, EXEC_SCRIPT, script);
    // Then it is 202 with a fresh v4 UUID as the job id
    let job = r.json()["jobId"].as_str().unwrap_or_default().to_string();
    assert!(Regex::new(UUID_V4).unwrap().is_match(&job), "jobId {job:?} is no v4 UUID: {}", r.body);
    expect_json(&r, 202, &json!({ "jobId": job }));
    // And the Engine saw, in order: the list, the clean-up, the job dir's mkdir, the archive PUT into that dir and the
    // detached start, which is never inspected
    let dir = format!("/tmp/exec-jobs/{job}");
    let put = format!("PUT /containers/as1/archive?path={}", query_dir(&dir));
    let expected = [
        "GET /containers/json?all=true",
        "POST /containers/as1/exec",
        "POST /exec/exec-1/start",
        "GET /exec/exec-1/json",
        "POST /containers/as1/exec",
        "POST /exec/exec-2/start",
        "GET /exec/exec-2/json",
        put.as_str(),
        "POST /containers/as1/exec",
        "POST /exec/exec-3/start",
    ];
    assert_eq!(engine_calls(&svc), expected);
    let creates = exec_creates(&svc);
    let attached = |cmd: Value| json!({ "AttachStdout": true, "AttachStderr": true, "Cmd": cmd });
    assert_eq!(creates[..2], [attached(json!(["sh", "-c", FIND])), attached(json!(["mkdir", "-p", "-m", "700", dir]))]);
    let detached = (&creates[2]["AttachStdout"], &creates[2]["AttachStderr"], creates[2].get("Env"));
    assert_eq!(detached, (&json!(false), &json!(false), None), "{}", creates[2]);
    let start: Value = serde_json::from_slice(&svc.world().fake_engine().request_body(9)).expect("JSON");
    assert_eq!(start, json!({ "Detach": true }));
    // And every request is framed as Node frames it: each JSON body and the tar with its content type and exact length
    let framing: Vec<Value> = svc.world().engine_requests().iter().map(|r| r["headers"].clone()).collect();
    assert_eq!(framing, node_framing(&svc));
    // And the archive holds one file, script.sh, mode 0700, owned by root, with the script as the service decoded it:
    // the stray byte became U+FFFD
    let landed = b"#!/bin/sh\necho \"h\xc3\xa9llo \xe2\x82\xac\"\necho \xef\xbf\xbd\nexit 3\n";
    let file = TarEntry { name: "script.sh".into(), mode: 0o700, uid: 0, gid: 0, bytes: landed.to_vec() };
    let archive = Archive { container: "as1".into(), path: dir, entries: vec![file.clone()] };
    assert_eq!(svc.world().fake_engine().archives(), [archive]);
    // And that archive is the tar Node writes for it
    expect_node_tars(&svc, &[file]);
    // And the detached start runs the job wrapper Node runs, with the job's dir and the 1800 s backstop
    let cmd = serde_json::to_string_pretty(&creates[2]["Cmd"]).expect("JSON").replace(&job, "<jobId>");
    assert_golden("exec-script-job-cmd.json", &(cmd + "\n"));
});

case!(exec_script_start_looks_for_agent_shell_before_it_reads_the_script, ["POST /services/agent-shell/exec-script"], {
    // Given an Engine with no agent-shell
    let svc = with_engine(NO_AGENT_SHELL).start();
    // When an empty script is posted, Then it is 503: the container is looked for first
    expect_not_running(&post(&svc, EXEC_SCRIPT, b""));
    // Given agent-shell
    let svc = with_engine(AGENT_SHELL).start();
    // When an empty script is posted, Then it is 400 and nothing runs in agent-shell
    let r = post(&svc, EXEC_SCRIPT, b"");
    expect_json(&r, 400, &json!({ "error": "request body (the script to run) must not be empty" }));
    assert_eq!(engine_calls(&svc), ["GET /containers/json?all=true"]);
});

case!(exec_script_start_answers_a_failing_mkdir_or_archive_with_502, ["POST /services/agent-shell/exec-script"], {
    // Given agent-shell, where the clean-up exits 1 (which nothing reads), the first mkdir exits 1 and the next exits 0,
    // and the archive PUT answers 500 with a body whose 500th UTF-16 unit is the first half of a surrogate pair
    let w = with_engine(AGENT_SHELL);
    let no_space = "mkdir: cannot create directory '/tmp/exec-jobs': No space left on device\n";
    w.fake_engine()
        .exec(Exec::exact(&["sh", "-c", FIND]).answer("", 1))
        .exec(Exec::prefix(&["mkdir", "-p", "-m", "700"], "^/tmp/exec-jobs/").answer(no_space, 1).answer("", 0));
    let long = format!("{}\u{20ac}\u{1f600}{}", "x".repeat(498), "y".repeat(100));
    w.fake_engine().fail("/containers/as1/archive", 500, &long);
    let svc = w.start();
    // When a script is posted, Then the failed mkdir is 502 with its exit code and output
    let r = post(&svc, EXEC_SCRIPT, b"echo hi\n");
    expect_json(&r, 502, &json!({ "error": "mkdir failed inside agent-shell", "exitCode": 1, "output": no_space }));
    // When it is posted again, Then the failed archive PUT is 502 with its status and the body's first 500 UTF-16 units,
    // the cut pair's half escaped as JSON.stringify prints a lone surrogate (raw text: serde_json refuses one)
    let r = post(&svc, EXEC_SCRIPT, b"echo hi\n");
    let cut = format!(r#"{{"error":"docker archive write failed","statusCode":500,"body":"{}{}\ud83d"}}"#, "x".repeat(498), '\u{20ac}');
    assert_eq!((r.status, r.body.as_str(), r.header("content-type")), (502, cut.as_str(), Some("application/json")));
    // And neither started a job: the execs were the clean-up and the mkdir, twice
    let cmds: Vec<String> = exec_creates(&svc).iter().map(|c| c["Cmd"][0].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(cmds, ["sh", "mkdir", "sh", "mkdir"]);
});

case!(exec_script_cancel_terms_the_job_group_and_is_ok_whatever_the_exec_exits, [
    "POST /services/agent-shell/exec-script/<jobId>/cancel"
], {
    // Given agent-shell, where the cancel script for the job exits 1
    let w = with_engine(AGENT_SHELL);
    w.fake_engine().exec(Exec::prefix(&["sh", "-c"], &format!(r" sh /tmp/exec-jobs/{JOB} 10$")).answer("", 1));
    let svc = w.start();
    // When the job is cancelled
    let r = post_empty(&svc, &cancel_path(JOB));
    // Then it is 200 ok, whatever the cancel exited
    expect_json(&r, 200, &json!({ "ok": true }));
    // And the cancel ran attached in agent-shell's app container: the script Node runs with the job's dir and the 10 s grace
    let expected = ["GET /containers/json?all=true", "POST /containers/as1/exec", "POST /exec/exec-1/start", "GET /exec/exec-1/json"];
    assert_eq!(engine_calls(&svc), expected);
    let creates = exec_creates(&svc);
    assert_eq!((&creates[0]["AttachStdout"], &creates[0]["AttachStderr"]), (&json!(true), &json!(true)));
    assert_golden("exec-script-cancel-cmd.json", &(serde_json::to_string_pretty(&creates[0]["Cmd"]).expect("JSON") + "\n"));
});

case!(exec_script_cancel_refuses_a_non_uuid_before_any_engine_call, ["POST /services/agent-shell/exec-script/<jobId>/cancel"], {
    // Given agent-shell
    let svc = with_engine(AGENT_SHELL).start();
    // When a job id that is no UUID is cancelled, Then it is 400 and the Engine is never asked
    expect_json(&post_empty(&svc, &cancel_path("not-a-uuid")), 400, &json!({ "error": "jobId must be a UUID" }));
    assert_eq!(engine_calls(&svc), Vec::<String>::new());
});

case!(credentials_write_each_tools_file_as_the_agent_user, ["POST /agent-shell/credentials"], {
    // Given agent-shell, where every mkdir and chown exits 0
    let svc = with_engine(AGENT_SHELL).start();
    credential_execs(&svc);
    let content = |tool: &str| match tool {
        "gcloud" => b"SQLite format 3\x00\xff\xfe\x01".to_vec(),
        other => format!(r#"{{"token":"{other}-token"}}"#).into_bytes(),
    };
    for (tool, dir, file) in TOOLS {
        // When the tool's credential file is posted, as the vnc-auth relay posts it
        let bytes = content(tool);
        let r = post(&svc, CREDENTIALS, credential(tool, &bytes).as_bytes());
        // Then it is 200 with the tool, the file's path and its byte count, and stdout has the written event
        expect_json(&r, 200, &json!({ "ok": true, "tool": tool, "path": format!("{dir}/{file}"), "bytes": bytes.len() }));
        let event = json!({ "level": "info", "message": "agent_shell_credentials.written", "tool": tool });
        svc.wait_line(&event.to_string(), Duration::from_secs(5));
    }
    // And each write listed agent-shell, made the dir (0700), gave it to uid 1001 and put the file into it by tar
    let mut expected = Vec::new();
    for (i, (_, dir, _)) in TOOLS.iter().enumerate() {
        let (mkdir, chown) = (2 * i + 1, 2 * i + 2);
        expected.extend([
            "GET /containers/json?all=true".to_string(),
            "POST /containers/as1/exec".into(),
            format!("POST /exec/exec-{mkdir}/start"),
            format!("GET /exec/exec-{mkdir}/json"),
            "POST /containers/as1/exec".into(),
            format!("POST /exec/exec-{chown}/start"),
            format!("GET /exec/exec-{chown}/json"),
            format!("PUT /containers/as1/archive?path={}", query_dir(dir)),
        ]);
    }
    assert_eq!(engine_calls(&svc), expected);
    let cmds: Vec<Value> = exec_creates(&svc).iter().map(|c| c["Cmd"].clone()).collect();
    let made: Vec<Value> =
        TOOLS.iter().flat_map(|(_, dir, _)| [json!(["mkdir", "-p", "-m", "700", dir]), json!(["chown", "1001:1001", dir])]).collect();
    assert_eq!(cmds, made);
    // And each archive holds the one file, mode 0600, owned by 1001:1001, with the decoded bytes
    let archives: Vec<Archive> = TOOLS
        .iter()
        .map(|(tool, dir, file)| {
            let entry = TarEntry { name: file.to_string(), mode: 0o600, uid: 1001, gid: 1001, bytes: content(tool) };
            Archive { container: "as1".into(), path: dir.to_string(), entries: vec![entry] }
        })
        .collect();
    assert_eq!(svc.world().fake_engine().archives(), archives);
    // And each is the tar Node writes for its file
    let files: Vec<TarEntry> = archives.into_iter().flat_map(|a| a.entries).collect();
    expect_node_tars(&svc, &files);
});

case!(credentials_are_written_whatever_the_chown_exits, ["POST /agent-shell/credentials"], {
    // Given agent-shell, where the mkdir exits 0 and the chown fails, whose exit code nothing reads
    let w = with_engine(AGENT_SHELL);
    w.fake_engine()
        .exec(Exec::prefix(&["mkdir", "-p", "-m", "700"], "^/workspace/home/").answer("", 0))
        .exec(Exec::prefix(&["chown", "1001:1001"], "^/workspace/home/").answer("chown: Operation not permitted\n", 1));
    let svc = w.start();
    // When claude's credentials are posted
    let r = post(&svc, CREDENTIALS, credential("claude", b"{}").as_bytes());
    // Then it is 200 as if the chown had passed, and stdout has the written event
    let path = "/workspace/home/.claude/.credentials.json";
    expect_json(&r, 200, &json!({ "ok": true, "tool": "claude", "path": path, "bytes": 2 }));
    let event = json!({ "level": "info", "message": "agent_shell_credentials.written", "tool": "claude" });
    svc.wait_line(&event.to_string(), Duration::from_secs(5));
    // And the file still went in by tar after the chown: mode 0600, owned by 1001:1001
    let calls = engine_calls(&svc);
    assert_eq!(calls.last().map(String::as_str), Some("PUT /containers/as1/archive?path=%2Fworkspace%2Fhome%2F.claude"), "{calls:?}");
    let file = TarEntry { name: ".credentials.json".into(), mode: 0o600, uid: 1001, gid: 1001, bytes: b"{}".to_vec() };
    let archive = Archive { container: "as1".into(), path: "/workspace/home/.claude".into(), entries: vec![file] };
    assert_eq!(svc.world().fake_engine().archives(), [archive]);
});

case!(credentials_decode_base64_as_leniently_as_node, ["POST /agent-shell/credentials"], {
    // Given agent-shell, where every mkdir and chown exits 0
    let svc = with_engine(AGENT_SHELL).start();
    credential_execs(&svc);
    // When claude's content is base64 with no padding, a space inside, a stray character, text after the padding, or in
    // the URL-safe alphabet, all of which Buffer.from(content, 'base64') decodes
    let contents: [(&str, &[u8]); 5] =
        [("aGk", b"hi"), ("aG k=", b"hi"), ("aGk!", b"hi"), ("aGk==extra", b"hi"), ("a-_8", b"\x6b\xef\xfc")];
    for (content, bytes) in contents {
        let r = post(&svc, CREDENTIALS, json!({ "tool": "claude", "content": content }).to_string().as_bytes());
        // Then each is 200 with the decoded length
        let path = "/workspace/home/.claude/.credentials.json";
        expect_json(&r, 200, &json!({ "ok": true, "tool": "claude", "path": path, "bytes": bytes.len() }));
    }
    // And each archive holds the decoded bytes
    let file = |bytes: &[u8]| TarEntry { name: ".credentials.json".into(), mode: 0o600, uid: 1001, gid: 1001, bytes: bytes.to_vec() };
    let written: Vec<TarEntry> = svc.world().fake_engine().archives().into_iter().flat_map(|a| a.entries).collect();
    assert_eq!(written, contents.map(|(_, bytes)| file(bytes)));
});

case!(credentials_validate_the_body_before_looking_for_agent_shell, ["POST /agent-shell/credentials"], {
    // Given an Engine with no agent-shell
    let svc = with_engine(NO_AGENT_SHELL).start();
    // When the body is no JSON, Then it is 400 body must be JSON
    expect_json(&post(&svc, CREDENTIALS, b"{oops"), 400, &json!({ "error": "body must be JSON" }));
    // When the tool is unknown, missing, no string, or the body is empty or no object, Then it is 400 with the known tools
    let unknown = json!({ "error": "unknown tool", "knownTools": KNOWN_TOOLS });
    for body in [r#"{"tool":"vim","content":"aGk="}"#, r#"{"content":"aGk="}"#, r#"{"tool":5,"content":"aGk="}"#, "", "[1]"] {
        expect_json(&post(&svc, CREDENTIALS, body.as_bytes()), 400, &unknown);
    }
    // When the content is missing, empty or no string, Then it is 400 content must be a non-empty base64 string
    let empty = json!({ "error": "content must be a non-empty base64 string" });
    for body in [r#"{"tool":"claude"}"#, r#"{"tool":"claude","content":""}"#, r#"{"tool":"claude","content":5}"#] {
        expect_json(&post(&svc, CREDENTIALS, body.as_bytes()), 400, &empty);
    }
    // When the content decodes to no bytes (no base64 character, or padding first), Then it is 400 content is not valid
    // base64
    let invalid = json!({ "error": "content is not valid base64" });
    for body in [r#"{"tool":"claude","content":"!!!"}"#, r#"{"tool":"claude","content":"=aGk"}"#] {
        expect_json(&post(&svc, CREDENTIALS, body.as_bytes()), 400, &invalid);
    }
    // And none of them asked the Engine
    assert_eq!(engine_calls(&svc), Vec::<String>::new());
    // When a valid body is posted, Then it is 503: agent-shell is looked for only after the body passes
    expect_not_running(&post(&svc, CREDENTIALS, credential("claude", b"{}").as_bytes()));
    assert_eq!(engine_calls(&svc), ["GET /containers/json?all=true"]);
});

case!(credentials_for_a_builtin_object_name_are_an_unknown_tool, ["POST /agent-shell/credentials"], {
    // Given agent-shell
    let svc = with_engine(AGENT_SHELL).start();
    // When the tool is a name every object has: toString, constructor, __proto__ or hasOwnProperty
    for tool in ["toString", "constructor", "__proto__", "hasOwnProperty"] {
        let r = post(&svc, CREDENTIALS, json!({ "tool": tool, "content": "aGk=" }).to_string().as_bytes());
        // Then it is 400 unknown tool with the known tools, like any other name
        expect_json(&r, 400, &json!({ "error": "unknown tool", "knownTools": KNOWN_TOOLS }));
    }
    // And agent-shell was never asked
    assert_eq!(engine_calls(&svc), Vec::<String>::new());
});

case!(agent_shell_writes_are_503_without_a_running_agent_shell, [
    "POST /services/agent-shell/exec-script",
    "POST /services/agent-shell/exec-script/<jobId>/cancel",
    "POST /agent-shell/credentials"
], {
    let writes = [
        (EXEC_SCRIPT.to_string(), b"echo hi\n".to_vec()),
        (cancel_path(JOB), Vec::new()),
        (CREDENTIALS.to_string(), credential("claude", b"{}").into_bytes()),
    ];
    // Given an Engine listing only agent-shell's tailscale sidecar and another app
    let svc = with_engine(NO_AGENT_SHELL).start();
    // When each agent-shell write is made, Then each is 503
    for (target, body) in &writes {
        expect_not_running(&post(&svc, target, body));
    }
    // Given an Engine listing only an exited agent-shell
    let svc = with_engine(EXITED_AGENT_SHELL).start();
    // When each write is made, Then each is 503 as well, the Engine is only listed, and no archive is put
    for (target, body) in &writes {
        expect_not_running(&post(&svc, target, body));
    }
    assert!(engine_calls(&svc).iter().all(|call| call.starts_with("GET /containers/json")));
    assert_eq!(svc.world().fake_engine().archives(), Vec::<Archive>::new());
});
