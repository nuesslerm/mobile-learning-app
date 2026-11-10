//! The double of the crate's port for tests: provider CLIs as scripted processes.
use crate::application::ports::{Running, Spawner};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot, Notify};

/// What a faked provider CLI prints and how it ends.
#[derive(Clone, Default)]
pub struct CliScript {
    pub lines: Vec<String>,
    /// `Err` is the stderr tail of a CLI that exited with a failure.
    pub exit: Option<String>,
    /// The process stays up after its lines until it is killed.
    pub hold: bool,
    /// The program cannot be started (such as `not found`).
    pub not_started: Option<String>,
}

/// One start the spawner was asked for.
#[derive(Clone, Debug)]
pub struct Spawned {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// The provider CLIs as scripted processes, keyed by program name; a program with no script prints nothing and exits cleanly.
#[derive(Default)]
pub struct ScriptedSpawner {
    scripts: Mutex<HashMap<String, Vec<CliScript>>>,
    spawned: Mutex<Vec<Spawned>>,
    killed: Arc<AtomicUsize>,
}

impl ScriptedSpawner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// What every start of `program` does.
    pub fn script(&self, program: &str, script: CliScript) {
        self.script_each(program, vec![script]);
    }

    /// What the starts of `program` do, in order: the first start plays the first script, and a start after the last plays the last again.
    pub fn script_each(&self, program: &str, scripts: Vec<CliScript>) {
        self.scripts.lock().unwrap_or_else(|p| p.into_inner()).insert(program.to_string(), scripts);
    }

    pub fn spawned(&self) -> Vec<Spawned> {
        self.spawned.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn kills(&self) -> usize {
        self.killed.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Spawner for ScriptedSpawner {
    async fn spawn(&self, program: &str, args: &[String], cwd: &Path) -> Result<Running, String> {
        let start = {
            let mut spawned = self.spawned.lock().unwrap_or_else(|p| p.into_inner());
            spawned.push(Spawned { program: program.into(), args: args.to_vec(), cwd: cwd.to_path_buf() });
            spawned.iter().filter(|s| s.program == program).count() - 1
        };
        let script = self.scripts.lock().unwrap_or_else(|p| p.into_inner()).get(program).and_then(|all| all.get(start).or(all.last())).cloned().unwrap_or_default();
        if let Some(why) = script.not_started.clone() {
            return Err(why);
        }
        let (tx, rx) = mpsc::channel::<String>(64);
        let (done_tx, done_rx) = oneshot::channel();
        let stop = Arc::new(Notify::new());
        let waiting = Arc::clone(&stop);
        let counter = Arc::clone(&self.killed);
        tokio::spawn(async move {
            for line in script.lines {
                if tx.send(line).await.is_err() {
                    break;
                }
            }
            let killed = if script.hold {
                waiting.notified().await;
                true
            } else {
                false
            };
            drop(tx);
            let _ = done_tx.send(match (&script.exit, killed) {
                (Some(stderr), false) => Err(stderr.clone()),
                _ => Ok(()),
            });
        });
        let kill: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            stop.notify_one();
        });
        Ok(Running {
            lines: Box::pin(futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|line| (line, rx)) })),
            finish: Box::pin(async move { done_rx.await.unwrap_or(Ok(())) }),
            kill,
        })
    }
}
