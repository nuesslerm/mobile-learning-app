//! A client command as the WebSocket delivers it (`ClientOrchestrationCommandSchema` in the contracts package), read by serde: the
//! command's `type` first, then the fields that type needs. Unknown fields are dropped; the first field that does not fit is the
//! error, in serde's words with its path.

use rca_service_core::domain::schema::{parse, Cli, Decision, InteractionMode, NonEmpty, RuntimeMode, UserRole, Utc};
use remote_chat_agent_core::domain::types::{Body, Bootstrap, Command, ModelSelection};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
enum CommandType {
    #[serde(rename = "thread.create")]
    Create,
    #[serde(rename = "thread.delete")]
    Delete,
    #[serde(rename = "thread.archive")]
    Archive,
    #[serde(rename = "thread.unarchive")]
    Unarchive,
    #[serde(rename = "thread.meta.update")]
    MetaUpdate,
    #[serde(rename = "thread.runtime-mode.set")]
    RuntimeModeSet,
    #[serde(rename = "thread.interaction-mode.set")]
    InteractionModeSet,
    #[serde(rename = "thread.turn.start")]
    TurnStart,
    #[serde(rename = "thread.turn.interrupt")]
    TurnInterrupt,
    #[serde(rename = "thread.approval.respond")]
    ApprovalRespond,
    #[serde(rename = "thread.user-input.respond")]
    UserInputRespond,
    #[serde(rename = "thread.session.stop")]
    SessionStop,
}

#[derive(Deserialize)]
struct Tagged {
    #[serde(rename = "type")]
    kind: CommandType,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ids {
    command_id: NonEmpty,
    thread_id: NonEmpty,
}

#[derive(Deserialize)]
struct ModelSelectionIn {
    cli: Cli,
    #[serde(rename = "providerID")]
    provider_id: Option<NonEmpty>,
    #[serde(rename = "modelID")]
    model_id: Option<NonEmpty>,
}

impl From<ModelSelectionIn> for ModelSelection {
    fn from(selection: ModelSelectionIn) -> Self {
        Self { cli: selection.cli.as_str().to_string(), provider_id: selection.provider_id.map(String::from), model_id: selection.model_id.map(String::from) }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadOptions {
    title: NonEmpty,
    cwd: Option<NonEmpty>,
    model_selection: ModelSelectionIn,
    runtime_mode: RuntimeMode,
    interaction_mode: InteractionMode,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetaUpdateIn {
    title: Option<NonEmpty>,
    model_selection: Option<ModelSelectionIn>,
    cwd: Option<NonEmpty>,
    branch: Option<NonEmpty>,
    worktree_path: Option<NonEmpty>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeModeSetIn {
    runtime_mode: RuntimeMode,
    #[allow(dead_code)]
    created_at: Utc,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InteractionModeSetIn {
    interaction_mode: InteractionMode,
    #[allow(dead_code)]
    created_at: Utc,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageIn {
    message_id: NonEmpty,
    #[allow(dead_code)]
    role: UserRole,
    text: String,
    attachments: Option<Vec<Value>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapIn {
    create_thread: ThreadOptions,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnStartIn {
    message: MessageIn,
    model_selection: Option<ModelSelectionIn>,
    runtime_mode: RuntimeMode,
    interaction_mode: InteractionMode,
    source_proposed_plan: Option<Value>,
    bootstrap: Option<BootstrapIn>,
    #[allow(dead_code)]
    created_at: Utc,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnInterruptIn {
    turn_id: Option<NonEmpty>,
    created_at: Utc,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalRespondIn {
    request_id: NonEmpty,
    decision: Decision,
    created_at: Utc,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserInputRespondIn {
    request_id: NonEmpty,
    answers: Map<String, Value>,
    created_at: Utc,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionStopIn {
    created_at: Utc,
}

fn options(options: ThreadOptions) -> (String, Option<String>, ModelSelection, String, String) {
    (options.title.into(), options.cwd.map(String::from), options.model_selection.into(), options.runtime_mode.as_str().to_string(), options.interaction_mode.as_str().to_string())
}

/// Reads a command. The error is the first thing wrong with it, as serde words it.
pub fn parse_command(value: &Value) -> Result<Command, String> {
    parse::<Map<String, Value>>(value)?;
    let kind = parse::<Tagged>(value)?.kind;
    let ids = parse::<Ids>(value)?;
    let body = match kind {
        CommandType::Create => {
            let thread = parse::<ThreadOptions>(value)?;
            let rest = parse::<CreateRest>(value)?;
            let (title, cwd, model_selection, runtime_mode, interaction_mode) = options(thread);
            Body::Create { title, cwd, model_selection, runtime_mode, interaction_mode, branch: rest.branch.map(String::from), worktree_path: rest.worktree_path.map(String::from) }
        }
        CommandType::Delete => Body::Delete,
        CommandType::Archive => Body::Archive,
        CommandType::Unarchive => Body::Unarchive,
        CommandType::MetaUpdate => {
            let update = parse::<MetaUpdateIn>(value)?;
            Body::MetaUpdate { title: update.title.map(String::from), model_selection: update.model_selection.map(Into::into), cwd: update.cwd.map(String::from), branch: update.branch.map(String::from), worktree_path: update.worktree_path.map(String::from) }
        }
        CommandType::RuntimeModeSet => Body::RuntimeModeSet { runtime_mode: parse::<RuntimeModeSetIn>(value)?.runtime_mode.as_str().to_string() },
        CommandType::InteractionModeSet => Body::InteractionModeSet { interaction_mode: parse::<InteractionModeSetIn>(value)?.interaction_mode.as_str().to_string() },
        CommandType::TurnStart => {
            let turn = parse::<TurnStartIn>(value)?;
            Body::TurnStart {
                message_id: turn.message.message_id.into(),
                text: turn.message.text,
                attachments: turn.message.attachments,
                model_selection: turn.model_selection.map(Into::into),
                runtime_mode: turn.runtime_mode.as_str().to_string(),
                interaction_mode: turn.interaction_mode.as_str().to_string(),
                source_proposed_plan: turn.source_proposed_plan,
                bootstrap: turn.bootstrap.map(|b| {
                    let (title, cwd, model_selection, runtime_mode, interaction_mode) = options(b.create_thread);
                    Bootstrap { title, cwd, model_selection, runtime_mode, interaction_mode }
                }),
            }
        }
        CommandType::TurnInterrupt => {
            let interrupt = parse::<TurnInterruptIn>(value)?;
            Body::TurnInterrupt { turn_id: interrupt.turn_id.map(String::from), created_at: interrupt.created_at.into() }
        }
        CommandType::ApprovalRespond => {
            let answer = parse::<ApprovalRespondIn>(value)?;
            Body::ApprovalRespond { request_id: answer.request_id.into(), decision: answer.decision.as_str().to_string(), created_at: answer.created_at.into() }
        }
        CommandType::UserInputRespond => {
            let answer = parse::<UserInputRespondIn>(value)?;
            Body::UserInputRespond { request_id: answer.request_id.into(), answers: answer.answers, created_at: answer.created_at.into() }
        }
        CommandType::SessionStop => Body::SessionStop { created_at: parse::<SessionStopIn>(value)?.created_at.into() },
    };
    Ok(Command { command_id: ids.command_id.into(), thread_id: ids.thread_id.into(), body })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRest {
    branch: Option<NonEmpty>,
    worktree_path: Option<NonEmpty>,
    #[allow(dead_code)]
    created_at: Utc,
}
