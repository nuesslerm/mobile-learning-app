//! The two read-only siblings behind `GET /redeploy/status`: the self-swap replaces this container, so its outcome is only readable
//! from the host's files.

/// A sibling that mounts the compose dir read-only and runs `script` in it.
pub fn read_only_sibling(vm_dir: &str, image: &str, script: &str) -> Vec<String> {
    ["run", "--rm", "-v", &format!("{vm_dir}:{vm_dir}:ro"), "-w", vm_dir, image, "sh", "-c", script].map(String::from).to_vec()
}

/// The last redeploy's log.
pub fn redeploy_status_sibling_args(vm_dir: &str, image: &str, redeploy_log_file: &str) -> Vec<String> {
    read_only_sibling(vm_dir, image, &format!("cat {redeploy_log_file} 2>/dev/null || echo 'no redeploy has run yet'"))
}

/// The history, whole; a missing file reads as empty.
pub fn history_sibling_args(vm_dir: &str, image: &str, history_file: &str) -> Vec<String> {
    read_only_sibling(vm_dir, image, &format!("cat {history_file} 2>/dev/null || true"))
}
