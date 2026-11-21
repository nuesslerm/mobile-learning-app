//! The terminal-artifact actions on real files: off until the executor is told where they may reach, then served below those places only, and a
//! read or write that carries the identity and fingerprint of a grant is refused with CHANGED once the file is another one.
use exec_proto::Code;
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::execute;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn call(ex: &LocalExecutor, tool: &str, args: Value) -> Result<Value, exec_proto::ExecError> {
    execute(ex, "c1", tool, &args, &mut |_, _| {}, &mut |_| {})
}

fn dir() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = std::fs::canonicalize(d.path()).unwrap();
    (d, p)
}

fn granted(stat: &Value) -> Value {
    json!({"identity": stat["identity"], "fingerprint": stat["fingerprint"]})
}

fn with(mut args: Value, extra: Value) -> Value {
    args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    args
}

fn text_of(read: &Value) -> String {
    use base64::Engine;
    String::from_utf8(base64::engine::general_purpose::STANDARD.decode(read["data"].as_str().unwrap()).unwrap()).unwrap()
}

fn shown(root: &Path, outside: &Path) -> (LocalExecutor, PathBuf) {
    let file = outside.join("log.txt");
    std::fs::write(&file, "build ok\n").unwrap();
    (LocalExecutor::new(root).unwrap().with_artifact_allow(&[outside.to_path_buf()]).unwrap(), file)
}

#[test]
fn given_no_allowed_directory_when_a_file_outside_the_root_is_asked_for_then_every_action_refuses_it_as_outside_and_says_nothing_of_whether_it_exists() {
    // Given an executor told of no artifact directory, and a file beside its root
    let ((_r, root), (_o, outside)) = (dir(), dir());
    std::fs::write(outside.join("log.txt"), "build ok\n").unwrap();
    let ex = LocalExecutor::new(&root).unwrap();
    let there = outside.join("log.txt").display().to_string();
    let absent = outside.join("absent.txt").display().to_string();
    // When each action is asked for the file that is there and for one that is not
    for path in [&there, &absent] {
        let answers = [
            call(&ex, "artifact_stat", json!({"path": path})),
            call(&ex, "artifact_read", json!({"path": path})),
            call(&ex, "artifact_write", json!({"path": path, "content": "x"})),
        ];
        // Then each is refused as outside the root, in the same words for both
        for answer in answers {
            let error = answer.unwrap_err();
            assert_eq!((error.code, error.message), (Code::PathOutsideRoot, format!("{path} is outside the root")));
        }
    }
    // And the file was not touched
    assert_eq!(std::fs::read_to_string(outside.join("log.txt")).unwrap(), "build ok\n");
}

#[test]
fn given_an_allowed_directory_when_a_file_there_is_described_read_and_written_then_the_grant_follows_the_file_and_a_rewrite_by_someone_else_is_refused_as_changed() {
    // Given an executor allowed to serve one directory outside its root, with a log in it
    let ((_r, root), (_o, outside)) = (dir(), dir());
    let (ex, file) = shown(&root, &outside);
    let path = file.display().to_string();
    // When the file is described
    let stat = call(&ex, "artifact_stat", json!({"path": path})).unwrap();
    // Then its canonical path, size and kind are told, with an identity and a fingerprint, and it is text
    assert_eq!((stat["path"].as_str(), stat["kind"].clone(), stat["size"].clone(), stat["binary"].clone()), (Some(path.as_str()), json!("file"), json!(9), json!(false)));
    assert!(stat["identity"].as_str().is_some_and(|i| !i.is_empty()) && stat["fingerprint"].as_str().is_some_and(|f| f.len() == 16), "{stat}");
    // When it is read under the grant, the bytes come back
    let read = call(&ex, "artifact_read", with(json!({"path": path}), granted(&stat))).unwrap();
    assert_eq!((text_of(&read), read["total_bytes"].clone(), read["eof"].clone()), ("build ok\n".to_string(), json!(9), json!(true)));
    // When it is written under the grant
    let written = call(&ex, "artifact_write", with(json!({"path": path, "content": "build failed\n"}), granted(&stat))).unwrap();
    // Then the file holds the content, kept its mode, left no temporary file, and the answer names the new identity and fingerprint
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "build failed\n");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    assert_eq!(written["bytes"], 13);
    assert_ne!(written["fingerprint"], stat["fingerprint"]);
    let again = call(&ex, "artifact_stat", json!({"path": path})).unwrap();
    assert_eq!((written["identity"].clone(), written["fingerprint"].clone()), (again["identity"].clone(), again["fingerprint"].clone()));
    // When another program rewrites the file (the same size, so only its content tells)
    std::fs::write(&file, "BUILD FAILED\n").unwrap();
    // Then a read and a write under the grant from before the rewrite are refused as changed, and the rewrite stands
    let read = call(&ex, "artifact_read", with(json!({"path": path}), granted(&again))).unwrap_err();
    let write = call(&ex, "artifact_write", with(json!({"path": path, "content": "mine"}), granted(&again))).unwrap_err();
    assert_eq!((read.code, write.code), (Code::Changed, Code::Changed));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "BUILD FAILED\n");
}

#[test]
fn given_an_allowed_directory_when_a_path_leads_out_of_it_then_it_is_refused_and_a_link_is_judged_by_where_it_ends() {
    // Given an allowed directory holding a link to a file elsewhere, a link to a file inside, and a file beside it that is not allowed
    let ((_r, root), (_o, outside), (_e, elsewhere)) = (dir(), dir(), dir());
    let (ex, file) = shown(&root, &outside);
    std::fs::write(elsewhere.join("secret.txt"), "no").unwrap();
    std::os::unix::fs::symlink(elsewhere.join("secret.txt"), outside.join("out-link")).unwrap();
    std::os::unix::fs::symlink(&file, outside.join("in-link")).unwrap();
    // When each is described, and a path with dots that climbs out of the directory
    let out = call(&ex, "artifact_stat", json!({"path": outside.join("out-link").display().to_string()})).unwrap_err();
    let inside = call(&ex, "artifact_stat", json!({"path": outside.join("in-link").display().to_string()})).unwrap();
    let climb = call(&ex, "artifact_stat", json!({"path": outside.join("../secret.txt").display().to_string()})).unwrap_err();
    // Then the link out is refused, the link in answers the canonical path of its target, and the climb is refused
    assert_eq!(out.code, Code::PathOutsideRoot);
    assert_eq!(inside["path"].as_str(), Some(file.display().to_string().as_str()));
    assert_eq!(climb.code, Code::PathOutsideRoot);
}

#[test]
fn given_a_directory_a_binary_file_and_a_read_only_file_when_asked_then_each_is_told_apart_and_nothing_is_created_or_replaced() {
    // Given an allowed directory with a directory, a binary file and a read-only file in it
    let ((_r, root), (_o, outside)) = (dir(), dir());
    let (ex, _file) = shown(&root, &outside);
    std::fs::create_dir(outside.join("sub")).unwrap();
    std::fs::write(outside.join("pic.bin"), b"ab\0cd").unwrap();
    std::fs::write(outside.join("ro.txt"), "keep").unwrap();
    std::fs::set_permissions(outside.join("ro.txt"), std::fs::Permissions::from_mode(0o444)).unwrap();
    let at = |name: &str| outside.join(name).display().to_string();
    // When each is described
    let sub = call(&ex, "artifact_stat", json!({"path": at("sub")})).unwrap();
    let bin = call(&ex, "artifact_stat", json!({"path": at("pic.bin")})).unwrap();
    // Then the directory has no fingerprint and the binary file is marked
    assert_eq!((sub["kind"].clone(), sub["fingerprint"].clone(), bin["binary"].clone()), (json!("dir"), Value::Null, json!(true)));
    // And a directory is not read or written, a path that is not there is not created, and a read-only file is not replaced
    assert_eq!(call(&ex, "artifact_read", json!({"path": at("sub")})).unwrap_err().code, Code::NotAFile);
    assert_eq!(call(&ex, "artifact_write", json!({"path": at("sub"), "content": "x"})).unwrap_err().code, Code::NotAFile);
    assert_eq!(call(&ex, "artifact_write", json!({"path": at("new.txt"), "content": "x"})).unwrap_err().code, Code::NotFound);
    assert!(!outside.join("new.txt").exists());
    assert_eq!(call(&ex, "artifact_write", json!({"path": at("ro.txt"), "content": "x"})).unwrap_err().code, Code::Io);
    assert_eq!(std::fs::read_to_string(outside.join("ro.txt")).unwrap(), "keep");
}

#[test]
fn given_content_over_the_cap_when_written_then_it_is_refused_unread_and_no_debug_text_of_the_request_shows_the_content() {
    // Given an executor with an allowed file
    let ((_r, root), (_o, outside)) = (dir(), dir());
    let (ex, file) = shown(&root, &outside);
    // When content longer than the cap is written
    let long = "x".repeat(exec_fs::ARTIFACT_WRITE_MAX + 1);
    let refused = call(&ex, "artifact_write", json!({"path": file.display().to_string(), "content": long})).unwrap_err();
    // Then it is refused as invalid and the file is as it was
    assert_eq!(refused.code, Code::InvalidArgs);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "build ok\n");
    // And the text a program formats from a parsed write shows its size, not its words
    let parsed = exec_fs::ArtifactWrite::parse(&json!({"path": "p", "content": "private words"})).unwrap();
    assert!(!format!("{parsed:?}").contains("private words") && format!("{parsed:?}").contains("13 bytes"));
}

#[test]
fn given_a_file_longer_than_the_hash_cap_when_written_then_it_is_refused_and_left_as_it_was() {
    // Given an allowed file longer than the fingerprint covers, and no fingerprint to carry
    let ((_r, root), (_o, outside)) = (dir(), dir());
    let (ex, _file) = shown(&root, &outside);
    let big = outside.join("big.log");
    std::fs::File::create(&big).unwrap().set_len(exec_fs::ARTIFACT_HASH_MAX + 1).unwrap();
    // When it is written
    let refused = call(&ex, "artifact_write", json!({"path": big.display().to_string(), "content": "x"})).unwrap_err();
    // Then it is refused as invalid and its length is unchanged
    assert_eq!(refused.code, Code::InvalidArgs);
    assert_eq!(std::fs::metadata(&big).unwrap().len(), exec_fs::ARTIFACT_HASH_MAX + 1);
}

#[test]
fn given_a_workspace_below_the_root_when_an_artifact_outside_it_is_asked_for_then_only_its_own_files_and_the_allowed_directories_are_served() {
    // Given an executor with an allowed directory, and a file in the workspace directory, one in a sibling directory of the root, and one in the allowed directory
    let ((_r, root), (_o, outside)) = (dir(), dir());
    let (ex, allowed) = shown(&root, &outside);
    std::fs::create_dir_all(root.join("apps/web")).unwrap();
    std::fs::create_dir_all(root.join("apps/api")).unwrap();
    std::fs::write(root.join("apps/web/a.txt"), "mine").unwrap();
    std::fs::write(root.join("apps/api/b.txt"), "sibling").unwrap();
    let at = |p: &Path| p.display().to_string();
    // When each is read as the workspace `apps/web`
    let own = call(&ex, "artifact_read", json!({"path": at(&root.join("apps/web/a.txt")), "within": "apps/web"}));
    let sibling = call(&ex, "artifact_read", json!({"path": at(&root.join("apps/api/b.txt")), "within": "apps/web"}));
    let write = call(&ex, "artifact_write", json!({"path": at(&root.join("apps/api/b.txt")), "content": "x", "within": "apps/web"}));
    let beyond = call(&ex, "artifact_read", json!({"path": at(&allowed), "within": "apps/web"}));
    // Then its own file and the allowed directory are served, and the sibling is refused as outside and left as it was
    assert!(own.is_ok() && beyond.is_ok());
    assert_eq!(sibling.unwrap_err().code, Code::PathOutsideRoot);
    assert_eq!(write.unwrap_err().code, Code::PathOutsideRoot);
    assert_eq!(std::fs::read_to_string(root.join("apps/api/b.txt")).unwrap(), "sibling");
}
