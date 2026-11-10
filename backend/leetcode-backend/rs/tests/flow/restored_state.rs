//! The real `leetcode-backend-rs` binary started on a data directory a backup restored: whatever it finds there it leaves exactly as it was.
use serde_json::json;
use service_kit::blackbox::Running;
use std::collections::BTreeMap;
use std::path::Path;

fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.path().is_file()).map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap())).collect()
}

#[tokio::test]
async fn given_a_restored_data_directory_when_the_service_starts_and_is_asked_then_every_file_in_it_is_byte_identical() {
    // Given a data directory as a restore leaves it: the stored session (in the odd formatting a different writer produced) and a file of the old host's
    let data = tempfile::tempdir().unwrap();
    let session = json!({ "cookie": "LEETCODE_SESSION=abc; csrftoken=xyz", "csrf": "xyz", "verifiedAt": 1_700_000_000u64 }).to_string();
    std::fs::write(data.path().join("session.json"), format!("  {session}\n")).unwrap();
    std::fs::write(data.path().join("old-host-note.txt"), "kept\n").unwrap();
    let before = snapshot(data.path());
    let env = [
        ("GIT_COMMIT", "0123456789abcdef".to_string()),
        ("LEETCODE_BACKEND_SESSION_FILE", data.path().join("session.json").display().to_string()),
        ("LEETCODE_ORIGIN", "http://127.0.0.1:9".to_string()),
        ("LEETCODE_CAPTURE_TOOL_BASE_URL", "http://127.0.0.1:9".to_string()),
        ("LEETCODE_CAPTURE_TRIGGER_SECRET", "trigger".to_string()),
        ("LEETCODE_BACKEND_ADMIN_SECRET", "admin".to_string()),
        ("VM_GATEWAY_AUTH_SEED", "bearer".to_string()),
    ];

    // When the real binary starts on it, answers its health, and is asked who is signed in and for the stored login
    let service = Running::start(env!("CARGO_BIN_EXE_leetcode-backend-rs"), "PORT", &env, "/health").await;
    let (health, _) = service.get("/health").await;
    let login = service.client.get(format!("{}/api/leetcode/login-session", service.base)).header("authorization", "Bearer bearer").send().await.unwrap();
    let user = service.client.get(format!("{}/api/leetcode/user-status", service.base)).header("authorization", "Bearer bearer").send().await.unwrap();
    assert_eq!(health, 200);
    // (LeetCode is unreachable here, so the user status answers 502: the stored session is still only read)
    assert_eq!((login.status().as_u16(), user.status().as_u16()), (200, 502));
    drop(service);

    // Then nothing in the directory was created over, rewritten or removed
    assert_eq!(snapshot(data.path()), before);
}
