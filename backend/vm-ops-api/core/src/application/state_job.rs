//! `POST /state/{verb}` and `GET /state/jobs/{job}`: CI starts the fixed backup or verify script on agent-shell and reads how it went. A hosted
//! runner cannot reach the admin port's exec route, and handing it that route would hand it every script. Only the jobs started here can be read.
use crate::application::agent_shell::{AgentShell, Failed};
use crate::domain::agent_shell::JobStatus;
use crate::domain::state_job::{is_verb, script};
use std::sync::{Arc, Mutex};

/// The newest jobs remembered: a CI run starts one or two.
const KEEP: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    BadVerb,
    /// The job id is not one this service started.
    UnknownJob,
    Failed(Failed),
}

pub struct StateJobs {
    agent_shell: Arc<AgentShell>,
    started: Mutex<Vec<String>>,
}

impl StateJobs {
    pub fn new(agent_shell: Arc<AgentShell>) -> Self {
        Self { agent_shell, started: Mutex::new(Vec::new()) }
    }

    pub async fn start(&self, verb: &str) -> Result<String, Refused> {
        if !is_verb(verb) {
            return Err(Refused::BadVerb);
        }
        let job = self.agent_shell.start_job(script(verb).as_bytes()).await.map_err(Refused::Failed)?;
        let mut started = self.started.lock().expect("the started jobs lock");
        started.push(job.clone());
        if started.len() > KEEP {
            started.remove(0);
        }
        Ok(job)
    }

    pub async fn status(&self, job: &str) -> Result<JobStatus, Refused> {
        if !self.started.lock().expect("the started jobs lock").iter().any(|id| id == job) {
            return Err(Refused::UnknownJob);
        }
        self.agent_shell.job_status(job).await.map_err(Refused::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::ExecResult;
    use crate::domain::containers::DockerContainer;
    use crate::testing::{ContainersCall, FakeClock, FakeContainers, FakeEventLog, FakeIds};

    fn jobs(containers: FakeContainers) -> (StateJobs, Arc<FakeContainers>) {
        let running = DockerContainer { id: "as1".into(), names: Some(vec!["/agent-shell".into()]), state: Some("running".into()), ..Default::default() };
        let containers = Arc::new(containers.with_app_containers("agent-shell", vec![running]));
        let shell = AgentShell::new(containers.clone(), Arc::new(FakeIds::default()), Arc::new(FakeClock::at(0)), Arc::new(FakeEventLog::default()));
        (StateJobs::new(Arc::new(shell)), containers)
    }

    #[tokio::test]
    async fn given_a_verb_that_is_no_state_job_when_started_then_nothing_reaches_agent_shell() {
        let (jobs, containers) = jobs(FakeContainers::new());

        assert_eq!(jobs.start("restore").await, Err(Refused::BadVerb));
        assert_eq!(jobs.start("backup;id").await, Err(Refused::BadVerb));
        assert_eq!(containers.calls(), []);
    }

    #[tokio::test]
    async fn given_a_started_backup_when_its_job_and_another_are_read_then_only_the_started_one_is_answered() {
        let (jobs, containers) = jobs(FakeContainers::new().with_exec(&[], vec![Ok(ExecResult::attached(Some(0), String::new()))]));

        let job = jobs.start("backup").await.expect("started");
        let put = containers.calls().into_iter().find_map(|call| if let ContainersCall::PutFile { tar, .. } = call { Some(tar) } else { None }).expect("the script was landed");
        let landed = String::from_utf8_lossy(&put).to_string();

        assert!(landed.contains("vm state backup --local --apps-file"), "{landed}");
        assert!(jobs.status(&job).await.is_ok());
        assert_eq!(jobs.status("0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab").await, Err(Refused::UnknownJob));
    }
}
