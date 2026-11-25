use std::process::Command;

pub fn run(args: &[String]) -> Option<i32> {
    let root = wf_kit::root::repo_root();
    let source = include_str!("archive.py");
    let result = Command::new("python3")
        .arg("-c")
        .arg(source)
        .arg(include_str!("style.css"))
        .args(args)
        .current_dir(root)
        .status();
    Some(match result {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            println!(
                "{}",
                serde_json::json!({"ok": false, "errors": [error.to_string()]})
            );
            1
        }
    })
}
