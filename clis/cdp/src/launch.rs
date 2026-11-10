//! Starting a Chromium of our own: finding the executable, launching it with a throwaway profile and an ephemeral
//! DevTools port, and reading that port from its stderr.
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub struct Launched {
    pub child: Child,
    /// `http://127.0.0.1:<port>`, what `Attached::attach` takes.
    pub endpoint: String,
    pub profile: PathBuf,
}

fn chrome_in(dir: &Path) -> Option<PathBuf> {
    ["chrome-linux/chrome", "chrome-linux64/chrome", "chrome"].iter().map(|rel| dir.join(rel)).find(|p| p.is_file())
}

/// `WF_CHROMIUM`, else the newest Playwright-installed Chromium (`PLAYWRIGHT_BROWSERS_PATH`, `/opt/pw-browsers`,
/// `~/.cache/ms-playwright`).
pub fn find_chromium() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("WF_CHROMIUM").filter(|p| !p.is_empty()) {
        return Some(p.into());
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("PLAYWRIGHT_BROWSERS_PATH").filter(|p| !p.is_empty() && p != "0") {
        roots.push(p.into());
    }
    roots.push("/opt/pw-browsers".into());
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(Path::new(&home).join(".cache/ms-playwright"));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("chromium-"))).collect();
        dirs.sort();
        if let Some(exe) = dirs.iter().rev().find_map(|d| chrome_in(d)) {
            return Some(exe);
        }
        if let Some(exe) = chrome_in(&root.join("chromium")) {
            return Some(exe);
        }
    }
    None
}

/// Launches `exe` with `args` (plus the profile and debugging port), `env` added to the environment.
pub fn launch(exe: &Path, args: &[String], env: &[(String, String)], headless: bool) -> Result<Launched, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let profile = std::env::temp_dir().join(format!("wf-browser-{}-{nanos}", std::process::id()));
    let mut cmd = Command::new(exe);
    if headless {
        cmd.arg("--headless=new");
    }
    // a sideways touch drag at the start of a scroller is otherwise Chromium's swipe-back gesture: it navigated the device frame away
    cmd.args(["--no-sandbox", "--no-first-run", "--no-default-browser-check", "--remote-debugging-port=0", "--disable-features=OverscrollHistoryNavigation", "--overscroll-history-navigation=0"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(args)
        .arg("about:blank")
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let stderr = child.stderr.take().ok_or("no stderr from the browser")?;
    let mut lines = BufReader::new(stderr).lines();
    let endpoint = loop {
        match lines.next() {
            Some(Ok(line)) => {
                if let Some(rest) = line.split("DevTools listening on ws://").nth(1) {
                    let authority = rest.split('/').next().unwrap_or("");
                    break format!("http://{authority}");
                }
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the browser exited before opening its DevTools port".into());
            }
        }
    };
    // Keep reading so the browser never blocks on a full stderr pipe.
    std::thread::spawn(move || for _ in lines {});
    Ok(Launched { child, endpoint, profile })
}

impl Launched {
    /// Stops the browser and removes its throwaway profile.
    pub fn close(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The profile is a directory this module created under the temp dir.
        if self.profile.starts_with(std::env::temp_dir()) {
            let _ = std::fs::remove_dir_all(&self.profile);
        }
    }
}
