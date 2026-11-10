//! The phone's files and project rows through the chain: what it lists, reads, stats, previews, searches and creates is what the real
//! rca-exec holds on its disk, reached through the real rca-service. A row counts only when the agent method it maps to ran in its call's window
//! and was answered, and a refused call is shown to have reached the service.
use crate::rig::*;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

const WS: &str = "w1";

fn walk(dir: &Path, base: &Path, out: &mut BTreeSet<String>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            out.insert(p.strip_prefix(base).unwrap().to_string_lossy().into_owned());
        }
    }
}

fn on_disk(chain: &Chain) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk(&chain.repo.path(), &chain.repo.path(), &mut out);
    out
}

fn answered(case: &exec_testkit::Case, method: &str) {
    let window = last_window(case);
    assert!(window.iter().any(|(m, ok)| m == method && *ok), "{method} did not answer in the call's window: {window:?}");
}

fn b64(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(text).unwrap()
}

#[test]
fn a_phone_lists_reads_and_previews_the_files_the_executor_holds_and_every_byte_it_gets_is_the_disks() {
    exec_testkit::case!(SUITE, |case| {
        // Given the executor's repository with text files, a folder, an image, a binary larger than one window and a text file past the read limit
        let chain = Chain::start(None);
        let png: Vec<u8> = (0..3000u32).map(|i| (i * 7 % 256) as u8).collect();
        let blob: Vec<u8> = (0..1300u32).map(|i| (i % 251) as u8 + 1).collect();
        chain.repo.write("README.md", "# hello\nworld\n");
        chain.repo.write("src/main.rs", "fn main() {}\n");
        chain.repo.write("src/lib.rs", "pub fn f() {}\n");
        chain.repo.write("big.txt", &"x".repeat(512 * 1024 + 100));
        std::fs::create_dir_all(chain.repo.path().join("img")).unwrap();
        std::fs::write(chain.repo.path().join("img/logo.png"), &png).unwrap();
        std::fs::write(chain.repo.path().join("data.bin"), &blob).unwrap();
        let mut phone = Phone::connect(case, &chain);
        let sel = json!({"worktree": format!("id:{WS}::")});
        let with = |extra: Value| {
            let mut p = sel.clone();
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            p
        };
        // When it lists the workspace, Then the listing is the files on disk, nothing cut, each with a kind
        let listed = phone.call(case, &chain, &["rpc:files.list"], "files.list", sel.clone());
        answered(case, "file.list");
        let paths: BTreeSet<String> = listed["result"]["files"].as_array().unwrap().iter().map(|f| f["relativePath"].as_str().unwrap().to_string()).collect();
        assert_eq!(paths, on_disk(&chain), "{}", chain.evidence());
        assert_eq!(listed["result"]["truncated"], false);
        let kind = |p: &str| listed["result"]["files"].as_array().unwrap().iter().find(|f| f["relativePath"] == p).unwrap()["kind"].clone();
        assert_eq!((kind("img/logo.png"), kind("src/main.rs")), (json!("binary"), json!("text")));
        // When it opens the root folder and a folder, Then the names are the disk's with folders first, and a paged read walks the same entries
        let root = phone.call(case, &chain, &["rpc:files.readDir"], "files.readDir", with(json!({"relativePath": ""})));
        answered(case, "file.list");
        let names: Vec<(&str, bool)> = root["result"].as_array().unwrap().iter().map(|e| (e["name"].as_str().unwrap(), e["isDirectory"].as_bool().unwrap())).collect();
        assert_eq!(names, [("img", true), ("src", true), ("big.txt", false), ("data.bin", false), ("README.md", false)], "{}", chain.evidence());
        let page = phone.call(case, &chain, &["rpc:files.readDir"], "files.readDir", with(json!({"relativePath": "", "limit": 2, "after": {"name": "img", "isDirectory": true}})));
        assert_eq!((page["result"]["entries"][0]["name"].clone(), page["result"]["total"].clone(), page["result"]["hasMore"].clone()), (json!("src"), json!(5), json!(true)));
        // When it reads a text file and the over-long one, Then the first is the file and the second is cut at the host's limit
        let readme = phone.call(case, &chain, &["rpc:files.read"], "files.read", with(json!({"relativePath": "README.md"})));
        answered(case, "file.readChunk");
        assert_eq!(readme["result"]["content"], std::fs::read_to_string(chain.repo.path().join("README.md")).unwrap());
        let big = phone.call(case, &chain, &["rpc:files.read"], "files.read", with(json!({"relativePath": "big.txt"})));
        assert_eq!((big["result"]["truncated"].clone(), big["result"]["content"].as_str().unwrap().len()), (json!(true), 512 * 1024));
        // When it downloads the binary in windows with the size from stat, Then it reassembles the disk's bytes and the stat is the disk's
        let stat = phone.call(case, &chain, &["rpc:files.stat"], "files.stat", with(json!({"relativePath": "data.bin"})));
        answered(case, "file.stat");
        let meta = std::fs::metadata(chain.repo.path().join("data.bin")).unwrap();
        let disk_ms = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        assert_eq!((stat["result"]["size"].clone(), stat["result"]["isDirectory"].clone()), (json!(blob.len()), json!(false)));
        assert!(stat["result"]["mtime"].as_u64().unwrap().abs_diff(disk_ms) < 2000, "{stat}");
        let (mut got, mut offset) = (Vec::new(), 0usize);
        while got.len() < blob.len() {
            let chunk = phone.call(case, &chain, &["rpc:files.readChunk"], "files.readChunk", with(json!({"relativePath": "data.bin", "offset": offset, "length": 512})));
            answered(case, "file.readChunk");
            let piece = b64(chunk["result"]["contentBase64"].as_str().unwrap());
            assert_eq!(chunk["result"]["bytesRead"], piece.len());
            offset += piece.len();
            got.extend(piece);
            assert_eq!(chunk["result"]["eof"], got.len() >= blob.len());
        }
        assert_eq!(got, blob);
        // When it previews the image, Then it gets the disk's bytes with their type
        let image = phone.call(case, &chain, &["rpc:files.readPreview"], "files.readPreview", with(json!({"relativePath": "img/logo.png"})));
        answered(case, "file.readChunk");
        assert_eq!((b64(image["result"]["content"].as_str().unwrap()), image["result"]["mimeType"].clone(), image["result"]["isImage"].clone()), (png, json!("image/png"), json!(true)));
        // When it searches paths, Then the best match is the file the query names, and no folder is offered
        let found = phone.call(case, &chain, &["rpc:files.searchPaths"], "files.searchPaths", with(json!({"query": "main", "limit": 5, "mode": "quick-open"})));
        answered(case, "file.search");
        let hits: Vec<&str> = found["result"]["files"].as_array().unwrap().iter().map(|f| f["relativePath"].as_str().unwrap()).collect();
        assert_eq!(hits, ["src/main.rs"], "{}", chain.evidence());
    });
}

#[test]
fn a_phone_creates_a_file_that_is_then_on_the_executors_disk_and_a_name_that_is_taken_is_refused_by_the_executor() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a name no file has
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        let params = json!({"worktree": WS, "relativePath": "notes.md"});
        assert!(!chain.repo.path().join("notes.md").exists());
        // When the phone creates it, reads it back and creates it again
        let first = phone.call(case, &chain, &["rpc:files.createFile"], "files.createFile", params.clone());
        answered(case, "file.create");
        let read = phone.call(case, &chain, &["rpc:files.read"], "files.read", params.clone());
        let again = phone.refused(case, &chain, &["rpc:files.createFile"], "files.createFile", params);
        // Then the file is on the disk empty, the phone reads it empty, and the second is refused with the executor's sentence after the call reached the service
        assert_eq!(first["result"], json!({"ok": true}));
        assert_eq!(std::fs::read(chain.repo.path().join("notes.md")).unwrap(), Vec::<u8>::new());
        assert_eq!((read["result"]["content"].clone(), read["result"]["byteLength"].clone()), (json!(""), json!(0)));
        assert_eq!(again["error"]["code"], "runtime_error", "{}", chain.evidence());
        assert!(again["error"]["message"].as_str().unwrap().contains("already exists"), "{again}");
        assert!(last_window(case).iter().any(|(m, ok)| m == "file.create" && !ok), "the refusal did not come from the service: {:?}", last_window(case));
    });
}

#[test]
fn a_path_that_leaves_the_workspace_is_refused_on_the_phone_and_the_executor_refuses_it_too_so_nothing_outside_is_read_or_written() {
    exec_testkit::case!(SUITE, |case| {
        // Given a secret file beside the executor's repository and a repository file
        let chain = Chain::start(None);
        let outside = chain.repo.path().parent().unwrap().join("outside-secret.txt");
        std::fs::write(&outside, "TOP-SECRET").unwrap();
        chain.repo.write("ok.txt", "fine");
        let mut phone = Phone::connect(case, &chain);
        let escapes = ["../outside-secret.txt", "a/../../outside-secret.txt", "/etc/hostname"];
        // When the phone reads, creates and stats such paths, Then each is refused naming the path and no byte of the secret is in a reply
        for path in escapes {
            for (id, method) in [("rpc:files.read", "files.read"), ("rpc:files.stat", "files.stat"), ("rpc:files.createFile", "files.createFile"), ("rpc:files.readPreview", "files.readPreview"), ("rpc:files.readChunk", "files.readChunk"), ("rpc:files.readDir", "files.readDir")] {
                let reply = phone.refused(case, &chain, &[id], method, json!({"worktree": WS, "relativePath": path}));
                assert!(!reply.to_string().contains("TOP-SECRET"), "{method} {path}: {reply}");
                assert_eq!(reply["error"]["message"], "invalid_relative_path", "{method} {path}: {reply}");
            }
        }
        // And a path inside still works, so the refusal is the path's and not the chain's
        let ok = phone.call(case, &chain, &["rpc:files.read"], "files.read", json!({"worktree": WS, "relativePath": "ok.txt"}));
        assert_eq!(ok["result"]["content"], "fine");
        // Then the secret is unchanged and nothing was created beside the repository
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "TOP-SECRET");
        // When the same path goes straight to the service, Then the executor itself refuses it (the mapper is not the only guard)
        let mut direct = chain.direct();
        let straight = direct.call("files.edit", "file.readChunk", json!({"workspace": WS, "path": "../outside-secret.txt", "offset": 0, "length": 64}));
        assert!(straight.as_ref().map_or(true, |v| !v.to_string().contains("TOP-SECRET")), "the executor served a path outside its workspace: {straight:?}");
        assert!(straight.is_err(), "the executor answered a path outside its workspace: {straight:?}");
    });
}

#[test]
fn rows_that_name_a_desktop_tab_are_refused_as_not_offered_and_ask_the_service_for_nothing() {
    exec_testkit::case!(SUITE, |case| {
        // Given a connected phone
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        for (method, row) in [("files.open", "rpc:files.open"), ("files.openDiff", "rpc:files.openDiff"), ("markdown.readTab", "rpc:markdown.readTab"), ("markdown.saveTab", "rpc:markdown.saveTab")] {
            // When it calls one
            let r = phone.refused(case, &chain, &[row], method, json!({"worktree": WS, "relativePath": "README.md", "tabId": "t1", "content": "x"}));
            // Then it is not offered, by M6, and nothing was asked of the service
            assert_eq!((r["error"]["code"].clone(), r["error"]["data"].clone()), (json!("not_offered"), json!({"decidedBy": "M6"})), "{method}");
            assert_eq!(last_window(case), Vec::<(String, bool)>::new(), "{method}");
        }
    });
}

#[test]
fn a_phone_reads_the_project_catalog_and_folder_workspaces_rca_holds_and_a_workspace_made_by_another_device_appears() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a folder in the repository that another device makes a workspace of and names, and a folder workspace it stores
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        let before = phone.call(case, &chain, &["rpc:mobileRelay.projectSections"], "mobileRelay.projectSections", json!({}));
        answered(case, "repo.list");
        let ids = |reply: &Value| -> Vec<String> { reply["result"]["catalog"]["repos"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect() };
        assert_eq!(ids(&before), [WS], "{}", chain.evidence());
        std::fs::create_dir_all(chain.repo.path().join("site")).unwrap();
        let mut other = chain.direct();
        other.call("workspace", "workspace.create", json!({"workspace": "site", "path": "site"})).expect("rca makes the workspace");
        other.call("state", "state.set", json!({"scope": "orca", "key": "orca.repo.site", "value": {"displayName": "The site", "badgeColor": "#00aa55"}})).expect("rca stores the name");
        let none = phone.call(case, &chain, &["rpc:folderWorkspace.list"], "folderWorkspace.list", json!({}));
        other.call("state", "state.set", json!({"scope": "orca", "key": "orca.folder-workspaces", "value": [{"id": "f1", "name": "Scratch"}]})).expect("rca stores the row");
        // When the phone reads the catalog and the folder workspaces again
        let after = phone.call(case, &chain, &["rpc:mobileRelay.projectSections"], "mobileRelay.projectSections", json!({}));
        let folders = phone.call(case, &chain, &["rpc:folderWorkspace.list"], "folderWorkspace.list", json!({}));
        // Then the new workspace is a project with the name and colour stored for it, the root keeps its own, and the folder row is the stored one
        assert_eq!(ids(&after), ["site", WS], "{}", chain.evidence());
        let site = &after["result"]["catalog"]["repos"][0];
        assert_eq!((site["displayName"].clone(), site["badgeColor"].clone()), (json!("The site"), json!("#00aa55")));
        assert_eq!(after["result"]["catalog"]["projects"][0], json!({"id": "site", "displayName": "The site"}));
        assert_eq!(none["result"], json!({"folderWorkspaces": []}));
        assert_eq!(folders["result"], json!({"folderWorkspaces": [{"id": "f1", "name": "Scratch"}]}));
        answered(case, "state.get");
    });
}

fn git_paths(chain: &Chain) -> BTreeSet<String> {
    chain.repo.git(&["worktree", "list", "--porcelain"]).lines().filter_map(|l| l.strip_prefix("worktree ")).map(|p| std::fs::canonicalize(p).map(|c| c.display().to_string()).unwrap_or_else(|_| p.to_string())).collect()
}

fn canon(path: &str) -> String {
    std::fs::canonicalize(path).map(|c| c.display().to_string()).unwrap_or_else(|_| path.to_string())
}

#[test]
fn a_phone_creates_lists_sets_reads_into_and_removes_a_worktree_and_each_step_is_what_git_has_on_the_executors_disk() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a phone that reads the repo, project, host setup and group views first
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        let repos = phone.call(case, &chain, &["rpc:repo.list"], "repo.list", json!({}));
        answered(case, "repo.list");
        let projects = phone.call(case, &chain, &["rpc:project.list"], "project.list", json!({}));
        let setups = phone.call(case, &chain, &["rpc:projectHostSetup.list"], "projectHostSetup.list", json!({}));
        let groups = phone.call(case, &chain, &["rpc:projectGroup.list"], "projectGroup.list", json!({}));
        answered(case, "state.get");
        assert_eq!((repos["result"]["repos"][0]["id"].clone(), projects["result"]["projects"][0]["sourceRepoIds"].clone(), setups["result"]["setups"][0]["repoId"].clone()), (json!(WS), json!([WS]), json!(WS)), "{}", chain.evidence());
        assert_eq!(groups["result"], json!({"groups": []}));
        // When it lists the worktrees
        let before = phone.call(case, &chain, &["rpc:worktree.list"], "worktree.list", json!({"repo": WS}));
        answered(case, "worktree.list");
        // Then there is the main checkout, at the repository's path on git's branch
        let rows = before["result"]["worktrees"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{}", chain.evidence());
        assert_eq!((canon(rows[0]["path"].as_str().unwrap()), rows[0]["isMainWorktree"].clone(), rows[0]["branch"].clone()), (canon(chain.repo.path().to_str().unwrap()), json!(true), json!("refs/heads/main")));
        // When it creates a worktree
        let made = phone.call(case, &chain, &["rpc:worktree.create"], "worktree.create", json!({"repo": WS, "name": "feat a", "baseBranch": "main"}));
        answered(case, "worktree.create");
        let id = made["result"]["worktree"]["id"].as_str().expect("a worktree id").to_string();
        let path = made["result"]["worktree"]["path"].as_str().unwrap().to_string();
        // Then a directory with the repository's files is under the executor's worktree base, and git lists it on its own branch
        assert!(std::path::Path::new(&path).starts_with(&chain.base) && std::path::Path::new(&path).join("README.md").is_file(), "{path} is no checkout: {}", chain.evidence());
        assert!(git_paths(&chain).contains(&canon(&path)));
        assert_eq!(made["result"]["worktree"]["branch"], "refs/heads/feat-a");
        assert_eq!(chain.repo.git(&["branch", "--list", "feat-a"]).trim().trim_start_matches(['*', '+']).trim(), "feat-a");
        // When it lists again, shows it by id and by path, and lists its files by its id
        let after = phone.call(case, &chain, &["rpc:worktree.list"], "worktree.list", json!({}));
        let shown = phone.call(case, &chain, &["rpc:worktree.show"], "worktree.show", json!({"worktree": id}));
        let by_path = phone.call(case, &chain, &["rpc:worktree.show"], "worktree.show", json!({"worktree": format!("path:{path}")}));
        std::fs::write(std::path::Path::new(&path).join("only-here.txt"), "x").unwrap();
        let files = phone.call(case, &chain, &["rpc:files.list"], "files.list", json!({"worktree": id}));
        answered(case, "file.list");
        // Then both are listed, the show is the same row, and the files are the worktree's own, not the main checkout's
        assert_eq!(after["result"]["totalCount"], 2);
        assert_eq!((shown["result"]["worktree"]["id"].clone(), by_path["result"]["worktree"]["id"].clone()), (json!(id), json!(id)));
        let listed: BTreeSet<&str> = files["result"]["files"].as_array().unwrap().iter().map(|f| f["relativePath"].as_str().unwrap()).collect();
        assert!(listed.contains("only-here.txt") && listed.contains("README.md"), "{files}");
        assert!(!chain.repo.path().join("only-here.txt").exists());
        // When it sets a comment, Then another device reads the same value from rca's state and the list shows it
        let set = phone.call(case, &chain, &["rpc:worktree.set"], "worktree.set", json!({"worktree": id, "comment": "wip", "isPinned": true}));
        answered(case, "state.set");
        let stored = chain.direct().call("state", "state.get", json!({"scope": "orca", "key": format!("orca.worktree.{id}")})).expect("rca reads it");
        assert_eq!((set["result"]["worktree"]["comment"].clone(), stored["value"]["comment"].clone(), stored["value"]["isPinned"].clone()), (json!("wip"), json!("wip"), json!(true)));
        // When it creates the same name again, Then the executor refuses it after the call reached the service
        let again = phone.refused(case, &chain, &["rpc:worktree.create"], "worktree.create", json!({"repo": WS, "name": "feat a"}));
        assert_eq!(again["error"]["code"], "runtime_error", "{}", chain.evidence());
        assert!(last_window(case).iter().any(|(m, ok)| m == "worktree.create" && !ok), "{:?}", last_window(case));
        // When it removes the worktree, Then the directory and git's entry are gone and its name is retired
        let removed = phone.call(case, &chain, &["rpc:worktree.rm"], "worktree.rm", json!({"worktree": id, "force": true}));
        answered(case, "worktree.remove");
        assert_eq!(removed["result"]["removed"], true, "{}", chain.evidence());
        assert!(!std::path::Path::new(&path).exists() && !git_paths(&chain).contains(&canon(&path)));
        let retired = phone.call(case, &chain, &["rpc:worktree.listRetiredNames"], "worktree.listRetiredNames", json!({"repo": WS}));
        assert_eq!(retired["result"]["retiredNamesByRepo"], json!({WS: ["feat-a"]}));
        // And the main checkout is never removed
        let main = phone.refused(case, &chain, &["rpc:worktree.rm"], "worktree.rm", json!({"worktree": rows[0]["id"]}));
        assert_eq!(main["error"]["message"], "Cannot remove the main worktree");
        assert!(chain.repo.path().join("README.md").is_file());
    });
}

#[test]
fn a_phone_updates_a_repo_saves_a_preset_reads_hooks_and_refs_and_prefetches_a_remote_and_each_matches_the_executors_repository() {
    exec_testkit::case!(SUITE, |case| {
        // Given a repository with an orca.yaml, a feature branch and a tag, and a remote that holds a newer commit than the repository has fetched
        let chain = Chain::start(None);
        chain.repo.write("orca.yaml", "scripts:\n  setup: npm ci\n  archive: |\n    echo a\n    echo b\n");
        chain.repo.commit_all("hooks");
        chain.repo.git(&["branch", "feature/x"]);
        chain.repo.git(&["tag", "v1"]);
        let remote = chain.state.path().join("remote.git");
        let other = chain.state.path().join("other");
        chain.repo.git(&["init", "--bare", "-b", "main", remote.to_str().unwrap()]);
        chain.repo.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        chain.repo.git(&["push", "origin", "main"]);
        chain.repo.git(&["fetch", "origin"]);
        chain.repo.git(&["clone", remote.to_str().unwrap(), other.to_str().unwrap()]);
        std::fs::write(other.join("new.txt"), "n").unwrap();
        chain.repo.git(&["-C", other.to_str().unwrap(), "add", "-A"]);
        chain.repo.git(&["-C", other.to_str().unwrap(), "commit", "-m", "ahead"]);
        chain.repo.git(&["-C", other.to_str().unwrap(), "push", "origin", "main"]);
        let remote_head = chain.repo.git(&["-C", remote.to_str().unwrap(), "rev-parse", "main"]).trim().to_string();
        let tracked = || chain.repo.git(&["rev-parse", "origin/main"]).trim().to_string();
        assert_ne!(tracked(), remote_head);
        let mut phone = Phone::connect(case, &chain);
        let sel = json!({"repo": WS});
        let with = |extra: Value| {
            let mut p = sel.clone();
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            p
        };
        // When it reads the hooks, Then they are the file's scripts
        let hooks = phone.call(case, &chain, &["rpc:repo.hooks"], "repo.hooks", sel.clone());
        answered(case, "file.readChunk");
        assert_eq!((hooks["result"]["hasHooksFile"].clone(), hooks["result"]["hooks"]["scripts"].clone()), (json!(true), json!({"setup": "npm ci", "archive": "echo a\necho b"})), "{}", chain.evidence());
        // When it reads the default base and searches refs, Then the default is the remote's main and the search is git's branches and remote refs without tags
        let base = phone.call(case, &chain, &["rpc:repo.baseRefDefault"], "repo.baseRefDefault", sel.clone());
        answered(case, "git.refs");
        let search = phone.call(case, &chain, &["rpc:repo.searchRefs"], "repo.searchRefs", with(json!({"query": "", "limit": 50})));
        let found = phone.call(case, &chain, &["rpc:repo.searchRefs"], "repo.searchRefs", with(json!({"query": "feature"})));
        assert_eq!((base["result"]["defaultBaseRef"].clone(), base["result"]["remoteCount"].clone()), (json!("origin/main"), json!(1)), "{}", chain.evidence());
        let refs: BTreeSet<&str> = search["result"]["refs"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
        assert!(refs.contains("main") && refs.contains("feature/x") && refs.contains("origin/main") && !refs.contains("v1"), "{refs:?}");
        assert_eq!(found["result"]["refs"], json!(["feature/x"]));
        // When it prefetches the create base, Then the remote's newer commit is in the repository's remote-tracking branch
        let fetched = phone.call(case, &chain, &["rpc:worktree.prefetchCreateBase"], "worktree.prefetchCreateBase", with(json!({"baseBranch": "origin/main"})));
        answered(case, "git.fetch");
        assert_eq!(fetched["result"], Value::Null);
        assert_eq!(tracked(), remote_head, "{}", chain.evidence());
        // When it updates the repo and saves a preset twice, Then another device reads the stored name and the one preset with the new content, and rca's id and path are kept
        let updated = phone.call(case, &chain, &["rpc:repo.update"], "repo.update", with(json!({"updates": {"displayName": "Hello", "badgeColor": "#123456", "id": "x"}})));
        answered(case, "state.set");
        let first = phone.call(case, &chain, &["rpc:repo.saveSparsePreset"], "repo.saveSparsePreset", with(json!({"name": "Web", "directories": ["apps/web"]})));
        let id = first["result"]["preset"]["id"].as_str().unwrap().to_string();
        phone.call(case, &chain, &["rpc:repo.saveSparsePreset"], "repo.saveSparsePreset", with(json!({"id": id, "name": "Web+", "directories": ["apps/web", "libs"]})));
        let presets = phone.call(case, &chain, &["rpc:repo.sparsePresets"], "repo.sparsePresets", sel.clone());
        let stored = chain.direct().call("state", "state.get", json!({"scope": "orca", "key": "orca.repo.w1"})).expect("rca reads it");
        assert_eq!((updated["result"]["repo"]["id"].clone(), updated["result"]["repo"]["displayName"].clone(), stored["value"]["displayName"].clone()), (json!(WS), json!("Hello"), json!("Hello")));
        assert!(stored["value"].get("id").is_none(), "{stored}");
        assert_eq!(presets["result"]["presets"].as_array().unwrap().len(), 1);
        assert_eq!((presets["result"]["presets"][0]["name"].clone(), presets["result"]["presets"][0]["directories"].clone()), (json!("Web+"), json!(["apps/web", "libs"])));
        // And a repo nobody knows is refused with the host's word after the list was asked of the service
        let nobody = phone.refused(case, &chain, &["rpc:repo.update"], "repo.update", json!({"repo": "nope", "updates": {}}));
        assert_eq!(nobody["error"]["code"], "selector_not_found");
        assert!(last_window(case).iter().any(|(m, ok)| m == "repo.list" && *ok), "{:?}", last_window(case));
    });
}
