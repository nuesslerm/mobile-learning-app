//! The drills: a stop or a freeze of this service's own container, taken by a detached sibling that outlives it, so the watchdog's
//! recovery is shown to work.

/// Seconds the sibling waits after the 202, so the reply is out before the container goes.
pub const DELAY_SEC: u32 = 5;
/// Seconds a frozen container stays frozen: the sibling thaws it itself, so the thaw needs neither wf, nor 8080, nor a sandbox.
pub const FREEZE_SEC: u32 = 600;

/// The container that answers on 8080, named as compose and the watchdog's `watchdog.recreated` line name it.
pub const OWN_CONTAINER: &str = "vm-ops-api-ops-api-1";
const DOCKER_SOCKET: &str = "/var/run/docker.sock:/var/run/docker.sock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Stop,
    Freeze,
}

impl Action {
    /// `stop` or `freeze`, exactly; anything else, case included, is no drill.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "stop" => Some(Action::Stop),
            "freeze" => Some(Action::Freeze),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Action::Stop => "stop",
            Action::Freeze => "freeze",
        }
    }
}

/// `docker run` of the sibling: detached, removed when done, on this service's own image, with the host's Docker socket.
pub fn sibling_args(image: &str, action: Action, container: &str) -> Vec<String> {
    let script = match action {
        Action::Stop => format!("sleep {DELAY_SEC}; docker stop {container}"),
        Action::Freeze => format!("sleep {DELAY_SEC}; docker kill -s STOP {container}; sleep {FREEZE_SEC}; docker kill -s CONT {container} || true"),
    };
    ["run", "--pull", "missing", "--rm", "-d", "-v", DOCKER_SOCKET, image, "sh", "-c", &script].map(str::to_string).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_two_actions_when_parsed_then_only_their_exact_names_are_drills() {
        assert_eq!([Action::parse("stop"), Action::parse("freeze")], [Some(Action::Stop), Some(Action::Freeze)]);
        for other in ["", "STOP", "reboot", " stop", "stop "] {
            assert_eq!(Action::parse(other), None, "{other:?}");
        }
    }

    #[test]
    fn given_an_action_when_the_sibling_is_rendered_then_it_is_the_argv_node_spawns() {
        let stop = sibling_args("ns/vm-ops-api-rs:sha-1234567", Action::Stop, "vm-ops-api-ops-api-1");
        assert_eq!(
            stop,
            ["run", "--pull", "missing", "--rm", "-d", "-v", "/var/run/docker.sock:/var/run/docker.sock", "ns/vm-ops-api-rs:sha-1234567", "sh", "-c", "sleep 5; docker stop vm-ops-api-ops-api-1"]
        );
        let freeze = sibling_args("ns/vm-ops-api-rs:sha-1234567", Action::Freeze, OWN_CONTAINER);
        assert_eq!(freeze.last().map(String::as_str), Some("sleep 5; docker kill -s STOP vm-ops-api-ops-api-1; sleep 600; docker kill -s CONT vm-ops-api-ops-api-1 || true"));
    }
}
