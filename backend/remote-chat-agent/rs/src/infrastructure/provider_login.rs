//! The real `ProviderLogin`: the CLI's login command started with that provider's environment alone, in its own directory, so what the CLI
//! writes lands there. Its output (stdout and stderr, where some CLIs print the prompt) is read for the address and code, and a pasted code is
//! written to its input. Written from the CLIs' documentation: not run against the real CLIs.
use remote_chat_agent_host::{provider_command, supervise};
use async_trait::async_trait;
use rca_cli_driver::domain::{login_args, Driver, LoginMethod};
use rca_group_accounts::{LoginRun, ProviderLogin};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

pub struct CliLogin {
    root: String,
}

impl CliLogin {
    pub fn at(root: impl Into<String>) -> CliLogin {
        CliLogin { root: root.into() }
    }
}

#[async_trait]
impl ProviderLogin for CliLogin {
    async fn start(&self, driver: Driver, method: LoginMethod) -> Result<LoginRun, String> {
        let args = login_args(driver, method).ok_or_else(|| format!("{} has no {} login", driver.id(), method.name()))?;
        let cwd = PathBuf::from(&self.root).join(driver.program());
        let mut command = provider_command(&self.root, driver.program(), &args, &cwd);
        let child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).spawn().map_err(|e| e.kind().to_string())?;
        let supervised = supervise(child, true)?;
        let input = supervised.input;
        Ok(LoginRun {
            lines: supervised.running.lines,
            finish: supervised.running.finish,
            input: Arc::new(move |line| input.as_ref().is_some_and(|tx| tx.send(line).is_ok())),
            kill: supervised.running.kill,
        })
    }
}
