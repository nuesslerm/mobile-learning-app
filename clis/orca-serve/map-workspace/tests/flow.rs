//! The files rows through the `Host` dispatcher with an in-memory workspace set as rca: each reply is read the way the phone reads it, and the
//! agent methods the mapper sent are held to its `USES` list. Given/When/Then in each test.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
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
        let host = Host::new("rt".into(), vec![Box::new(WorkspaceArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
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
fn given_projects_when_the_catalog_is_read_then_roots_and_directories_are_listed_named_as_stored_and_worktrees_are_not() {
    // Given workspaces of the three kinds and a stored name for one
    let mut rig = Rig::new();
    rig.agent.set_state("orca", "orca.repo.docs", json!({"displayName": "Docs site", "badgeColor": "#ff0000"}));
    // When the phone reads the catalog and the folder workspaces
    let sections = rig.ok("mobileRelay.projectSections", json!({}));
    let folders = rig.ok("folderWorkspace.list", json!({}));
    // Then the repos are the root and the directory, the directory carries its stored name and colour, and the worktree is no project
    let repos = sections["catalog"]["repos"].as_array().unwrap();
    let ids: Vec<&str> = repos.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["docs", "main"]);
    assert_eq!((repos[0]["displayName"].clone(), repos[0]["badgeColor"].clone(), repos[0]["path"].clone()), (json!("Docs site"), json!("#ff0000"), json!("/srv/docs")));
    assert_eq!(sections["catalog"]["projects"], json!([{"id": "docs", "displayName": "Docs site"}, {"id": "main", "displayName": "main"}]));
    assert_eq!(sections["catalog"]["projectOrderBy"], "manual");
    // And no folder workspace is listed until one is stored, and then it is the stored row
    assert_eq!(folders, json!({"folderWorkspaces": []}));
    rig.agent.set_state("orca", "orca.folder-workspaces", json!([{"id": "f1", "name": "Scratch"}]));
    assert_eq!(rig.ok("folderWorkspace.list", json!({})), json!({"folderWorkspaces": [{"id": "f1", "name": "Scratch"}]}));
}

#[test]
fn given_every_row_of_the_area_when_called_then_the_agent_methods_sent_are_exactly_the_ones_listed() {
    // Given each served files row called once, so that every code path sends what it sends
    let mut rig = Rig::new();
    rig.files.put("main", "a.txt", b"x").put("main", "i.png", &[1]);
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
    // When the methods asked of the agent API are collected
    let sent: BTreeSet<String> = rig.agent.requests().into_iter().map(|(m, _)| m).collect();
    // Then they are the list the crate declares (state reads are the port's own call)
    let declared: BTreeSet<String> = USES.iter().map(|m| m.name().to_string()).collect();
    assert_eq!(sent, declared);
}

#[test]
fn given_the_rows_the_area_does_not_offer_when_called_then_each_answers_not_offered_with_its_reason_which_is_the_decisions_files() {
    // Given the decisions file and a host with the area
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/plans/rca-parity/coverage/orca-decisions.json");
    let decisions: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut rig = Rig::new();
    assert!(!NOT_OFFERED.is_empty());
    for row in NOT_OFFERED {
        // When the row is called
        let r = rig.call(row.name, json!({"worktree": "main", "relativePath": "a.txt"}));
        // Then it answers not_offered with the reason and the milestone the decisions file records for it, byte for byte
        let d = &decisions["decisions"][format!("rpc:{}", row.name)];
        assert_eq!(d["state"], "not-offered", "{}", row.name);
        assert_eq!((r["error"]["code"].clone(), r["error"]["data"].clone()), (json!("not_offered"), json!({"decidedBy": d["decidedBy"]})), "{}", row.name);
        assert_eq!(r["error"]["message"], format!("{} is not offered by this host: {}", row.name, d["reason"].as_str().unwrap()), "{}", row.name);
        assert_eq!(row.reason, d["reason"].as_str().unwrap());
    }
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
