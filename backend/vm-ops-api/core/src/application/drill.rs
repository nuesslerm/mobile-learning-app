//! `POST /drill`: takes this service's own container down from a detached sibling, so a drill can target the very host that runs it.
use crate::application::ports::Siblings;
use crate::domain::drill::{sibling_args, Action};
use std::sync::Arc;

/// What the 202 says: the drill and the container its sibling takes down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub action: Action,
    pub container: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// No `action`, or one that is neither `stop` nor `freeze`: nothing was started.
    UnknownAction,
    /// The sibling could not be started; the message is why.
    NotStarted(String),
}

pub struct Drill {
    siblings: Arc<dyn Siblings>,
    image: String,
    container: String,
}

impl Drill {
    /// `image` is the image this service runs (its siblings run it too); `container` is this service's own.
    pub fn new(siblings: Arc<dyn Siblings>, image: String, container: String) -> Self {
        Self { siblings, image, container }
    }

    pub fn start(&self, action: Option<&str>) -> Result<Accepted, Refused> {
        let action = action.and_then(Action::parse).ok_or(Refused::UnknownAction)?;
        self.siblings.spawn_detached(&sibling_args(&self.image, action, &self.container)).map_err(|error| Refused::NotStarted(error.to_string()))?;
        Ok(Accepted { action, container: self.container.clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeSiblings, SiblingCall};

    fn drill(siblings: &Arc<FakeSiblings>) -> Drill {
        Drill::new(siblings.clone(), "ns/vm-ops-api-rs:sha-1234567".into(), "vm-ops-api-ops-api-1".into())
    }

    #[test]
    fn given_a_drill_when_each_action_starts_then_one_detached_sibling_takes_the_service_down_and_the_reply_names_it() {
        let siblings = Arc::new(FakeSiblings::new());

        let stop = drill(&siblings).start(Some("stop"));
        let freeze = drill(&siblings).start(Some("freeze"));

        let named = |action| Ok(Accepted { action, container: "vm-ops-api-ops-api-1".into() });
        assert_eq!((stop, freeze), (named(Action::Stop), named(Action::Freeze)));
        let scripts: Vec<String> = siblings.calls().iter().map(|call| match call {
            SiblingCall::Detached(args) => args.last().cloned().unwrap_or_default(),
            SiblingCall::Run(args) => panic!("a drill waited for {args:?}"),
        }).collect();
        assert_eq!(scripts[0], "sleep 5; docker stop vm-ops-api-ops-api-1");
        assert!(scripts[1].starts_with("sleep 5; docker kill -s STOP vm-ops-api-ops-api-1; sleep 600; docker kill -s CONT"), "{}", scripts[1]);
    }

    #[test]
    fn given_no_action_or_another_when_started_then_nothing_runs_and_a_sibling_that_cannot_start_is_refused() {
        let siblings = Arc::new(FakeSiblings::new());
        for action in [None, Some(""), Some("reboot"), Some("STOP")] {
            assert_eq!(drill(&siblings).start(action), Err(Refused::UnknownAction), "{action:?}");
        }
        assert_eq!(siblings.calls(), []);

        let broken = Arc::new(FakeSiblings::new().with_spawn_error("spawn docker ENOENT"));
        assert_eq!(drill(&broken).start(Some("stop")), Err(Refused::NotStarted("spawn docker ENOENT".into())));
    }
}
