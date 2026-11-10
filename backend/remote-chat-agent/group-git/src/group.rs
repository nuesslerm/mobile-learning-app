use agent_api_git::{self as api, SubscribeParams};
use agent_api_proto::{code, GroupName, Response};
use async_trait::async_trait;
use exec_proto::Code;
use rca_agent_core::{Call, Connection, Group, Served};
use rca_exec_channel::remote::RemoteError;
use rca_exec_channel::Executors;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;

pub const SERVED: [Served; 22] = [
    Served::service(api::STATUS, &[exec_git::GIT_STATUS]),
    Served::service(api::REFS, &[exec_git::GIT_REFS]),
    Served::service(api::HISTORY, &[exec_git::GIT_LOG]),
    Served::service(api::DIFF, &[exec_git::GIT_DIFF]),
    Served::service(api::COMPARE, &[exec_git::GIT_COMPARE]),
    Served::service(api::STAGE, &[exec_git::GIT_STAGE]),
    Served::service(api::UNSTAGE, &[exec_git::GIT_UNSTAGE]),
    Served::service(api::DISCARD, &[exec_git::GIT_DISCARD]),
    Served::service(api::COMMIT, &[exec_git::GIT_COMMIT]),
    Served::service(api::CHECKOUT, &[exec_git::GIT_CHECKOUT]),
    Served::service(api::CREATE_REF, &[exec_git::GIT_CREATE_REF]),
    Served::service(api::FETCH, &[exec_git::GIT_FETCH]),
    Served::service(api::PULL, &[exec_git::GIT_PULL]),
    Served::service(api::PUSH, &[exec_git::GIT_PUSH]),
    Served::service(api::FAST_FORWARD, &[exec_git::GIT_FAST_FORWARD]),
    Served::service(api::UPSTREAM_STATUS, &[exec_git::GIT_UPSTREAM]),
    Served::service(api::MERGE, &[exec_git::GIT_MERGE]),
    Served::service(api::REBASE, &[exec_git::GIT_REBASE]),
    Served::service(api::CONFLICT_RESOLVE, &[exec_git::GIT_CONFLICT_RESOLVE]),
    Served::service(api::BLOB, &[exec_git::GIT_BLOB]),
    Served::service(api::INIT, &[exec_git::GIT_INIT]),
    Served::service(api::SUBSCRIBE, &[exec_git::GIT_WATCH]),
];

struct Refusal(String, String);

type Answer = Result<Value, Refusal>;

fn refuse(code: &str, message: impl Into<String>) -> Refusal {
    Refusal(code.into(), message.into())
}

impl From<agent_api_proto::ParamError> for Refusal {
    fn from(e: agent_api_proto::ParamError) -> Refusal {
        refuse(code::INVALID_PARAMS, e.0)
    }
}

impl From<RemoteError> for Refusal {
    fn from(e: RemoteError) -> Refusal {
        refuse(e.code(), e.message())
    }
}

pub struct GitGroup {
    executors: Arc<dyn Executors>,
}

impl GitGroup {
    pub fn new(executors: Arc<dyn Executors>) -> GitGroup {
        GitGroup { executors }
    }

    /// The channel of the workspace's executor and the args of `action`, aimed at the worktree when the workspace is one.
    fn aimed(&self, workspace: &str, mut args: Value) -> Result<(Arc<dyn rca_exec_channel::remote::ExecutorChannel>, Value), Refusal> {
        let channel = self.executors.get(workspace).ok_or_else(|| refuse(Code::NoExecutor.as_str(), format!("no executor is attached for workspace {workspace}")))?;
        let place = self.executors.place(workspace);
        match place.worktree {
            Some(worktree) => args["worktree"] = json!(worktree),
            None if place.prefix.is_some() => return Err(refuse(Code::InvalidArgs.as_str(), format!("git is not served for workspace {workspace}: it is a directory inside the repository, and git acts on the repository root"))),
            None => {}
        }
        Ok((channel, args))
    }

    async fn relay(&self, spec: &api::Spec, params: &Value) -> Answer {
        let workspace = api::workspace_param(params)?;
        let (channel, args) = self.aimed(&workspace, api::executor_args(spec, params))?;
        Ok(channel.call(spec.action, args).await?.result)
    }

    /// Answers with where the numbering stands; the executor's `git_watch` is then relayed as `git.status` events until the peer or the executor
    /// goes, which ends the watch at the executor (the call is dropped).
    fn subscribe(&self, params: &Value, connection: &Connection) -> Answer {
        let p = SubscribeParams::parse(params)?;
        let mut args = json!({});
        if let Some(interval) = p.interval_ms {
            args["interval_ms"] = json!(interval);
        }
        let (channel, args) = self.aimed(&p.workspace, args)?;
        let sink = connection.sink();
        let (workspace, after) = (p.workspace.clone(), p.after_seq);
        connection.follow(format!("git/{}/status", p.workspace), async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
            let call = channel.call_with_events(exec_git::GIT_WATCH, args, tx);
            tokio::pin!(call);
            let mut seq = after;
            loop {
                tokio::select! {
                    Some(raw) = rx.recv() => {
                        let Some(status) = raw.get("status") else { continue };
                        seq += 1;
                        if !sink.send_replay(&api::status_event(seq, &workspace, status)).await {
                            return;
                        }
                    }
                    _ = &mut call => return,
                }
            }
        });
        Ok(json!({"workspace": p.workspace, "seq": p.after_seq}))
    }
}

#[async_trait]
impl Group for GitGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let answer = match call.method {
            api::SUBSCRIBE => self.subscribe(call.params, call.connection),
            other => match api::spec_of(other) {
                Some(spec) => self.relay(spec, call.params).await,
                None => unreachable!("{} is not a method of git", other.name()),
            },
        };
        match answer {
            Ok(result) => Response::result(call.id, result),
            Err(Refusal(code, message)) => Response::error(call.id, &code, message),
        }
    }
}
