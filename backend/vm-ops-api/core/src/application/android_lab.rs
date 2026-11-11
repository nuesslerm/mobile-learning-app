//! The device lab a CI job drives without a shell: three fixed verbs on the one redroid in agent-shell. The script of each verb is
//! this file's, never the request's, and a job's status is readable only for a job this lab started, so the CI port gains no way
//! to run anything else or to read another job's output.
use crate::application::agent_shell::{AgentShell, Failed};
use crate::domain::agent_shell::JobStatus;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// More jobs than a run starts: the oldest are forgotten, which only ends a poll of a job nobody still reads.
const REMEMBERED_JOBS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Start { expo_go: bool },
    Stop,
    List,
}

impl Verb {
    /// What the job runs in agent-shell: wf's own lifecycle verb on the local docker, with no operand a request could shape.
    pub fn script(self) -> &'static str {
        match self {
            Verb::Start { expo_go: true } => "exec wf android start-device --local --expo-go\n",
            Verb::Start { expo_go: false } => "exec wf android start-device --local\n",
            Verb::Stop => "exec wf android stop-device --local\n",
            Verb::List => "exec wf android list-devices --local\n",
        }
    }
}

pub struct AndroidLab {
    shell: Arc<AgentShell>,
    jobs: Mutex<Vec<String>>,
}

impl AndroidLab {
    pub fn new(shell: Arc<AgentShell>) -> Self {
        Self { shell, jobs: Mutex::new(Vec::new()) }
    }

    pub async fn start(&self, verb: Verb) -> Result<String, Failed> {
        let job_id = self.shell.start_job(verb.script().as_bytes()).await?;
        let mut jobs = self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        jobs.push(job_id.clone());
        if jobs.len() > REMEMBERED_JOBS {
            jobs.remove(0);
        }
        Ok(job_id)
    }

    pub async fn status(&self, job_id: &str) -> Result<JobStatus, Failed> {
        let known: HashSet<String> = self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().cloned().collect();
        if !known.contains(job_id) {
            return Err(Failed::NotALabJob);
        }
        self.shell.job_status(job_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_each_verb_when_its_script_is_read_then_it_is_one_wf_android_lifecycle_call_on_the_local_docker_and_nothing_a_request_could_add() {
        for verb in [Verb::Start { expo_go: true }, Verb::Start { expo_go: false }, Verb::Stop, Verb::List] {
            let script = verb.script();
            assert!(script.starts_with("exec wf android ") && script.contains(" --local"), "{script}");
            assert_eq!(script.lines().count(), 1, "{script}");
            assert!(!script.contains([';', '&', '|', '$', '`', '<', '>']), "{script}");
        }
    }
}
