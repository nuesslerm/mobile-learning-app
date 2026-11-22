//! The model gateway as a turn sees it: a turn that the service routes through it is launched with a gateway address and a token of its own,
//! and releases the token when it ends. The gateway itself is another group's; the composition root implements this port over it.
use rca_cli_driver::domain::GatewayRoute;

pub trait TurnGateway: Send + Sync {
    /// The route of a turn of `driver` on `thread` that starts now, minting its token; none when the turn is not routed (no key is held for its
    /// provider, so the CLI keeps its own login).
    fn route(&self, thread: &str, driver: &str, model: Option<&str>) -> Option<GatewayRoute>;
    /// The turn is over: its token opens nothing any more.
    fn release(&self, thread: &str);
}

/// No gateway: every CLI runs with its own credentials.
pub struct NoGateway;

impl TurnGateway for NoGateway {
    fn route(&self, _: &str, _: &str, _: Option<&str>) -> Option<GatewayRoute> {
        None
    }

    fn release(&self, _: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Fixed;
    use crate::runner::{McpEndpoint, Runner};
    use crate::testing::MemoryThreads;
    use rca_cli_driver::domain::{Driver, TurnRequest};
    use rca_cli_driver::testing::ScriptedSpawner;
    use rca_exec_channel::testing::ScriptedChannel;
    use rca_exec_channel::Registry;
    use std::sync::{Arc, Mutex};

    struct Mcp;

    impl McpEndpoint for Mcp {
        fn url(&self, thread: &str) -> String {
            format!("http://127.0.0.1:1/mcp/{thread}")
        }
    }

    /// Mints `rcag_<n>` for every route it is asked for, and notes the releases.
    #[derive(Default)]
    struct Counting {
        minted: Mutex<u32>,
        released: Mutex<Vec<String>>,
    }

    impl TurnGateway for Counting {
        fn route(&self, _: &str, driver: &str, _: Option<&str>) -> Option<GatewayRoute> {
            if driver != "opencode" {
                return None;
            }
            let mut minted = self.minted.lock().unwrap();
            *minted += 1;
            Some(GatewayRoute { provider: "opencode-go".into(), base_url: "http://svc/gateway/opencode-go".into(), token: format!("rcag_{minted}") })
        }

        fn release(&self, thread: &str) {
            self.released.lock().unwrap().push(thread.into());
        }
    }

    fn request(driver: Driver) -> TurnRequest {
        TurnRequest { thread_id: "t".into(), run_id: "r".into(), instance_id: driver.id().into(), model: None, cwd: ".".into(), prompt: "go".into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: None, tools: None }
    }

    #[test]
    fn given_a_routed_turn_when_its_cli_is_started_again_then_the_turn_keeps_its_token_and_the_next_turn_mints_another_after_the_release() {
        // Given a runner whose gateway routes opencode turns
        let gateway = Arc::new(Counting::default());
        let runner = Runner::with_gateway(Arc::new(MemoryThreads::default()), Arc::new(Registry::default()), Arc::new(Fixed(ScriptedSpawner::new())), Arc::new(Mcp), gateway.clone());
        let line = |command: rca_cli_driver::domain::Command| format!("{} {}", command.program, command.args.join(" "));
        // When the turn's CLI is started, and started again by a steer in the same turn
        let first = line(runner.host_command(Driver::Opencode, &request(Driver::Opencode), "t").unwrap());
        let steered = line(runner.host_command(Driver::Opencode, &request(Driver::Opencode), "t").unwrap());
        // Then both are given the one token the turn started with
        assert!(first.contains("rcag_1") && steered.contains("rcag_1"), "{first} | {steered}");
        assert_eq!(*gateway.minted.lock().unwrap(), 1);
        // And when the turn is over, its token is released and the thread's next turn is given a fresh one
        runner.release_requests("t");
        assert_eq!(*gateway.released.lock().unwrap(), ["t"]);
        assert!(line(runner.host_command(Driver::Opencode, &request(Driver::Opencode), "t").unwrap()).contains("rcag_2"));
        // And a CLI that lives has no command to route at all, so no token is minted for it
        assert_eq!((runner.host_command(Driver::Codex, &request(Driver::Codex), "u"), runner.host_command(Driver::ClaudeAgent, &request(Driver::ClaudeAgent), "u")), (None, None));
        assert_eq!(*gateway.minted.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn given_a_launch_whose_log_refuses_the_first_event_when_the_thread_next_starts_a_turn_then_the_first_route_was_released_and_a_fresh_token_is_minted() {
        // Given a routed opencode turn on a thread the log does not know, so that recording its start fails
        let gateway = Arc::new(Counting::default());
        let executors = Arc::new(Registry::default());
        executors.open("w1", ScriptedChannel::new(|_, _| Ok(rca_exec_channel::remote::ActionOutput { result: serde_json::json!({}), output: String::new() })) as Arc<dyn rca_exec_channel::remote::ExecutorChannel>).unwrap();
        let runner = Runner::with_gateway(Arc::new(MemoryThreads::default()), executors, Arc::new(Fixed(ScriptedSpawner::new())), Arc::new(Mcp), gateway.clone());
        let input = |driver: &str| crate::runner::TurnInput { workspace: "w1".into(), driver: driver.into(), model: None, prompt: "go".into(), runtime_mode: None, interaction_mode: None };
        // When the launch fails, and the thread's next turn is for another driver
        let failed = runner.prepare("t", &crate::runner::Known::default(), None, input("opencode")).unwrap().launch().await;
        assert!(failed.is_err());
        // Then the failed turn's token was released and the next turn is given a fresh one
        assert_eq!(*gateway.released.lock().unwrap(), ["t"]);
        let next = runner.host_command(Driver::Opencode, &request(Driver::Opencode), "t").unwrap();
        assert!(next.args.join(" ").contains("rcag_2"));
    }
}
