use crate::node_script;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};

/// A helper process (an auth server, a proxy, a SOCKS listener) whose first line is `port <n>` (or a bare port) and whose later stdout
/// lines are collected; it is killed on drop.
pub struct Aux {
    child: Child,
    pub port: u16,
    seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Aux {
    pub fn spawn(script: &str) -> Self {
        let mut child = node_script(script).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        let port: u16 = first.trim().trim_start_matches("port ").parse().unwrap_or_else(|e| panic!("the helper prints its port first, not {first:?}: {e}"));
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        std::thread::spawn(move || {
            let mut line = String::new();
            while reader.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
                sink.lock().unwrap().push(line.trim_end().to_string());
                line.clear();
            }
        });
        Self { child, port, seen }
    }

    /// The lines printed after the port.
    pub fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    /// Whether the helper is still running: a failed check against a helper that died says so.
    pub fn running(&mut self) -> bool {
        self.child.try_wait().is_ok_and(|exited| exited.is_none())
    }
}

impl Drop for Aux {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
