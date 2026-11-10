//! The client against an in-process agent API server: the recorded exchanges of the `agent.threads` contract, a wrong bearer, a call that is
//! not answered, a socket that drops in the middle of a call and of a stream, the replay that overlaps what the client has, and a service that
//! is gone. Each test plays the service's side with a script (`fake`), so what the client does is read from what the service saw.
#[path = "flow/calls.rs"]
mod calls;
#[path = "flow/fake.rs"]
mod fake;
#[path = "flow/streams.rs"]
mod streams;

use agent_api_client::{Backoff, Config};
use agent_api_proto::Method;
use fake::{Fake, BEARER};
use std::time::Duration;

/// The method of the `agent.threads` contract with this name.
fn method(name: &str) -> Method {
    *agent_api_agent::METHODS.iter().find(|m| m.name() == name).unwrap_or_else(|| panic!("{name} is no method of the contract"))
}

/// A config for the fake with waits short enough for a test, and a backoff that gives up after `attempts`.
fn config(fake: &Fake, attempts: u32) -> Config {
    let mut config = Config::new(fake.url.clone(), BEARER);
    config.call_timeout = Duration::from_secs(5);
    config.connect_timeout = Duration::from_secs(2);
    config.backoff = Backoff { initial: Duration::from_millis(1), max: Duration::from_millis(4), attempts };
    config
}
