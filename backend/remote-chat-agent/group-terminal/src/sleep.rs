//! A workspace's sleep: its shells are closed and what starts them again is kept, so the sleep outlives the service. `terminal.sleep` closes the
//! shells and records them, `terminal.wake` starts each again where it was, with its title and size, and `terminal.sleeping` reads the records.
use crate::group::{refuse, value, Answer, Refusal, TerminalGroup};
use crate::terminal::lock;
use agent_api_terminal::{self as api, SleepResult, SleepingResult, SleepingWorkspace, SleptTerminal, Status, WakeFailure, WakeResult};
use exec_proto::Code;
use exec_pty::PTY_CLOSE;
use serde_json::{json, Map, Value};
use std::sync::Mutex;

/// Where the sleeping workspaces are kept: the adapter keeps them across restarts.
pub trait SleepStore: Send + Sync {
    fn load(&self) -> Result<Vec<SleepingWorkspace>, String>;
    /// Replaces what is kept with `all`, whole or not at all.
    fn save(&self, all: &[SleepingWorkspace]) -> Result<(), String>;
}

/// The store a group has until it is given another: it keeps the sleep for the life of the process.
#[derive(Default)]
pub struct MemorySleep(Mutex<Vec<SleepingWorkspace>>);

impl SleepStore for MemorySleep {
    fn load(&self) -> Result<Vec<SleepingWorkspace>, String> {
        Ok(lock(&self.0).clone())
    }

    fn save(&self, all: &[SleepingWorkspace]) -> Result<(), String> {
        *lock(&self.0) = all.to_vec();
        Ok(())
    }
}

fn unavailable(why: String) -> Refusal {
    refuse(api::code::SLEEP_UNAVAILABLE, why)
}

/// The cwd a shell starts again in: the one it had when it slept, if it still lies in the workspace's place, else the place's own start.
fn restart_cwd(prefix: Option<&str>, kept: &str) -> Option<String> {
    let inside = |root: &str| kept == root || (kept.starts_with(&format!("{}/", root.trim_end_matches('/'))) && !kept.split('/').any(|part| part == ".."));
    match prefix {
        Some(root) if !inside(root) => Some(root.to_string()),
        _ if kept.is_empty() => prefix.map(str::to_string),
        _ => Some(kept.to_string()),
    }
}

impl TerminalGroup {
    pub(crate) async fn sleep(&self, params: &Value) -> Answer {
        let workspace = api::workspace_param(params)?;
        let channel = self.channel(&workspace)?;
        self.adopt(&workspace).await?;
        let mut terms: Vec<_> = lock(&self.terminals).values().filter(|t| t.workspace == workspace).cloned().collect();
        terms.sort_by(|a, b| a.terminal.cmp(&b.terminal));
        let (mut slept, mut failure) = (Vec::new(), None);
        for term in terms {
            let info = term.info();
            match channel.call(PTY_CLOSE, json!({"pty": term.pty})).await {
                Ok(_) => {}
                Err(e) if e.code() == Code::NotFound.as_str() => {}
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
            term.close();
            lock(&self.terminals).remove(&(workspace.clone(), term.terminal.clone()));
            slept.push(SleptTerminal { terminal: info.terminal, title: info.title, cwd: info.cwd, cols: info.cols, rows: info.rows });
        }
        let _held = lock(&self.sleep_lock);
        let mut all = self.sleeps.load().map_err(unavailable)?;
        let already = all.iter().any(|s| s.workspace == workspace);
        if !slept.is_empty() {
            let at = self.clock.now_ms();
            let kept = match all.iter().position(|s| s.workspace == workspace) {
                Some(at) => &mut all[at],
                None => {
                    all.push(SleepingWorkspace { workspace: workspace.clone(), slept_at_ms: at, terminals: Vec::new() });
                    all.last_mut().unwrap_or_else(|| unreachable!("a workspace was just pushed"))
                }
            };
            for shell in &slept {
                kept.terminals.retain(|k| k.terminal != shell.terminal);
                kept.terminals.push(shell.clone());
            }
            kept.terminals.sort_by(|a, b| a.terminal.cmp(&b.terminal));
            self.sleeps.save(&all).map_err(unavailable)?;
        }
        if let Some(e) = failure {
            return Err(e.into());
        }
        value(&SleepResult { workspace, slept, already })
    }

    pub(crate) async fn wake(&self, params: &Value) -> Answer {
        let workspace = api::workspace_param(params)?;
        let record = {
            let _held = lock(&self.sleep_lock);
            self.sleeps.load().map_err(unavailable)?.into_iter().find(|s| s.workspace == workspace)
        };
        let Some(record) = record else { return value(&WakeResult { workspace, woken: Vec::new(), failed: Vec::new() }) };
        let channel = self.channel(&workspace)?;
        self.adopt(&workspace).await?;
        let prefix = self.executors.place(&workspace).prefix;
        let (mut woken, mut failed, mut done) = (Vec::new(), Vec::new(), Vec::new());
        for kept in &record.terminals {
            let session = api::Session { workspace: workspace.clone(), terminal: kept.terminal.clone() };
            if let Some(existing) = self.known(&session) {
                if existing.state().status == Status::Running {
                    done.push(kept.terminal.clone());
                    continue;
                }
                let _ = channel.call(PTY_CLOSE, json!({"pty": existing.pty})).await;
                existing.close();
                lock(&self.terminals).remove(&(workspace.clone(), kept.terminal.clone()));
            }
            let cwd = restart_cwd(prefix.as_deref(), &kept.cwd);
            match self.spawn(&session, cwd, Some(kept.cols), Some(kept.rows), &Map::new(), Some(kept.title.clone())).await {
                Ok(term) => {
                    done.push(kept.terminal.clone());
                    woken.push(term.info());
                }
                Err(Refusal(code, message)) => failed.push(WakeFailure { terminal: kept.terminal.clone(), code, message }),
            }
        }
        let _held = lock(&self.sleep_lock);
        let mut all = self.sleeps.load().map_err(unavailable)?;
        if let Some(at) = all.iter().position(|s| s.workspace == workspace) {
            all[at].terminals.retain(|k| !done.contains(&k.terminal));
            if all[at].terminals.is_empty() {
                all.remove(at);
            }
            self.sleeps.save(&all).map_err(unavailable)?;
        }
        value(&WakeResult { workspace, woken, failed })
    }

    pub(crate) fn sleeping(&self, params: &Value) -> Answer {
        let only = api::optional_workspace(params)?;
        let _held = lock(&self.sleep_lock);
        let mut sleeping = self.sleeps.load().map_err(unavailable)?;
        sleeping.retain(|s| only.as_ref().is_none_or(|w| *w == s.workspace));
        sleeping.sort_by(|a, b| a.workspace.cmp(&b.workspace));
        value(&SleepingResult { sleeping })
    }
}

#[cfg(test)]
mod unit {
    use super::restart_cwd;

    #[test]
    fn given_a_kept_cwd_when_a_shell_starts_again_then_it_stays_inside_the_place_and_falls_back_to_the_place_itself() {
        // Given a worktree place and the cwds a shell could have slept in
        let root = Some("/base/wt");
        // Then one inside is kept, one outside, with a dotdot or empty falls back to the place's own start
        assert_eq!(restart_cwd(root, "/base/wt/src").as_deref(), Some("/base/wt/src"));
        assert_eq!(restart_cwd(root, "/base/wt").as_deref(), Some("/base/wt"));
        assert_eq!(restart_cwd(root, "/base/other").as_deref(), Some("/base/wt"));
        assert_eq!(restart_cwd(root, "/base/wt/../x").as_deref(), Some("/base/wt"));
        assert_eq!(restart_cwd(root, "").as_deref(), Some("/base/wt"));
        // And the root place leaves a cwd to the executor's confinement, with none when nothing was kept
        assert_eq!(restart_cwd(None, "/repo/src").as_deref(), Some("/repo/src"));
        assert_eq!(restart_cwd(None, ""), None);
    }
}
