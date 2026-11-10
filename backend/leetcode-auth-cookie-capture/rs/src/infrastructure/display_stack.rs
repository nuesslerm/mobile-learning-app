use async_trait::async_trait;
use capture_core::application::ports::{DisplayStack, DisplayStacks, PortError};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const DISPLAY: &str = ":99";
const VNC_PORT: u16 = 5900;
const SCREEN: &str = "1280x800x24";
/// What each process gets to create its socket before the next one attaches to it.
const SETTLE: Duration = Duration::from_millis(500);
const STOP_WAIT: Duration = Duration::from_secs(3);

/// Xvfb (the virtual display), x11vnc (a VNC server on it) and websockify (the noVNC page, proxying to the VNC server), as child
/// processes. Linux and Docker only; never started in local mode.
pub struct XvfbStacks {
    dir: Option<PathBuf>,
}

impl XvfbStacks {
    /// The three programs as `PATH` finds them (the image installs them).
    pub fn on_path() -> Self {
        Self { dir: None }
    }

    /// The three programs from `dir`.
    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: Some(dir.into()) }
    }

    fn spawn(&self, program: &'static str, args: &[String]) -> Result<Child, PortError> {
        let path = self.dir.as_ref().map_or_else(|| PathBuf::from(program), |dir| dir.join(program));
        Command::new(path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PortError::new(format!("Failed to start {program}: {e}")))
    }
}

struct Stack {
    /// In start order; stopped in reverse.
    children: Mutex<Vec<(&'static str, Child)>>,
}

#[async_trait]
impl DisplayStacks for XvfbStacks {
    async fn start(&self, novnc_port: u16) -> Result<Box<dyn DisplayStack>, PortError> {
        let stack = Stack { children: Mutex::new(Vec::new()) };
        let steps: [(&'static str, Vec<String>); 3] = [
            ("Xvfb", vec![DISPLAY.into(), "-screen".into(), "0".into(), SCREEN.into()]),
            ("x11vnc", ["-display", DISPLAY, "-rfbport", &VNC_PORT.to_string(), "-forever", "-shared", "-nopw"].map(String::from).to_vec()),
            ("websockify", vec!["--web=/usr/share/novnc".into(), novnc_port.to_string(), format!("localhost:{VNC_PORT}")]),
        ];
        for (program, args) in steps {
            match self.spawn(program, &args) {
                Ok(child) => stack.children.lock().await.push((program, child)),
                Err(error) => {
                    stack.teardown().await;
                    return Err(error);
                }
            }
            tokio::time::sleep(SETTLE).await;
        }
        Ok(Box::new(stack))
    }
}

#[async_trait]
impl DisplayStack for Stack {
    fn display(&self) -> &str {
        DISPLAY
    }

    async fn teardown(&self) {
        let mut children = self.children.lock().await;
        while let Some((program, mut child)) = children.pop() {
            if let Some(pid) = child.id().and_then(|id| i32::try_from(id).ok()) {
                let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
            }
            if tokio::time::timeout(STOP_WAIT, child.wait()).await.is_err() {
                tracing::warn!(program, "display.process.did_not_stop");
                let _ = child.kill().await;
            }
        }
    }
}
