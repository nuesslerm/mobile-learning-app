//! The files rows through the `Host` dispatcher with an in-memory workspace set as rca: each reply is read the way the phone reads it, and the
//! agent methods the mapper sent are held to its `USES` list. Given/When/Then in each test.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{AgentApi, Area};
use orca_core::{Host, HostInfo, ServiceStatus};
use orca_map_workspace::testing::Workspaces;
use orca_map_workspace::{WorkspaceArea, NOT_OFFERED, USES};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

struct Rig {
    agent: Arc<ScriptedAgent>,
    files: Workspaces,
    conn: orca_core::connection::Connection,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        let files = Workspaces::default();
        files.workspace("main", "root", None).workspace("docs", "directory", Some("/srv/docs")).workspace("feat", "worktree", Some("/srv/wt/feat"));
        files.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(WorkspaceArea::default())], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { agent, files, conn: host.connection("dev", "tok", vec![]), n: 0 }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.n += 1;
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        let out = self.conn.on_message(&frame);
        serde_json::from_str(&out[0]).unwrap()
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert_eq!(r["ok"], true, "{method}: {r}");
        r["result"].clone()
    }

    fn err(&mut self, method: &str, params: Value) -> (String, String) {
        let r = self.call(method, params);
        assert_eq!(r["ok"], false, "{method} should be refused: {r}");
        (r["error"]["code"].as_str().unwrap().to_string(), r["error"]["message"].as_str().unwrap().to_string())
    }
}

fn payload(n: usize) -> Vec<u8> {
    (0..n).map(|i| b'a' + (i % 26) as u8).collect()
}

#[test]
fn given_a_text_file_when_read_then_its_content_and_size_are_the_files_and_a_longer_one_stops_at_the_hosts_limit() {
    // Given a small file and one a byte past the 512 KiB the host reads
    let mut rig = Rig::new();
    let small = payload(1000);
    let big = payload(512 * 1024 + 10);
    rig.files.put("main", "src/small.txt", &small).put("main", "big.txt", &big);
    // When both are read through the id the host's own listing names
    let a = rig.ok("files.read", json!({"worktree": "id:main::", "relativePath": "src/small.txt"}));
    let b = rig.ok("files.read", json!({"worktree": "id:main::", "relativePath": "big.txt"}));
    // Then the small one is whole and the big one is cut at the limit, saying so and with the length the host counts (the limit and one more byte)
    assert_eq!((a["content"].as_str().unwrap().as_bytes(), a["truncated"].clone(), a["byteLength"].clone()), (small.as_slice(), json!(false), json!(small.len())));
    assert_eq!((b["content"].as_str().unwrap().len(), b["truncated"].clone(), b["byteLength"].clone()), (512 * 1024, json!(true), json!(512 * 1024 + 1)));
    assert_eq!((a["worktree"].clone(), a["relativePath"].clone()), (json!("main::"), json!("src/small.txt")));
}

#[test]
fn given_paths_the_host_refuses_when_read_then_each_is_refused_with_the_hosts_word_and_a_file_that_is_there_is_not() {
    // Given a binary file, a text file with a NUL in it, and a plain one
    let mut rig = Rig::new();
    rig.files.put("main", "a.png", &[1, 2, 3]).put("main", "nul.dat", &[b'x', 0, b'y']).put("main", "ok.txt", b"fine");
    // When they are read, with an unsafe path and an unknown worktree besides
    let png = rig.err("files.read", json!({"worktree": "main", "relativePath": "a.png"}));
    let nul = rig.err("files.read", json!({"worktree": "main", "relativePath": "nul.dat"}));
    let up = rig.err("files.read", json!({"worktree": "main", "relativePath": "../x"}));
    let abs = rig.err("files.read", json!({"worktree": "main", "relativePath": "/etc/passwd"}));
    let nowhere = rig.err("files.read", json!({"worktree": "id:nowhere::", "relativePath": "ok.txt"}));
    let missing = rig.err("files.read", json!({"worktree": "main", "relativePath": "gone.txt"}));
    let none = rig.err("files.read", json!({"relativePath": "ok.txt"}));
    // Then each carries the host's code and sentence
    assert_eq!(png, ("runtime_error".into(), "binary_file".into()));
    assert_eq!(nul, ("runtime_error".into(), "binary_file".into()));
    assert_eq!(up, ("runtime_error".into(), "invalid_relative_path".into()));
    assert_eq!(abs, ("runtime_error".into(), "invalid_relative_path".into()));
    assert_eq!(nowhere, ("selector_not_found".into(), "selector_not_found".into()));
    assert_eq!(missing.0, "runtime_error");
    assert!(missing.1.contains("gone.txt"), "{missing:?}");
    assert_eq!(none, ("invalid_argument".into(), "Missing worktree selector".into()));
    // And the same file is read by its id, its bare name and the path of its workspace
    for selector in ["main", "id:main::", "id:main"] {
        assert_eq!(rig.ok("files.read", json!({"worktree": selector, "relativePath": "ok.txt"}))["content"], "fine", "{selector}");
    }
    rig.files.put("docs", "ok.txt", b"docs one");
    for selector in ["path:/srv/docs", "/srv/docs", "id:docs::/srv/docs"] {
        assert_eq!(rig.ok("files.read", json!({"worktree": selector, "relativePath": "ok.txt"}))["content"], "docs one", "{selector}");
    }
}

#[test]
fn given_a_folder_when_read_as_a_directory_then_folders_lead_in_natural_order_and_pages_by_cursor_walk_it_once() {
    // Given a folder of files and folders whose names the natural order moves around
    let mut rig = Rig::new();
    for name in ["file10.txt", "file9.txt", "File2.txt", "zeta/inner.txt", "alpha/inner.txt", "beta/deep/x.txt"] {
        rig.files.put("main", &format!("dir/{name}"), b"x");
    }
    // When it is read whole
    let whole = rig.ok("files.readDir", json!({"worktree": "main", "relativePath": "dir"}));
    let names: Vec<(&str, bool)> = whole.as_array().unwrap().iter().map(|e| (e["name"].as_str().unwrap(), e["isDirectory"].as_bool().unwrap())).collect();
    // Then folders come first, files sort by number value, and the symlink flag the phone salvages is present
    assert_eq!(names, [("alpha", true), ("beta", true), ("zeta", true), ("File2.txt", false), ("file9.txt", false), ("file10.txt", false)]);
    assert!(whole.as_array().unwrap().iter().all(|e| e["isSymlink"] == false));
    // And walking it in pages of two with the cursor gives the same listing, saying when no more follow
    let (mut walked, mut after, mut totals) = (Vec::new(), Value::Null, Vec::new());
    loop {
        let mut params = json!({"worktree": "main", "relativePath": "dir", "limit": 2});
        if !after.is_null() {
            params["after"] = after.clone();
        }
        let page = rig.ok("files.readDir", params);
        totals.push(page["total"].clone());
        walked.extend(page["entries"].as_array().unwrap().clone());
        let more = page["hasMore"] == true;
        after = walked.last().map(|e| json!({"name": e["name"], "isDirectory": e["isDirectory"]})).unwrap_or(Value::Null);
        if !more {
            break;
        }
    }
    assert_eq!(Value::Array(walked), whole);
    assert!(totals.iter().all(|t| *t == json!(6)));
    // And a limit past the host's page size is refused
    assert_eq!(rig.err("files.readDir", json!({"worktree": "main", "relativePath": "dir", "limit": 5001})).0, "invalid_argument");
}

#[test]
fn given_a_file_when_read_in_chunks_then_the_windows_reassemble_it_and_the_last_says_eof() {
    // Given a file of a few windows
    let mut rig = Rig::new();
    let data = payload(1500);
    rig.files.put("main", "data.bin", &data);
    // When it is read in 512-byte windows the way the phone downloads media
    let (mut got, mut offset, mut flags) = (Vec::new(), 0u64, Vec::new());
    loop {
        let r = rig.ok("files.readChunk", json!({"worktree": "main", "relativePath": "data.bin", "offset": offset, "length": 512}));
        use base64::Engine;
        let piece = base64::engine::general_purpose::STANDARD.decode(r["contentBase64"].as_str().unwrap()).unwrap();
        assert_eq!(r["bytesRead"], piece.len());
        offset += piece.len() as u64;
        got.extend(piece);
        flags.push(r["eof"].as_bool().unwrap());
        if flags.last() == Some(&true) {
            break;
        }
    }
    // Then the bytes are the file's, and only the last window said eof
    assert_eq!(got, data);
    assert_eq!(flags, [false, false, true]);
    // And a window the host refuses is refused
    for bad in [json!({"offset": 0, "length": 0}), json!({"offset": 0, "length": 512 * 1024 + 1}), json!({"length": 5}), json!({"offset": -1, "length": 5})] {
        let mut p = json!({"worktree": "main", "relativePath": "data.bin"});
        p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert_eq!(rig.err("files.readChunk", p).0, "invalid_argument");
    }
}

#[test]
fn given_a_file_and_a_folder_when_stat_then_size_kind_and_time_are_the_executors_and_a_missing_path_is_refused() {
    // Given a file and the folder holding it
    let mut rig = Rig::new();
    rig.files.put("main", "d/f.txt", &payload(77));
    // When both are stat
    let file = rig.ok("files.stat", json!({"worktree": "main", "relativePath": "d/f.txt"}));
    let dir = rig.ok("files.stat", json!({"worktree": "main", "relativePath": "d"}));
    // Then the file has its size and the folder says it is one, each with a time
    assert_eq!((file["size"].clone(), file["isDirectory"].clone()), (json!(77), json!(false)));
    assert_eq!(dir["isDirectory"], true);
    assert!(file["mtime"].as_u64().unwrap() > 0);
    assert_eq!(rig.err("files.stat", json!({"worktree": "main", "relativePath": "nope"})).0, "runtime_error");
}

#[test]
fn given_a_batch_of_paths_when_checked_then_each_answers_whether_it_is_there_in_order_and_a_bad_batch_is_refused_whole() {
    // Given a file and a folder
    let mut rig = Rig::new();
    rig.files.put("main", "d/f.txt", &payload(7));
    // When a present file, a present folder and a missing path are checked together
    let found = rig.ok("files.pathsExist", json!({"worktree": "main", "relativePaths": ["d/f.txt", "d", "nope.txt"]}));
    // Then the answers are in the order asked: there, there, absent
    assert_eq!(found, json!([{"exists": true}, {"exists": true}, {"exists": false}]));
    // And an empty batch answers an empty list, a path outside the worktree and a batch of more than 128 are refused whole
    assert_eq!(rig.ok("files.pathsExist", json!({"worktree": "main", "relativePaths": []})), json!([]));
    assert_eq!(rig.err("files.pathsExist", json!({"worktree": "main", "relativePaths": ["../x"]})).0, "runtime_error");
    assert_eq!(rig.err("files.pathsExist", json!({"worktree": "main", "relativePaths": vec!["a"; 129]})).0, "runtime_error");
}

#[test]
fn given_a_new_name_when_a_file_is_created_then_it_is_there_empty_and_creating_it_again_is_refused() {
    // Given a workspace without the file
    let mut rig = Rig::new();
    assert!(rig.files.read("main", "notes.md").is_none());
    // When it is created, and created again
    let first = rig.ok("files.createFile", json!({"worktree": "main", "relativePath": "notes.md"}));
    let again = rig.err("files.createFile", json!({"worktree": "main", "relativePath": "notes.md"}));
    // Then it is an empty file the executor holds, the answer is the host's, and the second is refused with the executor's sentence
    assert_eq!(first, json!({"ok": true}));
    assert_eq!(rig.files.read("main", "notes.md"), Some(Vec::new()));
    assert_eq!(rig.ok("files.stat", json!({"worktree": "main", "relativePath": "notes.md"}))["size"], 0);
    assert_eq!(again.0, "runtime_error");
    assert!(again.1.contains("already exists"), "{again:?}");
}

#[test]
fn given_files_when_listed_and_searched_then_the_inventory_is_sorted_with_kinds_and_a_search_finds_files_only_within_its_limit() {
    // Given files of both kinds in nested folders
    let mut rig = Rig::new();
    for p in ["README.md", "src/main.rs", "src/lib.rs", "img/logo.png", "src/util/main_helper.rs"] {
        rig.files.put("main", p, b"x");
    }
    // When the workspace is listed and searched
    let listed = rig.ok("files.list", json!({"worktree": "main"}));
    let found = rig.ok("files.searchPaths", json!({"worktree": "main", "query": "main", "limit": 1}));
    let all = rig.ok("files.searchPaths", json!({"worktree": "main", "query": "main", "limit": 16, "mode": "quick-open", "excludePaths": ["src/main.rs"]}));
    let none = rig.ok("files.searchPaths", json!({"worktree": "main", "query": "  ", "mode": "quick-open"}));
    let first = rig.ok("files.searchPaths", json!({"worktree": "main", "query": "", "limit": 2}));
    // Then the list holds only files, in order, each with its basename and kind, and says nothing was cut
    let rows: Vec<(&str, &str, &str)> = listed["files"].as_array().unwrap().iter().map(|f| (f["relativePath"].as_str().unwrap(), f["basename"].as_str().unwrap(), f["kind"].as_str().unwrap())).collect();
    assert_eq!(rows, [("img/logo.png", "logo.png", "binary"), ("README.md", "README.md", "text"), ("src/lib.rs", "lib.rs", "text"), ("src/main.rs", "main.rs", "text"), ("src/util/main_helper.rs", "main_helper.rs", "text")]);
    assert_eq!((listed["totalCount"].clone(), listed["truncated"].clone(), listed["worktree"].clone()), (json!(5), json!(false), json!("main::")));
    // And a limited search keeps the best match and says more exist, an excluded path is left out, a blank quick-open query finds nothing
    assert_eq!(found["files"].as_array().unwrap().len(), 1);
    assert_eq!((found["truncated"].clone(), found["totalCount"].clone()), (json!(true), json!(2)));
    assert_eq!(all["files"].as_array().unwrap().iter().map(|f| f["relativePath"].as_str().unwrap()).collect::<Vec<_>>(), ["src/util/main_helper.rs"]);
    assert_eq!(none["files"], json!([]));
    // And the legacy mode with no query lists the first files, and a limit over 32 is refused
    assert_eq!(first["files"].as_array().unwrap().len(), 2);
    assert_eq!(rig.err("files.searchPaths", json!({"worktree": "main", "query": "m", "limit": 33})).0, "invalid_argument");
}

#[test]
fn given_an_image_a_text_file_and_a_binary_when_previewed_then_each_is_answered_as_the_host_does_and_a_file_too_large_is_refused() {
    // Given an image, a text file, a file with a NUL and one far past what a reply carries
    let mut rig = Rig::new();
    let png = (0..3000u32).map(|i| (i % 251) as u8).collect::<Vec<u8>>();
    rig.files.put("main", "p/pic.PNG", &png).put("main", "t.txt", "héllo".as_bytes()).put("main", "b.dat", &[0, 1, 2]).put("main", "huge.txt", &payload(512 * 1024 + 1)).put("main", "huge.png", &vec![7u8; 4 * 1024 * 1024]);
    // When each is previewed
    let image = rig.ok("files.readPreview", json!({"worktree": "main", "relativePath": "p/pic.PNG"}));
    let text = rig.ok("files.readPreview", json!({"worktree": "main", "relativePath": "t.txt"}));
    let bin = rig.ok("files.readPreview", json!({"worktree": "main", "relativePath": "b.dat"}));
    // Then the image is its bytes in base64 with its mime type, the text is its content, the binary says so
    use base64::Engine;
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(image["content"].as_str().unwrap()).unwrap(), png);
    assert_eq!((image["isBinary"].clone(), image["isImage"].clone(), image["mimeType"].clone()), (json!(true), json!(true), json!("image/png")));
    assert_eq!(text, json!({"content": "héllo", "isBinary": false}));
    assert_eq!(bin, json!({"content": "", "isBinary": true}));
    // And a text file past the read limit and an image past the reply's budget are refused with the host's word
    assert_eq!(rig.err("files.readPreview", json!({"worktree": "main", "relativePath": "huge.txt"})), ("runtime_error".into(), "file_too_large".into()));
    assert_eq!(rig.err("files.readPreview", json!({"worktree": "main", "relativePath": "huge.png"})), ("runtime_error".into(), "file_too_large".into()));
}

#[test]
fn given_projects_when_the_catalog_and_folder_workspaces_are_read_then_the_catalog_is_null_as_with_no_desktop_window_and_folders_are_the_stored_rows() {
    // Given workspaces of the three kinds and a stored name for one
    let mut rig = Rig::new();
    rig.agent.set_state("orca", "orca.repo.docs", json!({"displayName": "Docs site", "badgeColor": "#ff0000"}));
    // When the phone reads the catalog and the folder workspaces
    let sections = rig.ok("mobileRelay.projectSections", json!({}));
    let folders = rig.ok("folderWorkspace.list", json!({}));
    // Then the catalog is null (the pinned host answers so when no desktop window published one), and the repos are still listed by repo.list
    assert_eq!(sections, json!({"catalog": null}));
    let listed = rig.ok("repo.list", json!({}));
    let ids: Vec<&str> = listed["repos"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["docs", "main"]);
    // And no folder workspace is listed until one is stored, and then it is the stored row
    assert_eq!(folders, json!({"folderWorkspaces": []}));
    rig.agent.set_state("orca", "orca.folder-workspaces", json!([{"id": "f1", "name": "Scratch"}]));
    assert_eq!(rig.ok("folderWorkspace.list", json!({})), json!({"folderWorkspaces": [{"id": "f1", "name": "Scratch"}]}));
}

#[test]
fn given_every_row_of_the_area_when_called_then_the_agent_methods_sent_are_exactly_the_ones_listed() {
    // Given each served files row called once, so that every code path sends what it sends
    let mut rig = Rig::new();
    rig.files.put("main", "a.txt", b"x").put("main", "i.png", &[1]).artifact("/var/log/b.txt", b"log");
    let sel = json!({"worktree": "main"});
    let with = |extra: Value| {
        let mut p = sel.clone();
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    rig.ok("files.list", sel.clone());
    rig.ok("files.searchPaths", with(json!({"query": "a"})));
    rig.ok("files.read", with(json!({"relativePath": "a.txt"})));
    rig.ok("files.readPreview", with(json!({"relativePath": "i.png"})));
    rig.ok("files.readChunk", with(json!({"relativePath": "a.txt", "offset": 0, "length": 1})));
    rig.ok("files.readDir", with(json!({"relativePath": ""})));
    rig.ok("files.stat", with(json!({"relativePath": "a.txt"})));
    rig.ok("files.createFile", with(json!({"relativePath": "n.txt"})));
    let tap = rig.ok("files.resolveTerminalPath", with(json!({"pathText": "/var/log/b.txt"})));
    let granted = with(json!({"grantId": tap["openTarget"]["grantId"], "absolutePath": "/var/log/b.txt"}));
    rig.ok("files.readTerminalArtifact", granted.clone());
    rig.ok("files.readTerminalArtifactPreview", granted.clone());
    let mut save = granted.clone();
    save["content"] = json!("logged");
    rig.ok("files.writeTerminalArtifact", save);
    rig.files.put("main", "n.md", b"# n");
    rig.ok("files.open", with(json!({"relativePath": "n.md"})));
    rig.ok("files.openDiff", with(json!({"relativePath": "n.md"})));
    let version = rig.ok("markdown.readTab", json!({"worktree": "main", "tabId": "markdown:n.md"}))["version"].clone();
    rig.ok("markdown.saveTab", json!({"worktree": "main", "tabId": "markdown:n.md", "baseVersion": version, "content": "# m"}));
    rig.ok("ssh.listTargetSummaries", json!({}));
    rig.ok("mobileRelay.projectSections", json!({}));
    rig.ok("repo.list", json!({}));
    rig.ok("repo.update", json!({"repo": "main", "updates": {"displayName": "M"}}));
    rig.ok("repo.sparsePresets", json!({"repo": "main"}));
    rig.ok("repo.saveSparsePreset", json!({"repo": "main", "name": "p", "directories": ["a"]}));
    rig.ok("repo.hooks", json!({"repo": "main"}));
    rig.ok("repo.baseRefDefault", json!({"repo": "main"}));
    rig.ok("repo.searchRefs", json!({"repo": "main", "query": "m"}));
    let made = rig.ok("worktree.create", json!({"repo": "main", "name": "u1"}));
    let id = made["worktree"]["id"].as_str().unwrap().to_string();
    rig.ok("worktree.list", json!({}));
    rig.ok("worktree.show", json!({"worktree": id}));
    rig.ok("worktree.set", json!({"worktree": id, "comment": "c"}));
    rig.ok("worktree.prefetchCreateBase", json!({"repo": "main"}));
    rig.ok("worktree.rm", json!({"worktree": id}));
    rig.ok("worktree.listRetiredNames", json!({"repo": "main"}));
    rig.agent.reply("terminal.list", |p| Ok(json!({"workspace": p["workspace"], "terminals": []})));
    rig.agent.reply("pane.list", |_| Ok(json!({"panes": [pane("a", "main", "done", true, 1)]})));
    rig.agent.reply("pane.mark", |_| Ok(json!({"marked": 1})));
    rig.agent.reply("terminal.sleeping", |_| Ok(json!({"sleeping": [{"workspace": "main", "slept_at_ms": 1, "terminals": []}]})));
    rig.agent.reply("terminal.sleep", |p| Ok(json!({"workspace": p["workspace"], "slept": [], "already": false})));
    rig.agent.reply("terminal.wake", |p| Ok(json!({"workspace": p["workspace"], "woken": [], "failed": []})));
    rig.agent.reply("thread.list", |_| Ok(json!({"threads": [{"id": "r", "workspace": "main", "status": "running"}]})));
    rig.agent.reply("thread.forceStop", |_| Ok(json!({})));
    rig.agent.reply("notification.since", |_| Ok(json!({"epoch": "e", "notifications": []})));
    rig.ok("worktree.ps", json!({}));
    rig.ok("worktree.sleep", sel.clone());
    rig.ok("worktree.activate", sel.clone());
    rig.ok("agentActivity.setRead", json!({"paneKeys": ["thread:a"], "read": true}));
    rig.ok("notifications.getMissedSince", json!({"lastSeenSeq": 0}));
    // When the methods asked of the agent API are collected
    let sent: BTreeSet<String> = rig.agent.requests().into_iter().map(|(m, _)| m).collect();
    // Then they are the list the crate declares (state reads are the port's own call)
    let declared: BTreeSet<String> = USES.iter().map(|m| m.name().to_string()).collect();
    assert_eq!(sent, declared);
}

#[test]
fn given_stored_settings_when_the_project_views_are_read_then_each_is_the_same_repos_as_the_host_shows_them() {
    // Given a stored name and colour for the directory, a stored group, and a worktree that is no repo
    let mut rig = Rig::new();
    rig.agent.set_state("orca", "orca.repo.docs", json!({"displayName": "Docs site", "badgeColor": "#ff0000"}));
    rig.agent.set_state("orca", "orca.project-groups", json!([{"id": "g1", "name": "Work"}]));
    // When the phone reads the repos, projects, host setups and groups
    let repos = rig.ok("repo.list", json!({}));
    let projects = rig.ok("project.list", json!({}));
    let setups = rig.ok("projectHostSetup.list", json!({}));
    let groups = rig.ok("projectGroup.list", json!({}));
    // Then the repos are the root and the directory with the stored values over the defaults, and the other views name the same ids
    let ids = |v: &Value, key: &str| v[key].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(ids(&repos, "repos"), ["docs", "main"]);
    assert_eq!((repos["repos"][0]["displayName"].clone(), repos["repos"][0]["badgeColor"].clone(), repos["repos"][1]["displayName"].clone()), (json!("Docs site"), json!("#ff0000"), json!("main")));
    assert_eq!(ids(&projects, "projects"), ["docs", "main"]);
    assert_eq!(projects["projects"][0]["sourceRepoIds"], json!(["docs"]));
    assert_eq!((setups["setups"][0]["repoId"].clone(), setups["setups"][0]["path"].clone(), setups["setups"][0]["setupState"].clone()), (json!("docs"), json!("/srv/docs"), json!("ready")));
    assert_eq!(groups, json!({"groups": [{"id": "g1", "name": "Work"}]}));
    // And with nothing stored the groups are empty
    let mut bare = Rig::new();
    assert_eq!(bare.ok("projectGroup.list", json!({})), json!({"groups": []}));
}

#[test]
fn given_a_root_without_a_path_when_mobile_lists_repos_then_it_has_the_primary_checkout_path() {
    // Given RCA's root row and the Git checkouts its executor returns
    let mut rig = Rig::new();
    rig.agent.reply("worktree.list", |params| {
        assert_eq!(params, &json!({"workspace": "main"}));
        Ok(json!({"worktrees": [
            {"path": "/srv/feature", "branch": "feature", "is_main": false},
            {"path": "/srv/project", "branch": "main", "is_main": true},
        ]}))
    });
    // When the mobile workspace picker reads its project sources
    let repos = rig.ok("repo.list", json!({}));
    let setups = rig.ok("projectHostSetup.list", json!({}));
    // Then the root is selectable and refers to the primary checkout
    assert_eq!(repos["repos"][1], json!({
        "displayName": "main", "badgeColor": "#737373", "addedAt": 0,
        "kind": "git", "id": "main", "path": "/srv/project",
    }));
    assert_eq!(setups["setups"][1]["path"], "/srv/project");
}

#[test]
fn given_a_repo_update_and_sparse_presets_when_read_back_then_they_are_what_was_written_and_a_resave_by_id_replaces() {
    // Given a repo
    let mut rig = Rig::new();
    // When its name is updated, a preset saved, and the same preset saved again by its id
    let updated = rig.ok("repo.update", json!({"repo": "docs", "updates": {"displayName": "Manual", "id": "hijack", "path": "/x"}}));
    let first = rig.ok("repo.saveSparsePreset", json!({"repo": "docs", "name": "Web", "directories": ["apps/web"]}));
    let id = first["preset"]["id"].as_str().unwrap().to_string();
    rig.ok("repo.saveSparsePreset", json!({"repo": "docs", "id": id, "name": "Web 2", "directories": ["apps/web", "libs"]}));
    // Then the repo reads back with the name but with rca's own id and path, and one preset is stored with the new content
    assert_eq!((updated["repo"]["displayName"].clone(), updated["repo"]["id"].clone(), updated["repo"]["path"].clone()), (json!("Manual"), json!("docs"), json!("/srv/docs")));
    assert_eq!(rig.ok("repo.list", json!({}))["repos"][0]["displayName"], "Manual");
    let presets = rig.ok("repo.sparsePresets", json!({"repo": "docs"}));
    assert_eq!(presets["presets"].as_array().unwrap().len(), 1);
    assert_eq!((presets["presets"][0]["name"].clone(), presets["presets"][0]["directories"].clone(), presets["presets"][0]["repoId"].clone()), (json!("Web 2"), json!(["apps/web", "libs"]), json!("docs")));
    // And a repo nobody knows, and a worktree as a repo, are refused with the host's word, and a preset with no name with its message
    assert_eq!(rig.err("repo.update", json!({"repo": "nope", "updates": {}})).0, "selector_not_found");
    assert_eq!(rig.err("repo.sparsePresets", json!({"repo": "feat"})).0, "selector_not_found");
    assert_eq!(rig.err("repo.saveSparsePreset", json!({"repo": "main", "directories": []})).1, "Missing preset name");
}

#[test]
fn given_refs_and_a_hooks_file_when_asked_then_the_default_base_prefers_the_remote_main_and_search_and_hooks_read_what_is_there() {
    // Given refs with a remote main, a feature branch and a tag, and an orca.yaml with a setup script
    let mut rig = Rig::new();
    rig.files.refs("main", &[("main", "branch"), ("feature/x", "branch"), ("origin/main", "remote"), ("v1", "tag")]).put("main", "orca.yaml", b"scripts:\n  setup: npm ci\n");
    // When the default base, a search and the hooks are read
    let base = rig.ok("repo.baseRefDefault", json!({"repo": "main"}));
    let found = rig.ok("repo.searchRefs", json!({"repo": "main", "query": "x"}));
    let all = rig.ok("repo.searchRefs", json!({"repo": "main", "query": ""}));
    let hooks = rig.ok("repo.hooks", json!({"repo": "main"}));
    let none = rig.ok("repo.hooks", json!({"repo": "docs"}));
    // Then the default is the remote's main with one remote, search is a filter that leaves tags out, and the hooks are the file's scripts
    assert_eq!(base, json!({"defaultBaseRef": "origin/main", "remoteCount": 1}));
    assert_eq!(found["refs"], json!(["feature/x"]));
    assert_eq!(all["refs"], json!(["main", "feature/x", "origin/main"]));
    assert_eq!((hooks["hasHooksFile"].clone(), hooks["hooks"].clone(), hooks["source"].clone()), (json!(true), json!({"scripts": {"setup": "npm ci"}}), json!("orca.yaml")));
    assert_eq!((none["hasHooksFile"].clone(), none["hooks"].clone()), (json!(false), Value::Null));
    // And a repo with no remote falls back to its local main
    rig.files.refs("docs", &[("main", "branch")]);
    assert_eq!(rig.ok("repo.baseRefDefault", json!({"repo": "docs"})), json!({"defaultBaseRef": "main", "remoteCount": 0}));
}

#[test]
fn given_a_stat_that_fails_for_another_reason_when_hooks_are_read_then_the_failure_is_reported_not_read_as_no_hooks_file() {
    // Given a repo whose file.stat cannot reach the executor, and one that answers with another refusal
    let mut rig = Rig::new();
    rig.files.put("main", "orca.yaml", b"scripts:\n  setup: npm ci\n");
    rig.agent.reply("file.stat", |_| Err(orca_core::AgentError::Failed("executor gone".into())));
    // When the hooks are read
    let down = rig.err("repo.hooks", json!({"repo": "main"}));
    rig.agent.reply("file.stat", |_| Err(orca_core::AgentError::Refused { code: "PERMISSION_DENIED".into(), message: "no".into() }));
    let denied = rig.err("repo.hooks", json!({"repo": "main"}));
    // Then each is the refusal the mapper maps it to, never hasHooksFile false
    assert_eq!(down.0, "runtime_unavailable");
    assert_eq!(denied, ("runtime_error".to_string(), "no".to_string()));
}

#[test]
fn given_the_mobile_branch_choice_when_creating_a_worktree_then_rca_uses_the_chosen_base_and_branch() {
    // Given a repository and the branch choice sent by the mobile dialog
    let mut rig = Rig::new();
    let params = json!({
        "repo": "main", "name": "test workspace", "branchIntent": "create-new",
        "baseBranch": "origin/dev", "branchNameOverride": "dev-claude-orca",
    });
    // When Orca creates a new branch from the selected ref
    rig.ok("worktree.create", params);
    // Then RCA receives the base and branch, and the client can discover this request shape
    let requests: Vec<_> = rig.agent.requests().into_iter()
        .filter_map(|(method, params)| (method == "worktree.create").then_some(params)).collect();
    assert_eq!(requests, [json!({
        "workspace": "main", "worktree": "test-workspace", "branch": "dev-claude-orca", "base": "origin/dev",
    })]);
    assert!(WorkspaceArea::default().capabilities().contains(&"worktree.open-existing-branch.v2"));
}

#[test]
fn given_a_worktree_when_created_listed_set_and_removed_then_each_step_is_seen_by_the_next_and_its_name_is_retired() {
    // Given a repo with only its main checkout
    let mut rig = Rig::new();
    let before = rig.ok("worktree.list", json!({"repo": "main"}));
    // When a worktree is created with an unsafe name and a display name
    let made = rig.ok("worktree.create", json!({"repo": "main", "name": " my feature/1 ", "displayName": "My feature", "baseBranch": "main"}));
    let id = made["worktree"]["id"].as_str().unwrap().to_string();
    // Then it is named safely, is on its own branch and has the stored display name
    assert_eq!(before["worktrees"].as_array().unwrap().len(), 1);
    assert_eq!(id, "main::/base/my-feature-1");
    assert_eq!((made["worktree"]["branch"].clone(), made["worktree"]["displayName"].clone(), made["worktree"]["isMainWorktree"].clone()), (json!("refs/heads/my-feature-1"), json!("My feature"), json!(false)));
    // And it is listed after the main checkout, and shown by id, by path and by branch
    let listed = rig.ok("worktree.list", json!({"repo": "main"}));
    assert_eq!(listed["totalCount"], 2);
    assert_eq!(listed["worktrees"][0]["isMainWorktree"], true);
    for selector in [id.clone(), format!("id:{id}"), "path:/base/my-feature-1".to_string(), "my-feature-1".to_string()] {
        assert_eq!(rig.ok("worktree.show", json!({"worktree": selector}))["worktree"]["id"], id.as_str(), "{selector}");
    }
    // When its comment and pin are set
    let set = rig.ok("worktree.set", json!({"worktree": id, "comment": "wip", "isPinned": true}));
    // Then they read back from the list
    assert_eq!((set["worktree"]["comment"].clone(), set["worktree"]["isPinned"].clone()), (json!("wip"), json!(true)));
    assert_eq!(rig.ok("worktree.list", json!({}))["worktrees"][1]["comment"], "wip");
    // When it is removed
    assert_eq!(rig.ok("worktree.rm", json!({"worktree": id})), json!({"removed": true}));
    // Then it is gone, its name is retired for the repo, and showing it again is refused
    assert_eq!(rig.ok("worktree.list", json!({}))["totalCount"], 1);
    assert_eq!(rig.ok("worktree.listRetiredNames", json!({"repo": "main"})), json!({"retiredNamesByRepo": {"main": ["my-feature-1"]}, "retiredNameTiersByRepo": {"main": 0}}));
    assert_eq!(rig.err("worktree.show", json!({"worktree": id})).0, "selector_not_found");
}

#[test]
fn given_worktree_requests_rca_refuses_when_called_then_the_phone_gets_the_hosts_word_after_the_service_was_asked() {
    // Given a repo and a created worktree
    let mut rig = Rig::new();
    rig.ok("worktree.create", json!({"repo": "main", "name": "dup"}));
    let asked = |rig: &Rig, method: &str| rig.agent.requests().iter().filter(|(m, _)| m == method).count();
    // When the same name is created again, the main checkout is removed, and a missing selector is sent
    let again = rig.err("worktree.create", json!({"repo": "main", "name": "dup"}));
    let main = rig.err("worktree.rm", json!({"worktree": "main"}));
    let missing = rig.err("worktree.show", json!({}));
    let nobody = rig.err("worktree.create", json!({"repo": "nope"}));
    // Then the duplicate reached the service and was refused by it, the main checkout is never removed, and the others are the host's own words
    assert_eq!(asked(&rig, "worktree.create"), 2);
    assert_eq!(again.0, "runtime_error");
    assert_eq!(asked(&rig, "worktree.remove"), 0);
    assert_eq!(main, ("runtime_error".to_string(), "Cannot remove the main worktree".to_string()));
    assert_eq!(missing, ("invalid_argument".to_string(), "Missing worktree selector".to_string()));
    assert_eq!(nobody.0, "selector_not_found");
}

#[test]
fn given_a_repo_with_and_without_a_remote_when_the_create_base_is_prefetched_then_the_remote_is_fetched_and_none_is_not_a_failure() {
    // Given one repo with a remote and one without
    let mut rig = Rig::new();
    rig.files.remote("main");
    // When the base of each is prefetched
    let with = rig.ok("worktree.prefetchCreateBase", json!({"repo": "main", "baseBranch": "origin/main"}));
    let without = rig.ok("worktree.prefetchCreateBase", json!({"repo": "docs"}));
    // Then both answer null and only the repo with a remote was fetched
    assert_eq!((with, without), (Value::Null, Value::Null));
    assert_eq!(rig.files.fetched(), ["main"]);
}

#[test]
fn given_a_wrong_typed_snapshot_id_when_the_repos_are_listed_then_it_is_refused_and_a_string_is_accepted() {
    // Given a host
    let mut rig = Rig::new();
    // When the phone lists the repos after a snapshot id of the wrong type, and of the right one
    let refused = rig.err("repo.list", json!({"afterSnapshotId": 7}));
    // Then the first is refused as the schema refuses it, and the second is answered
    assert_eq!(refused, ("invalid_argument".into(), "Invalid input: expected string, received number".into()));
    rig.ok("repo.list", json!({"afterSnapshotId": "a"}));
}

#[test]
fn given_a_list_asked_for_a_zero_limit_or_a_repo_nobody_holds_when_it_is_asked_then_it_says_so_in_the_hosts_codes() {
    // Given the workspaces of a host
    let mut rig = Rig::new();
    // When the phone lists worktrees with a limit of zero and a repo that does not exist, Then the limit is refused first
    assert_eq!(rig.err("worktree.list", json!({"limit": 0, "repo": "nobody"})), ("invalid_limit".to_string(), "invalid_limit".to_string()));
    // And a repo no workspace holds is the host's repo_not_found
    assert_eq!(rig.err("worktree.list", json!({"repo": "nobody"})), ("repo_not_found".to_string(), "repo_not_found".to_string()));
}

// ---- activity: notifications, agent activity, worktree.ps, sleep and activate ----

fn terminal_info(terminal: &str, status: &str, updated_at: u64) -> Value {
    json!({"workspace": "main", "terminal": terminal, "title": terminal, "cwd": "/main", "cols": 80, "rows": 24, "status": status, "exit_code": null, "pid": 1, "offset": 0, "base_offset": 0, "updated_at": updated_at})
}

fn pane(thread: &str, workspace: &str, state: &str, unread: bool, at: u64) -> Value {
    json!({"pane": format!("thread:{thread}"), "thread": thread, "title": format!("title {thread}"), "workspace": workspace, "state": state, "listed": true, "unread": unread, "clearable": state != "working", "sort_at_ms": at, "state_started_at_ms": at})
}

/// A service with a main checkout running one terminal and one finished unread agent, and a linked worktree that sleeps.
fn busy() -> Rig {
    let mut rig = Rig::new();
    rig.files.linked_worktree("main", "feat2", "feat2");
    rig.agent.reply("terminal.list", |p| Ok(json!({"workspace": p["workspace"], "terminals": if p["workspace"] == "main" { vec![terminal_info("t1", "running", 65_000), terminal_info("t0", "exited", 90_000)] } else { vec![] }})));
    rig.agent.reply("pane.list", |_| Ok(json!({"panes": [pane("a", "main", "done", true, 4_000), pane("n", "main", "idle", false, 0), pane("w", "feat2", "working", false, 7_000)]})));
    rig.agent.reply("terminal.sleeping", |_| Ok(json!({"sleeping": []})));
    let _ = &mut rig;
    rig
}

#[test]
fn given_a_main_checkout_with_a_terminal_and_a_finished_agent_when_worktree_ps_is_asked_then_each_row_has_its_terminals_agents_and_the_status_they_make() {
    // Given a main checkout with a running terminal and a done unread agent, and a worktree whose agent works
    let mut rig = busy();
    // When the rows are asked for
    let ps = rig.ok("worktree.ps", json!({}));
    // Then the main row is unread first, with one live terminal, its output time, the done agent as the pane list shows it, and status done
    assert_eq!((ps["totalCount"].clone(), ps["truncated"].clone(), ps["hostScope"].clone()), (json!(2), json!(false), json!({"hostIds": [], "omittedHostIds": ["local"]})));
    let main = &ps["worktrees"][0];
    assert_eq!((main["worktreeId"].clone(), main["unread"].clone(), main["liveTerminalCount"].clone(), main["hasAttachedPty"].clone(), main["lastOutputAt"].clone(), main["status"].clone(), main["isMainWorktree"].clone(), main["terminalPlatform"].clone()), (json!("main::/main"), json!(true), json!(1), json!(true), json!(65_000), json!("done"), json!(true), json!("linux")));
    assert_eq!(main["agents"], json!([{
        "paneKey": "thread:a", "parentPaneKey": null, "state": "done", "agentType": null, "prompt": "", "taskTitle": null, "displayName": null, "conversationName": "title a",
        "lastAssistantMessage": null, "toolName": null, "toolInput": null, "interrupted": false, "stateStartedAt": 4000, "updatedAt": 4000,
        "activity": {"listed": true, "status": "done", "unread": true, "clearable": true, "sortAt": 4000, "title": "title a"}}]));
    // And the worktree whose agent works is working with no terminal, and a never-run pane is no agent row
    let feat = &ps["worktrees"][1];
    assert_eq!((feat["worktreeId"].clone(), feat["status"].clone(), feat["liveTerminalCount"].clone(), feat["lastOutputAt"].clone(), feat["agents"][0]["state"].clone(), feat["unread"].clone()), (json!("main::/base/feat2"), json!("working"), json!(0), Value::Null, json!("working"), json!(false)));
    assert_eq!(main["agents"].as_array().unwrap().len(), 1);
    // And a limit cuts the page, saying so, and one that is no whole number above zero is refused as the host refuses it
    let cut = rig.ok("worktree.ps", json!({"limit": 1}));
    assert_eq!((cut["worktrees"].as_array().unwrap().len(), cut["totalCount"].clone(), cut["truncated"].clone()), (1, json!(2), json!(true)));
    for bad in [json!(0), json!(1.5), json!(-2)] {
        assert_eq!(rig.err("worktree.ps", json!({"limit": bad})), ("invalid_limit".to_string(), "invalid_limit".to_string()));
    }
    assert_eq!(rig.err("worktree.ps", json!({"acceptRowDelta": false})).0, "invalid_argument");
}

#[test]
fn given_two_checkouts_when_worktree_ps_builds_their_summaries_then_it_shares_the_repo_name_read_for_that_request() {
    // Given two checkouts of a renamed repository
    let mut rig = busy();
    rig.agent.set_state("orca", "orca.repo.main", json!({"displayName": "Renamed repo"}));

    // When the phone refreshes its worktrees
    let ps = rig.ok("worktree.ps", json!({}));

    // Then both rows show that name and the listing enumerates Git checkouts only once
    let rows = ps["worktrees"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row["repo"] == "Renamed repo"));
    let calls = rig.agent.calls();
    assert_eq!(calls.iter().filter(|method| method.as_str() == "worktree.list").count(), 1);
    assert_eq!(calls.iter().filter(|method| method.as_str() == "state.get").count(), 3);

    // The next request sees a later name
    rig.agent.set_state("orca", "orca.repo.main", json!({"displayName": "Another name"}));
    let next = rig.ok("worktree.ps", json!({}));
    assert!(next["worktrees"].as_array().unwrap().iter().all(|row| row["repo"] == "Another name"));
}

#[test]
fn given_a_caller_that_sends_snapshot_ids_when_the_rows_change_then_it_gets_the_id_then_unchanged_then_the_rows_it_does_not_hold() {
    // Given the rows of a busy service asked for with row deltas accepted
    let mut rig = busy();
    let first = rig.ok("worktree.ps", json!({"acceptRowDelta": true}));
    let id = first["snapshotId"].as_str().unwrap().to_string();
    // Then the reply has an id of 32 characters and its preview is blank
    assert_eq!((id.len(), first["worktrees"][0]["preview"].clone(), first["worktrees"][0]["lastOutputAt"].clone()), (32, json!(""), json!(60_000)));
    // And asking again with that id answers unchanged
    assert_eq!(rig.ok("worktree.ps", json!({"afterSnapshotId": id, "acceptRowDelta": true})), json!({"unchanged": true, "snapshotId": id}));
    // When the agent of the main checkout is read, the reply names the base and gives the unchanged row as its place in the base
    rig.agent.reply("pane.list", |_| Ok(json!({"panes": [pane("a", "main", "done", false, 4_000), pane("w", "feat2", "working", false, 7_000)]})));
    let next = rig.ok("worktree.ps", json!({"afterSnapshotId": id, "acceptRowDelta": true}));
    assert_eq!((next["baseSnapshotId"].clone(), next.get("worktrees").is_none(), next["totalCount"].clone()), (json!(id), true, json!(2)));
    let rows = next["rowsFromBase"].as_array().unwrap();
    assert!(rows[0].is_object() && rows[0]["unread"] == false, "{next}");
    assert_eq!(rows[1], json!(1));
    // And an id the host never gave gets the whole reply with a new id
    let unknown = rig.ok("worktree.ps", json!({"afterSnapshotId": "zzz", "acceptRowDelta": true}));
    assert!(unknown["worktrees"].is_array() && unknown["snapshotId"].is_string());
}

#[test]
fn given_a_worktree_with_a_working_agent_when_it_sleeps_and_is_activated_then_the_agent_is_stopped_and_the_terminals_slept_and_woken_and_its_finished_agents_are_read() {
    // Given a service where the linked worktree's workspace runs a thread
    let mut rig = busy();
    rig.agent.reply("thread.list", |_| Ok(json!({"threads": [
        {"id": "w", "title": "w", "workspace": "feat2", "status": "running"}, {"id": "x", "title": "x", "workspace": "main", "status": "running"}, {"id": "i", "title": "i", "workspace": "feat2", "status": "idle"}]})));
    rig.agent.reply("thread.forceStop", |_| Ok(json!({})));
    rig.agent.reply("terminal.sleep", |p| Ok(json!({"workspace": p["workspace"], "slept": [], "already": false})));
    // When the worktree sleeps
    let slept = rig.ok("worktree.sleep", json!({"worktree": "main::/base/feat2"}));
    // Then the reply names it, only its running thread was stopped, and its terminals were put to sleep
    assert_eq!(slept, json!({"worktreeId": "main::/base/feat2"}));
    let sent = rig.agent.requests();
    assert_eq!(sent.iter().filter(|(m, _)| m == "thread.forceStop").map(|(_, p)| p.clone()).collect::<Vec<_>>(), [json!({"thread": "w"})]);
    assert_eq!(sent.iter().find(|(m, _)| m == "terminal.sleep").map(|(_, p)| p.clone()), Some(json!({"workspace": "feat2"})));
    // And with markChatsAsleep false no agent is stopped
    let before = sent.iter().filter(|(m, _)| m == "thread.forceStop").count();
    rig.ok("worktree.sleep", json!({"worktree": "feat2", "markChatsAsleep": false}));
    assert_eq!(rig.agent.requests().iter().filter(|(m, _)| m == "thread.forceStop").count(), before);
    // When the worktree wakes: a finished unread agent of main is read, and the sleeping terminals start again
    rig.agent.reply("terminal.sleeping", |_| Ok(json!({"sleeping": [{"workspace": "main", "slept_at_ms": 1, "terminals": []}]})));
    rig.agent.reply("pane.mark", |_| Ok(json!({"marked": 1})));
    rig.agent.reply("terminal.wake", |p| Ok(json!({"workspace": p["workspace"], "woken": [], "failed": []})));
    let woke = rig.ok("worktree.activate", json!({"worktree": "main::/main", "navigation": "caller"}));
    // Then the reply is the host's, the unread pane was marked read and the workspace woken
    assert_eq!(woke, json!({"repoId": "main", "worktreeId": "main::/main", "activated": true, "sleepingAgentWake": "not-applicable"}));
    let sent = rig.agent.requests();
    assert_eq!(sent.iter().find(|(m, _)| m == "pane.mark").map(|(_, p)| p.clone()), Some(json!({"panes": ["thread:a"], "mark": "read"})));
    assert_eq!(sent.iter().find(|(m, _)| m == "terminal.wake").map(|(_, p)| p.clone()), Some(json!({"workspace": "main"})));
    // And a selector nobody has, a missing one and a bad option are refused in the host's words
    assert_eq!(rig.err("worktree.sleep", json!({"worktree": "ghost"})).0, "selector_not_found");
    assert_eq!(rig.err("worktree.activate", json!({})), ("invalid_argument".to_string(), "Missing worktree selector".to_string()));
    assert_eq!(rig.err("worktree.sleep", json!({"worktree": "feat2", "markChatsAsleep": "no"})).1, "Invalid input: expected boolean, received string");
    // And a service that cannot stop the shells is not reported as asleep
    rig.agent.reply("terminal.sleep", |_| Err(orca_core::AgentError::Refused { code: "NO_EXECUTOR".into(), message: "no executor is attached for workspace feat2".into() }));
    assert_eq!(rig.err("worktree.sleep", json!({"worktree": "feat2"})), ("runtime_error".to_string(), "no executor is attached for workspace feat2".to_string()));
}

#[test]
fn given_pane_keys_when_agent_activity_is_set_or_cleared_then_the_service_is_asked_to_mark_them_and_the_host_answers_applied_and_a_bad_list_is_refused() {
    // Given a service that marks panes
    let mut rig = Rig::new();
    rig.agent.reply("pane.mark", |p| Ok(json!({"marked": p["panes"].as_array().map_or(0, Vec::len)})));
    // When two panes are set read, one unread and one cleared
    assert_eq!(rig.ok("agentActivity.setRead", json!({"paneKeys": ["thread:a", "thread:b"], "read": true})), json!({"applied": true}));
    assert_eq!(rig.ok("agentActivity.setRead", json!({"paneKeys": ["thread:a"], "read": false})), json!({"applied": true}));
    assert_eq!(rig.ok("agentActivity.clear", json!({"paneKeys": ["thread:b"]})), json!({"applied": true}));
    // Then the marks went to the service in that order
    let marks: Vec<Value> = rig.agent.requests().into_iter().filter(|(m, _)| m == "pane.mark").map(|(_, p)| p).collect();
    assert_eq!(marks, [json!({"panes": ["thread:a", "thread:b"], "mark": "read"}), json!({"panes": ["thread:a"], "mark": "unread"}), json!({"panes": ["thread:b"], "mark": "clear"})]);
    // And each bad list is refused as the host's schema refuses it, before the service is asked
    assert_eq!(rig.err("agentActivity.clear", json!({})).1, "Invalid input: expected array, received undefined");
    assert_eq!(rig.err("agentActivity.clear", json!({"paneKeys": "x"})).1, "Invalid input: expected array, received string");
    assert_eq!(rig.err("agentActivity.clear", json!({"paneKeys": []})).1, "Too small: expected array to have >=1 items");
    assert_eq!(rig.err("agentActivity.setRead", json!({"paneKeys": ["a"]})).1, "Invalid input: expected boolean, received undefined");
    assert_eq!(rig.agent.requests().iter().filter(|(m, _)| m == "pane.mark").count(), 3);
}

#[test]
fn given_a_journal_when_missed_notifications_are_asked_and_the_stream_is_opened_then_entries_come_as_the_hosts_events_with_the_worktree_and_pane_and_unsubscribe_ends_the_stream() {
    // Given a journal with one entry about a thread that runs in the main workspace
    let mut rig = Rig::new();
    let entry = json!({"seq": 1, "id": "n-abcd-1", "cause": "turn.completed", "title": "Fix build", "body": "Finished", "thread_id": "a", "workspace": "main", "at_ms": 7});
    let shown = entry.clone();
    rig.agent.reply("notification.since", move |p| Ok(json!({"epoch": "e-1", "notifications": if p["after_seq"] == 0 { vec![shown.clone()] } else { vec![] }})));
    // When a phone that saw nothing, and one that saw it, ask what they missed
    let all = rig.ok("notifications.getMissedSince", json!({"lastSeenSeq": 0, "deliveredPushes": [{"notificationId": "x", "notificationEpoch": "e-0", "notificationSeq": 0}]}));
    let none = rig.ok("notifications.getMissedSince", json!({"lastSeenSeq": 1, "epoch": "e-1"}));
    // Then the first gets the entry in the host's words, with no push dismissed, and the second gets none
    assert_eq!(all, json!({"epoch": "e-1", "dismissedPushes": [], "notifications": [{
        "type": "notification", "source": "agent-task-complete", "title": "Fix build", "body": "Finished", "worktreeId": "main::/main", "paneKey": "thread:a",
        "notificationId": "n-abcd-1", "notificationSeq": 1, "notificationEpoch": "e-1", "emittedAt": 7, "agentState": "done", "desktopAway": true}]}));
    assert_eq!(none, json!({"epoch": "e-1", "notifications": []}));
    assert_eq!(rig.agent.requests().iter().filter(|(m, _)| m == "notification.since").map(|(_, p)| p.clone()).collect::<Vec<_>>(), [json!({"after_seq": 0}), json!({"after_seq": 1, "epoch": "e-1"})]);
    // And a request without lastSeenSeq or with a negative or fractional one is refused in the host's words
    assert_eq!(rig.err("notifications.getMissedSince", json!({})).1, "Invalid input: expected number, received undefined");
    assert_eq!(rig.err("notifications.getMissedSince", json!({"lastSeenSeq": -1})).1, "lastSeenSeq must be a non-negative integer");
    assert_eq!(rig.err("notifications.getMissedSince", json!({"lastSeenSeq": 0.5})).1, "Invalid input: expected int, received number");
    // When the stream is opened and a waiting entry is made, a finished one and an entry of another kind of event come
    let feed = rig.agent.script_stream("notification.subscribe", &json!({}), Ok(json!({"topic": "notifications", "epoch": "e-1", "seq": 1, "reset": false})), vec![]);
    let opened = rig.conn.on_message(&json!({"id": "s1", "method": "notifications.subscribe", "params": {}}).to_string());
    let frames: Vec<Value> = opened.iter().map(|f| serde_json::from_str::<Value>(f).unwrap()["result"].clone()).collect();
    let id = frames[0]["subscriptionId"].as_str().unwrap().to_string();
    assert_eq!(frames[0], json!({"type": "ready", "subscriptionId": id, "epoch": "e-1"}));
    let mut waiting = entry.clone();
    waiting["cause"] = "input.requested".into();
    waiting["body"] = "which?".into();
    feed.push(2, "notification.posted", {
        let mut data = waiting.as_object().unwrap().clone();
        data.remove("seq");
        Value::Object(data)
    });
    feed.push(3, "schedule.run", json!({}));
    let live: Vec<Value> = rig.conn.poll().iter().map(|f| serde_json::from_str::<Value>(f).unwrap()["result"].clone()).collect();
    // Then only the entry comes, as a waiting notification numbered 2
    assert_eq!(live.len(), 1);
    assert_eq!((live[0]["notificationSeq"].clone(), live[0]["agentState"].clone(), live[0]["body"].clone(), live[0]["type"].clone()), (json!(2), json!("waiting"), json!("which?"), json!("notification")));
    // And unsubscribing ends the stream with the end frame, answers true, and an id nobody holds answers true as well
    let ended = rig.conn.on_message(&json!({"id": "u1", "method": "notifications.unsubscribe", "params": {"subscriptionId": id}}).to_string());
    let results: Vec<Value> = ended.iter().map(|f| serde_json::from_str::<Value>(f).unwrap()["result"].clone()).collect();
    assert!(results.contains(&json!({"unsubscribed": true})) && results.contains(&json!({"type": "end"})), "{results:?}");
    assert_eq!(rig.ok("notifications.unsubscribe", json!({"subscriptionId": "nobody"})), json!({"unsubscribed": true}));
    assert_eq!(rig.err("notifications.unsubscribe", json!({"subscriptionId": 7})).1, "Missing subscriptionId");
}

#[test]
fn given_the_methods_of_the_activity_rows_when_the_area_is_read_then_each_is_served_and_the_agent_methods_they_send_are_all_in_uses() {
    // Given the rows this module serves
    let rig = Rig::new();
    let methods: BTreeSet<&str> = ["worktree.ps", "worktree.sleep", "worktree.activate", "agentActivity.setRead", "agentActivity.clear", "notifications.getMissedSince", "notifications.subscribe", "notifications.unsubscribe"].into_iter().collect();
    // Then each is in the area's method list and none is declared not offered
    let area = <WorkspaceArea as orca_core::Area>::methods(&WorkspaceArea::default());
    assert!(methods.iter().all(|m| area.iter().any(|a| a.name == *m)));
    assert!(NOT_OFFERED.iter().all(|n| !methods.contains(n.name)));
    // And the agent methods it sends are listed in USES
    let uses: BTreeSet<&str> = USES.iter().map(|m| m.name()).collect();
    for m in ["notification.since", "pane.list", "pane.mark", "terminal.list", "terminal.sleep", "terminal.wake", "terminal.sleeping", "thread.list", "thread.forceStop"] {
        assert!(uses.contains(m), "{m}");
    }
    drop(rig);
}

fn open_tabs(rig: &Rig, workspace: &str) -> Value {
    rig.agent.state_get("orca", &format!("tabs.extra.{workspace}")).unwrap().unwrap_or(json!([]))
}

#[test]
fn given_files_when_opened_then_each_is_a_kept_tab_with_the_hosts_kind_and_a_missing_file_or_a_directory_is_refused() {
    // Given a worktree with a source file, a markdown file, an image and a directory
    let mut rig = Rig::new();
    rig.files.put("feat", "src/a.ts", b"x").put("feat", "README.md", b"# hi").put("feat", "logo.png", b"\x89PNG");
    let wt = "path:/srv/wt/feat";
    // When each is opened
    let text = rig.ok("files.open", json!({"worktree": wt, "relativePath": "src/a.ts", "line": 3}));
    let md = rig.ok("files.open", json!({"worktree": wt, "relativePath": "README.md"}));
    let img = rig.ok("files.open", json!({"worktree": wt, "relativePath": "logo.png"}));
    // Then the answer names the worktree id and the kind, and the kept tabs hold them in order with the last opened in front
    assert_eq!(text, json!({"worktree": "feat::/srv/wt/feat", "relativePath": "src/a.ts", "kind": "text", "opened": true}));
    assert_eq!((md["kind"].clone(), img["kind"].clone()), (json!("markdown"), json!("image")));
    let ids: Vec<Value> = open_tabs(&rig, "feat").as_array().unwrap().iter().map(|t| t["id"].clone()).collect();
    assert_eq!(ids, [json!("file:src/a.ts"), json!("markdown:README.md"), json!("file:logo.png")]);
    assert_eq!(rig.agent.state_get("orca", "tabs.active.feat").unwrap(), Some(json!("file:logo.png")));
    // And opening one again keeps one tab, a missing file, a directory and an unsafe path are refused, and no tab was added by them
    rig.ok("files.open", json!({"worktree": wt, "relativePath": "src/a.ts"}));
    assert_eq!(open_tabs(&rig, "feat").as_array().unwrap().len(), 3);
    assert_eq!(rig.err("files.open", json!({"worktree": wt, "relativePath": "nope.ts"})).1, "ENOENT: no such file or directory, open '/srv/wt/feat/nope.ts'");
    assert_eq!(rig.err("files.open", json!({"worktree": wt, "relativePath": "src"})).1, "EISDIR: illegal operation on a directory, open '/srv/wt/feat/src'");
    assert_eq!(rig.err("files.open", json!({"worktree": wt, "relativePath": "../x"})).1, "invalid_relative_path");
    assert_eq!(rig.err("files.open", json!({"worktree": "a", "relativePath": "a"})).0, "selector_not_found");
    assert_eq!(rig.err("files.open", json!({"relativePath": "a"})).0, "invalid_argument");
    assert_eq!(open_tabs(&rig, "feat").as_array().unwrap().len(), 3);
}

#[test]
fn given_a_diff_request_when_opened_then_a_diff_tab_for_its_side_is_kept_without_needing_the_file() {
    // Given a worktree
    let mut rig = Rig::new();
    // When a staged and an unstaged diff of a renamed file are opened
    let staged = rig.ok("files.openDiff", json!({"worktree": "path:/srv/wt/feat", "relativePath": "gone.md", "staged": true, "oldPath": "old.md"}));
    rig.ok("files.openDiff", json!({"worktree": "path:/srv/wt/feat", "relativePath": "gone.md"}));
    // Then the answer is the host's, and two diff tabs are kept, the staged one with its old path
    assert_eq!(staged, json!({"worktree": "feat::/srv/wt/feat", "relativePath": "gone.md", "kind": "markdown", "opened": true}));
    let tabs = open_tabs(&rig, "feat");
    assert_eq!((tabs[0]["id"].clone(), tabs[0]["diffSource"].clone(), tabs[0]["oldPath"].clone()), (json!("diff:staged:gone.md"), json!("staged"), json!("old.md")));
    assert_eq!(tabs[1]["id"], json!("diff:unstaged:gone.md"));
}

#[test]
fn given_a_markdown_tab_when_read_and_saved_then_the_file_is_the_document_and_a_stale_version_conflicts() {
    // Given a markdown tab opened on a file
    let mut rig = Rig::new();
    let wt = "path:/srv/wt/feat";
    rig.files.put("feat", "README.md", b"# one\n").put("feat", "a.ts", b"x");
    rig.ok("files.open", json!({"worktree": wt, "relativePath": "README.md"}));
    let tab = "markdown:README.md";
    // When it is read
    let read = rig.ok("markdown.readTab", json!({"worktree": wt, "tabId": tab}));
    // Then the answer is the file with its version, editable
    assert_eq!(read, json!({"tabId": tab, "filePath": "/srv/wt/feat/README.md", "relativePath": "README.md", "content": "# one\n", "isDirty": false, "version": orca_map_workspace::layout::content_version("# one\n"), "source": "file", "editable": true}));
    // When it is saved on that version
    let saved = rig.ok("markdown.saveTab", json!({"worktree": wt, "tabId": tab, "baseVersion": read["version"], "content": "# two\n"}));
    // Then the file holds the new text and the answer carries the new version
    assert_eq!(saved, json!({"tabId": tab, "isDirty": false, "version": orca_map_workspace::layout::content_version("# two\n"), "content": "# two\n"}));
    assert_eq!(rig.files.read("feat", "README.md").unwrap(), b"# two\n");
    // And a save on the old version conflicts and leaves the file, the same save again succeeds, and an unknown or non-markdown tab is not found
    assert_eq!(rig.err("markdown.saveTab", json!({"worktree": wt, "tabId": tab, "baseVersion": read["version"], "content": "# three\n"})).1, "conflict");
    assert_eq!(rig.files.read("feat", "README.md").unwrap(), b"# two\n");
    rig.ok("markdown.saveTab", json!({"worktree": wt, "tabId": tab, "baseVersion": read["version"], "content": "# two\n"}));
    rig.ok("files.open", json!({"worktree": wt, "relativePath": "a.ts"}));
    assert_eq!(rig.err("markdown.readTab", json!({"worktree": wt, "tabId": "file:a.ts"})).1, "tab_not_found");
    assert_eq!(rig.err("markdown.readTab", json!({"worktree": "a", "tabId": "a"})).0, "selector_not_found");
    assert_eq!(rig.err("markdown.saveTab", json!({"worktree": wt, "tabId": tab, "baseVersion": "v", "content": "x".repeat(256 * 1024 + 1)})).1, "file_too_large");
}

#[test]
fn given_executors_when_the_ssh_rows_are_called_then_each_is_a_target_that_reads_connected_while_attached() {
    // Given the host's one attached executor
    let mut rig = Rig::new();
    // When the targets are listed, one is read, and a connect is asked for it and for a stranger
    let listed = rig.ok("ssh.listTargetSummaries", json!({}));
    let state = rig.ok("ssh.getState", json!({"targetId": "e1"}));
    let connected = rig.ok("ssh.connect", json!({"targetId": "e1"}));
    // Then the target is the executor and connected, the state is the host's shape, and an unknown target has no state and cannot connect
    assert_eq!(listed, json!({"targets": [{"id": "e1", "label": "e1", "connected": true, "connectionStatus": "connected"}]}));
    assert_eq!(state, json!({"state": {"targetId": "e1", "status": "connected", "error": null, "reconnectAttempt": 0}}));
    assert_eq!(connected, state);
    assert_eq!(rig.ok("ssh.getState", json!({"targetId": "a"})), json!({"state": null}));
    assert_eq!(rig.err("ssh.connect", json!({"targetId": "a"})), ("runtime_error".to_string(), "SSH connection unavailable".to_string()));
    assert_eq!(rig.err("ssh.getState", json!({"targetId": 7})).0, "invalid_argument");
}

#[test]
fn given_two_phones_opening_files_of_one_worktree_at_once_when_both_read_the_kept_tabs_first_then_both_tabs_are_kept() {
    // Given one worktree and two connections whose reads of the kept tabs overlap
    let scripted = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
    let files = Workspaces::default();
    files.workspace("feat", "worktree", Some("/srv/wt/feat")).put("feat", "a.ts", b"a").put("feat", "b.ts", b"b");
    files.install(&scripted);
    let agent: Arc<dyn AgentApi> = Arc::new(orca_core::testing::OverlappingReads::over(scripted.clone(), std::time::Duration::from_millis(300)));
    let host = Host::new("rt".into(), vec![Box::new(WorkspaceArea::default())], agent, Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
    // When each phone opens a different file at the same moment
    let phones: Vec<_> = ["a.ts", "b.ts"]
        .into_iter()
        .map(|file| {
            let mut conn = host.connection("dev", "tok", vec![]);
            std::thread::spawn(move || {
                let frame = json!({"id": "r1", "method": "files.open", "params": {"worktree": "path:/srv/wt/feat", "relativePath": file}}).to_string();
                serde_json::from_str::<Value>(&conn.on_message(&frame)[0]).unwrap()
            })
        })
        .collect();
    for phone in phones {
        assert_eq!(phone.join().unwrap()["ok"], true);
    }
    // Then neither tab replaced the other
    let mut ids: Vec<String> = scripted.state_get("orca", "tabs.extra.feat").unwrap().unwrap().as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect();
    ids.sort();
    assert_eq!(ids, ["file:a.ts", "file:b.ts"]);
}

fn tap(rig: &mut Rig, path: &str) -> Value {
    rig.ok("files.resolveTerminalPath", json!({"worktree": "main", "pathText": path}))
}

fn grant(resolved: &Value) -> Value {
    json!({"worktree": "main", "grantId": resolved["openTarget"]["grantId"], "absolutePath": resolved["absolutePath"]})
}

#[test]
fn given_a_path_below_a_workspace_with_a_known_root_when_tapped_then_it_opens_as_that_workspaces_file_and_no_grant_is_made() {
    // Given a docs workspace rooted at /srv/docs holding a.txt
    let mut rig = Rig::new();
    rig.files.put("docs", "a.txt", b"x").put("docs", "sub/b.txt", b"y");
    // When an absolute path, a relative path against a terminal cwd, a directory and a missing file are tapped
    let abs = rig.ok("files.resolveTerminalPath", json!({"worktree": "docs", "pathText": "/srv/docs/a.txt"}));
    let rel = rig.ok("files.resolveTerminalPath", json!({"worktree": "docs", "pathText": "b.txt", "cwd": "/srv/docs/sub"}));
    let dir = rig.ok("files.resolveTerminalPath", json!({"worktree": "docs", "pathText": "sub"}));
    let gone = rig.ok("files.resolveTerminalPath", json!({"worktree": "docs", "pathText": "nope.txt"}));
    // Then the first two open as worktree files, the directory is one, the missing file is absent, and rca was asked for no artifact
    let open = |rel: &str, abs: &str| json!({"worktree": "docs::/srv/docs", "relativePath": rel, "absolutePath": abs, "exists": true, "isDirectory": false, "openTarget": {"kind": "worktree-file", "provider": "local", "relativePath": rel, "absolutePath": abs}});
    assert_eq!(abs, open("a.txt", "/srv/docs/a.txt"));
    assert_eq!(rel, open("sub/b.txt", "/srv/docs/sub/b.txt"));
    assert_eq!(dir, json!({"worktree": "docs::/srv/docs", "relativePath": "sub", "absolutePath": "/srv/docs/sub", "exists": true, "isDirectory": true}));
    assert_eq!(gone, json!({"worktree": "docs::/srv/docs", "relativePath": "nope.txt", "absolutePath": "/srv/docs/nope.txt", "exists": false, "isDirectory": false}));
    assert!(rig.agent.requests().iter().all(|(m, _)| m != "artifact.stat"));
}

#[test]
fn given_a_file_outside_the_workspace_when_tapped_then_it_is_granted_and_read_previewed_and_saved_under_the_grant() {
    // Given a log the executor serves beyond its root
    let mut rig = Rig::new();
    rig.files.artifact("/var/log/build.log", b"first line\n");
    // When it is tapped
    let resolved = tap(&mut rig, "/var/log/build.log");
    // Then the answer is an absolute-file grant with the file's size
    let id = resolved["openTarget"]["grantId"].as_str().unwrap().to_string();
    assert_eq!(resolved, json!({"worktree": "main::", "relativePath": null, "absolutePath": "/var/log/build.log", "exists": true, "isDirectory": false, "openTarget": {"kind": "absolute-file", "provider": "local", "absolutePath": "/var/log/build.log", "grantId": id, "byteLength": 11}}));
    // When it is read, previewed and saved, and read again
    let read = rig.ok("files.readTerminalArtifact", grant(&resolved));
    let preview = rig.ok("files.readTerminalArtifactPreview", grant(&resolved));
    let mut save = grant(&resolved);
    save["content"] = json!("second\n");
    let saved = rig.ok("files.writeTerminalArtifact", save);
    let again = rig.ok("files.readTerminalArtifact", grant(&resolved));
    // Then each answers as the host's rows do, and the file on the executor holds the saved text
    assert_eq!(read, json!({"worktree": "main::", "relativePath": "/var/log/build.log", "content": "first line\n", "truncated": false, "byteLength": 11}));
    assert_eq!(preview, json!({"content": "first line\n", "isBinary": false}));
    assert_eq!(saved, json!({"ok": true}));
    assert_eq!(again["content"], "second\n");
    assert_eq!(rig.files.artifact_bytes("/var/log/build.log").unwrap(), b"second\n");
}

#[test]
fn given_a_granted_file_when_someone_else_rewrites_it_then_read_and_save_are_refused_as_stale_and_the_rewrite_survives() {
    // Given a grant for a file
    let mut rig = Rig::new();
    rig.files.artifact("/tmp/x.txt", b"mine");
    let resolved = tap(&mut rig, "/tmp/x.txt");
    // When another writer replaces the file and the phone reads and then saves
    rig.files.rewrite_artifact("/tmp/x.txt", b"theirs");
    let read = rig.err("files.readTerminalArtifact", grant(&resolved));
    let mut save = grant(&resolved);
    save["content"] = json!("overwrite");
    let write = rig.err("files.writeTerminalArtifact", save);
    // Then both are refused as stale and the other writer's text stands
    assert_eq!(read.1, "terminal_file_grant_stale");
    assert_eq!(write.1, "terminal_file_grant_stale");
    assert_eq!(rig.files.artifact_bytes("/tmp/x.txt").unwrap(), b"theirs");
}

#[test]
fn given_a_grant_when_used_for_another_path_or_unknown_or_expired_then_it_is_refused_with_the_hosts_codes() {
    // Given a grant for one file
    let mut rig = Rig::new();
    rig.files.artifact("/tmp/a.txt", b"a").artifact("/tmp/b.txt", b"b");
    let resolved = tap(&mut rig, "/tmp/a.txt");
    // When it is used for the other file, an unknown id is used, and a missing grant id is sent
    let mut other = grant(&resolved);
    other["absolutePath"] = json!("/tmp/b.txt");
    let mismatch = rig.err("files.readTerminalArtifact", other);
    let mut unknown = grant(&resolved);
    unknown["grantId"] = json!("00000000-0000-4000-8000-000000000000");
    let expired = rig.err("files.readTerminalArtifact", unknown);
    let missing = rig.call("files.readTerminalArtifact", json!({"worktree": "main", "absolutePath": "/tmp/a.txt"}));
    // Then the host's messages answer, and the grant itself still reads
    assert_eq!(mismatch.1, "terminal_file_grant_mismatch");
    assert_eq!(expired.1, "terminal_file_grant_expired");
    assert_eq!(missing["error"]["message"], "Missing terminal artifact grant");
    assert_eq!(rig.ok("files.readTerminalArtifact", grant(&resolved))["content"], "a");
}

#[test]
fn given_a_grant_minted_for_one_device_when_another_device_uses_it_then_it_is_refused_and_the_owner_still_reads() {
    // Given a host with a file and two paired devices on it
    let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
    let files = Workspaces::default();
    files.workspace("main", "root", None).artifact("/tmp/a.txt", b"a");
    files.install(&agent);
    let host = Host::new("rt".into(), vec![Box::new(WorkspaceArea::default())], agent, Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
    let (mut first, mut second) = (host.connection("phone-a", "tok", vec![]), host.connection("phone-b", "tok", vec![]));
    let ask = |conn: &mut orca_core::connection::Connection, method: &str, params: Value| -> Value { serde_json::from_str(&conn.on_message(&json!({"id": "1", "method": method, "params": params}).to_string())[0]).unwrap() };
    // When device A taps the path and device B presents A's grant for the same worktree and path
    let resolved = ask(&mut first, "files.resolveTerminalPath", json!({"worktree": "main", "pathText": "/tmp/a.txt"}))["result"].clone();
    let stolen = ask(&mut second, "files.readTerminalArtifact", grant(&resolved));
    let stolen_write = ask(&mut second, "files.writeTerminalArtifact", json!({"worktree": "main", "grantId": resolved["openTarget"]["grantId"], "absolutePath": "/tmp/a.txt", "content": "x"}));
    // Then B is refused as a mismatch for the read and the write, and A's grant still reads
    assert_eq!((stolen["ok"].clone(), stolen["error"]["message"].clone()), (json!(false), json!("terminal_file_grant_mismatch")));
    assert_eq!((stolen_write["ok"].clone(), stolen_write["error"]["message"].clone()), (json!(false), json!("terminal_file_grant_mismatch")));
    assert_eq!(ask(&mut first, "files.readTerminalArtifact", grant(&resolved))["result"]["content"], "a");
}

#[test]
fn given_grants_that_live_no_time_when_the_phone_uses_one_then_it_has_expired() {
    // Given a host whose grants expire at once
    let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
    let files = Workspaces::default();
    files.workspace("main", "root", None).artifact("/tmp/a.txt", b"a");
    files.install(&agent);
    let area = WorkspaceArea::with_grants(orca_map_workspace::Grants::with_ttl(std::time::Duration::ZERO));
    let host = Host::new("rt".into(), vec![Box::new(area)], agent, Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
    let mut rig_conn = host.connection("dev", "tok", vec![]);
    let mut call = |id: &str, method: &str, params: Value| -> Value { serde_json::from_str(&rig_conn.on_message(&json!({"id": id, "method": method, "params": params}).to_string())[0]).unwrap() };
    // When a path is tapped and its grant used
    let resolved = call("1", "files.resolveTerminalPath", json!({"worktree": "main", "pathText": "/tmp/a.txt"}));
    let read = call("2", "files.readTerminalArtifact", json!({"worktree": "main", "grantId": resolved["result"]["openTarget"]["grantId"], "absolutePath": "/tmp/a.txt"}));
    // Then the grant is gone
    assert_eq!(read["error"]["message"], "terminal_file_grant_expired");
}

#[test]
fn given_the_executor_serves_nothing_outside_its_root_when_a_path_there_is_tapped_then_it_is_refused_and_no_grant_exists() {
    // Given an executor that refuses everything under /etc as outside
    let mut rig = Rig::new();
    rig.files.refuse_outside("/etc");
    // When /etc/hostname is tapped
    let refused = rig.err("files.resolveTerminalPath", json!({"worktree": "main", "pathText": "/etc/hostname"}));
    // Then the phone is told why, and a path the executor does not have is absent
    assert_eq!(refused.1, "Files outside this workspace are not granted on this host.");
    let absent = tap(&mut rig, "/srv/none.txt");
    assert_eq!((absent["exists"].clone(), absent["openTarget"].clone()), (json!(false), Value::Null));
}

#[test]
fn given_a_directory_a_binary_file_a_home_path_and_oversized_text_when_tapped_or_saved_then_each_is_told_apart() {
    // Given a directory with a file, a binary file and a text file
    let mut rig = Rig::new();
    rig.files.artifact("/data/d/f.txt", b"f").artifact("/data/p.bin", &[0, 1, 2]).artifact("/data/t.txt", b"t");
    // When each is tapped, with a home path too
    let dir = tap(&mut rig, "/data/d");
    let bin = tap(&mut rig, "/data/p.bin");
    let home = tap(&mut rig, "~/notes.txt");
    let text = tap(&mut rig, "/data/t.txt");
    // Then the directory carries no grant, the binary one is marked and not read as text, and the home path is absent
    assert_eq!(dir, json!({"worktree": "main::", "relativePath": null, "absolutePath": "/data/d", "exists": true, "isDirectory": true}));
    assert_eq!(bin["openTarget"]["binary"], true);
    assert_eq!(home, json!({"worktree": "main::", "relativePath": null, "absolutePath": null, "exists": false, "isDirectory": false}));
    assert_eq!(rig.err("files.readTerminalArtifact", grant(&bin)).1, "binary_file");
    // And a save over the 512 KiB the host allows is refused unwritten, a save with no content is invalid
    let mut big = grant(&text);
    big["content"] = json!("x".repeat(512 * 1024 + 1));
    assert_eq!(rig.err("files.writeTerminalArtifact", big).1, "file_too_large");
    assert_eq!(rig.call("files.writeTerminalArtifact", grant(&text))["error"]["message"], "Missing file content");
    assert_eq!(rig.files.artifact_bytes("/data/t.txt").unwrap(), b"t");
}

#[test]
fn given_an_expo_thread_when_orca_discovers_then_closes_it_then_the_tab_stays_closed() {
    let rig = Rig::new();
    let agent = rig.agent.as_ref();
    agent.set_state("mobile.thread.workspace", "mobile-id", json!("feat"));
    let discover = || orca_map_workspace::layout::import_mobile_chat(agent, "feat", "mobile-id", "codex", "From Expo").unwrap();
    discover();
    discover();
    let tabs = orca_map_workspace::layout::kept(agent, "feat").unwrap();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0]["sessionId"], "mobile-id");
    assert!(orca_map_workspace::layout::close(agent, "feat", "mobile-id").unwrap());
    discover();
    assert!(orca_map_workspace::layout::kept(agent, "feat").unwrap().is_empty());
}
