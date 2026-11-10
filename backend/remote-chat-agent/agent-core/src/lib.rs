//! The rca agent API (`/agent/v1`, `docs/plans/rca-client/architecture.md`) without its groups: one JSON request/response socket that every
//! app mapper (`t3 serve`, an Orca mapper) speaks. A capability group implements `Group` (its methods, and how it answers them), and the
//! composition root registers the groups on an `AgentApi`, which finds the group that serves a request's method and has it answer. This
//! crate names no group, so adding one changes no line of it. `service.status` lists a group only when every method of it is served
//! (`pending` is empty); the service group's own answer is built from that listing.
use agent_api_proto::{code, GroupName, Method, Request, Response};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub mod bearers;
pub mod connection;

pub use bearers::{bearer_token, token_digest, Bearer, DeviceBearers, Minted};
pub use connection::{Connection, EventSink, Following, Job};

/// What a method needs of the service beyond the executor actions it names. It decides whether the MCP tools count as used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uses {
    /// The service's own state and engine log: it runs no process.
    Service,
    /// An agent CLI: the method starts or signals a turn, and the model of that turn calls the MCP tools (`rca-exec-channel`'s `TOOLS`), so
    /// the executor actions that only a tool reaches are used for as long as some method has this.
    Cli,
}

/// A method a group answers, the executor actions it calls to do so, in order, and what else it uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Served {
    pub method: Method,
    pub actions: &'static [&'static str],
    pub uses: Uses,
}

impl Served {
    /// A method the service answers itself, calling `actions` at the executor.
    pub const fn service(method: Method, actions: &'static [&'static str]) -> Served {
        Served { method, actions, uses: Uses::Service }
    }

    /// A method that starts or signals a CLI turn, calling `actions` at the executor itself.
    pub const fn cli(method: Method, actions: &'static [&'static str]) -> Served {
        Served { method, actions, uses: Uses::Cli }
    }
}

/// The build that answers, for `service.status`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Build<'a> {
    pub version: &'a str,
    pub commit: &'a str,
}

/// One request handed to the group that serves it.
pub struct Call<'a> {
    pub id: Value,
    pub method: Method,
    pub params: &'a Value,
    pub build: Build<'a>,
    /// The groups `service.status` lists: the registered ones that serve all of their methods, in registration order.
    pub listed: &'a [GroupName],
    /// The socket the request came on: where a method that opens a stream sends its events (`Connection::follow`).
    pub connection: &'a Connection,
}

#[async_trait]
pub trait Group: Send + Sync {
    fn name(&self) -> GroupName;

    /// Every method the group answers.
    fn methods(&self) -> &[Served];

    /// The methods of the group's owner set that are not answered yet, by name. A group with any is registered and answers its methods, but
    /// `service.status` does not list it.
    fn pending(&self) -> &[&'static str] {
        &[]
    }

    /// The answer to a request for one of `methods()`: a result, or an error with the group's own code.
    async fn handle(&self, call: Call<'_>) -> Response;
}

/// The groups of one service, as its composition root registered them.
pub struct AgentApi {
    groups: Vec<Arc<dyn Group>>,
    listed: Vec<GroupName>,
}

impl AgentApi {
    /// Registers `groups` in order. Refused: two groups of one name, a group that answers nothing, and a group that answers a method of
    /// another group (which would let two groups answer one name).
    pub fn new(groups: Vec<Arc<dyn Group>>) -> Result<AgentApi, String> {
        for (at, group) in groups.iter().enumerate() {
            let name = group.name();
            if groups[..at].iter().any(|earlier| earlier.name() == name) {
                return Err(format!("group {} is registered twice", name.as_str()));
            }
            if group.methods().is_empty() {
                return Err(format!("group {} answers no method", name.as_str()));
            }
            if let Some(foreign) = group.methods().iter().find(|served| served.method.group() != name) {
                return Err(format!("group {} cannot answer {}, a method of group {}", name.as_str(), foreign.method.name(), foreign.method.group().as_str()));
            }
        }
        let listed = groups.iter().filter(|group| group.pending().is_empty()).map(|group| group.name()).collect();
        Ok(AgentApi { groups, listed })
    }

    /// The groups `service.status` lists, in registration order.
    pub fn listed(&self) -> &[GroupName] {
        &self.listed
    }

    /// Every method answered, with the executor actions it calls, in registration order.
    pub fn served(&self) -> impl Iterator<Item = &Served> {
        self.groups.iter().flat_map(|group| group.methods())
    }

    /// Whether some registered method starts a CLI turn: only then can a model reach the MCP tools.
    pub fn starts_cli(&self) -> bool {
        self.served().any(|served| served.uses == Uses::Cli)
    }

    /// The registered groups.
    pub fn groups(&self) -> &[Arc<dyn Group>] {
        &self.groups
    }

    /// The response frame for one request text: `{id, result}` or `{id, error: {code, message}}`. A method that opens a stream finds it refused:
    /// the caller reads answers only.
    pub async fn dispatch_text(&self, build: Build<'_>, text: &str) -> Value {
        self.dispatch_on(build, text, &Connection::answers_only()).await
    }

    /// `dispatch_text` for a request that came on `connection`: the streams it opens are left on the connection for the transport to run.
    pub async fn dispatch_on(&self, build: Build<'_>, text: &str, connection: &Connection) -> Value {
        match Request::parse(text) {
            Ok(request) => self.dispatch(build, request, connection).await.to_value(),
            Err(refusal) => refusal.to_value(),
        }
    }

    async fn dispatch(&self, build: Build<'_>, request: Request, connection: &Connection) -> Response {
        let Request { id, method, params } = request;
        let found = self.groups.iter().find_map(|group| group.methods().iter().find(|served| served.method.name() == method).map(|served| (group, served.method)));
        let Some((group, method)) = found else { return Response::error(id, code::UNKNOWN_METHOD, format!("{method} is not a method of this service")) };
        group.handle(Call { id, method, params: &params, build, listed: &self.listed, connection }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ALPHA: GroupName = GroupName::new("alpha");
    const BETA: GroupName = GroupName::new("beta");
    const PING: Method = Method::new(ALPHA, "alpha.ping");
    const PONG: Method = Method::new(ALPHA, "alpha.pong");
    const SEND: Method = Method::new(BETA, "beta.send");

    /// A group no contract crate defines: it answers each of its methods with who answered and what it was told.
    struct Demo {
        name: GroupName,
        served: Vec<Served>,
        pending: &'static [&'static str],
    }

    impl Demo {
        fn of(name: GroupName, methods: &[Method]) -> Demo {
            Demo { name, served: methods.iter().map(|m| Served::service(*m, &[])).collect(), pending: &[] }
        }
    }

    #[async_trait]
    impl Group for Demo {
        fn name(&self) -> GroupName {
            self.name
        }
        fn methods(&self) -> &[Served] {
            &self.served
        }
        fn pending(&self) -> &[&'static str] {
            self.pending
        }
        async fn handle(&self, call: Call<'_>) -> Response {
            Response::result(call.id, json!({"by": self.name.as_str(), "method": call.method.name(), "params": call.params, "version": call.build.version, "commit": call.build.commit, "listed": call.listed.iter().map(|g| g.as_str()).collect::<Vec<_>>()}))
        }
    }

    const BUILD: Build<'static> = Build { version: "1.2.3", commit: "abc" };

    fn api(groups: Vec<Demo>) -> AgentApi {
        AgentApi::new(groups.into_iter().map(|g| Arc::new(g) as Arc<dyn Group>).collect()).unwrap()
    }

    #[tokio::test]
    async fn given_two_groups_when_a_request_names_a_method_of_the_second_then_that_group_answers_with_the_frames_id_params_and_the_build() {
        // Given a service whose root registered two groups
        let api = api(vec![Demo::of(ALPHA, &[PING, PONG]), Demo::of(BETA, &[SEND])]);
        // When a request for a method of the second arrives, with a string id and params
        let answered = api.dispatch_text(BUILD, r#"{"id": "q1", "method": "beta.send", "params": {"to": "x"}}"#).await;
        // Then the second group answered it, with the id echoed, the params as sent, the build, and the groups the service lists
        assert_eq!(answered, json!({"id": "q1", "result": {"by": "beta", "method": "beta.send", "params": {"to": "x"}, "version": "1.2.3", "commit": "abc", "listed": ["alpha", "beta"]}}));
        // And a frame without params gives the group an empty object
        let bare = api.dispatch_text(BUILD, r#"{"id": 2, "method": "alpha.pong"}"#).await;
        assert_eq!((bare["id"].clone(), bare["result"]["by"].clone(), bare["result"]["params"].clone()), (json!(2), json!("alpha"), json!({})));
    }

    #[tokio::test]
    async fn given_bad_frames_when_handled_then_each_gets_its_error_code_and_the_id_it_came_with() {
        let api = api(vec![Demo::of(ALPHA, &[PING])]);
        let error = |frame: &'static str| {
            let api = &api;
            async move { api.dispatch_text(BUILD, frame).await }
        };
        // Given/When a frame that is not JSON, one with no id, one with no method, and a method no group answers
        let (garbage, no_id, no_method, unknown) = (error("not json").await, error(r#"{"method": "alpha.ping"}"#).await, error(r#"{"id": 1}"#).await, error(r#"{"id": 9, "method": "beta.send"}"#).await);
        // Then the first three are INVALID_REQUEST, the last UNKNOWN_METHOD naming the method and echoing the id
        for refused in [&garbage, &no_id, &no_method] {
            assert_eq!(refused["error"]["code"], json!("INVALID_REQUEST"));
        }
        assert_eq!(unknown, json!({"id": 9, "error": {"code": "UNKNOWN_METHOD", "message": "beta.send is not a method of this service"}}));
    }

    #[test]
    fn given_groups_with_and_without_methods_still_to_serve_when_registered_then_only_the_complete_ones_are_listed_in_registration_order() {
        // Given three groups: a complete one, one whose owner set has methods still to serve, and another complete one
        let gamma = GroupName::new("gamma");
        let ping = Method::new(gamma, "gamma.ping");
        let partial = Demo { pending: &["beta.later"], ..Demo::of(BETA, &[SEND]) };
        // When they are registered
        let api = api(vec![Demo::of(ALPHA, &[PING]), partial, Demo::of(gamma, &[ping])]);
        // Then the partial one is registered and answers, but service.status does not list it
        assert_eq!(api.listed(), [ALPHA, gamma]);
        assert_eq!(api.groups().len(), 3);
        assert_eq!(api.served().map(|s| s.method.name()).collect::<Vec<_>>(), ["alpha.ping", "beta.send", "gamma.ping"]);
    }

    #[tokio::test]
    async fn given_a_group_that_serves_the_rest_of_its_set_when_it_is_registered_then_it_is_listed_and_a_partial_one_still_answers() {
        // Given the same group registered once with a method pending and once complete
        let pending = Demo { pending: &["beta.rest"], ..Demo::of(BETA, &[SEND]) };
        let before = api(vec![pending]);
        let after = api(vec![Demo::of(BETA, &[SEND])]);
        // When each is asked for the method it serves
        let (asked_before, asked_after) = (before.dispatch_text(BUILD, r#"{"id": 1, "method": "beta.send"}"#).await, after.dispatch_text(BUILD, r#"{"id": 1, "method": "beta.send"}"#).await);
        // Then both answer, and only the complete one lists its group
        assert_eq!((asked_before["result"]["listed"].clone(), asked_after["result"]["listed"].clone()), (json!([]), json!(["beta"])));
    }

    #[test]
    fn given_registered_groups_when_asked_whether_a_method_starts_a_cli_then_only_a_group_that_declares_one_does() {
        // Given a group of methods the service answers itself and a group with a method that starts a CLI turn
        struct Starter(Vec<Served>);
        #[async_trait]
        impl Group for Starter {
            fn name(&self) -> GroupName {
                BETA
            }
            fn methods(&self) -> &[Served] {
                &self.0
            }
            async fn handle(&self, call: Call<'_>) -> Response {
                Response::result(call.id, json!({}))
            }
        }
        let alone = api(vec![Demo::of(ALPHA, &[PING])]);
        let with_cli = AgentApi::new(vec![Arc::new(Demo::of(ALPHA, &[PING])) as Arc<dyn Group>, Arc::new(Starter(vec![Served::cli(SEND, &[])]))]).unwrap();
        // When each is asked, Then only the second starts a CLI, and the row says what it uses
        assert_eq!((alone.starts_cli(), with_cli.starts_cli()), (false, true));
        assert_eq!(with_cli.served().map(|s| s.uses).collect::<Vec<_>>(), [Uses::Service, Uses::Cli]);
        assert!(!AgentApi::new(vec![]).unwrap().starts_cli());
    }

    #[tokio::test]
    async fn given_a_method_that_opens_a_stream_when_dispatched_on_a_connection_then_the_stream_is_left_on_it_and_a_caller_that_reads_answers_only_leaves_none_to_run() {
        // Given a group whose method opens the stream of a topic
        struct Opener;
        #[async_trait]
        impl Group for Opener {
            fn name(&self) -> GroupName {
                ALPHA
            }
            fn methods(&self) -> &[Served] {
                const ROWS: [Served; 1] = [Served::service(PING, &[])];
                &ROWS
            }
            async fn handle(&self, call: Call<'_>) -> Response {
                call.connection.follow("demo/1", async {});
                Response::result(call.id, json!({"opened": "demo/1"}))
            }
        }
        let api = AgentApi::new(vec![Arc::new(Opener) as Arc<dyn Group>]).unwrap();
        let connection = Connection::answers_only();
        // When the request comes on a connection, and again through the answers-only entry
        let answered = api.dispatch_on(BUILD, r#"{"id": 1, "method": "alpha.ping"}"#, &connection).await;
        let plain = api.dispatch_text(BUILD, r#"{"id": 2, "method": "alpha.ping"}"#).await;
        // Then the answer is the group's, the connection holds the stream the request opened, and the plain entry answers too
        assert_eq!(answered, json!({"id": 1, "result": {"opened": "demo/1"}}));
        assert_eq!(connection.take_following().iter().map(|f| f.topic.as_str()).collect::<Vec<_>>(), ["demo/1"]);
        assert_eq!(plain["result"]["opened"], json!("demo/1"));
    }

    #[test]
    fn given_groups_that_cannot_be_told_apart_when_registered_then_each_is_refused_naming_why() {
        let register = |groups: Vec<Demo>| AgentApi::new(groups.into_iter().map(|g| Arc::new(g) as Arc<dyn Group>).collect()).err();
        // Given a group registered twice, a group that answers nothing, and a group that answers a method of another group
        // Then each is refused with the reason
        assert_eq!(register(vec![Demo::of(ALPHA, &[PING]), Demo::of(ALPHA, &[PONG])]).as_deref(), Some("group alpha is registered twice"));
        assert_eq!(register(vec![Demo::of(ALPHA, &[])]).as_deref(), Some("group alpha answers no method"));
        assert_eq!(register(vec![Demo::of(ALPHA, &[PING, SEND])]).as_deref(), Some("group alpha cannot answer beta.send, a method of group beta"));
        // And an empty service is a service
        assert!(AgentApi::new(vec![]).is_ok_and(|api| api.listed().is_empty()));
    }
}
