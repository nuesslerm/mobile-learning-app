//! The `git.ai` sub-group: `git.generateCommitMessage` asks the workspace's executor for the change (`git_diff`) and a model, through
//! `ModelApi`, for the message. The prompt and the tidying of the model's answer are pure and tested alone.
use crate::ports::{Completion, ModelApi, ModelError};
use agent_api_git::ai::{self, code, GenerateParams, Style};
use agent_api_proto::{code as proto_code, GroupName, Response};
use async_trait::async_trait;
use exec_proto::Code;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::Executors;
use serde_json::{json, Value};
use std::sync::Arc;

pub const GIT_AI_SERVED: [Served; 1] = [Served::service(ai::GENERATE_COMMIT_MESSAGE, &[exec_git::GIT_DIFF])];

/// The most of a diff that goes to the model: a message is written from the head of a large change, and says so.
const DIFF_CHARS: usize = 24_000;

const SYSTEM: &str = "You write git commit messages. Answer with the commit message only: no code fence, no quotes, no commentary.";

/// What the model is asked for the change `diff` in `style`.
pub fn commit_message_prompt(diff: &str, style: Style) -> Completion {
    let rule = match style {
        Style::Conventional => "Use the Conventional Commits form `type(scope): summary` (type is feat, fix, docs, refactor, test, chore, perf, build or ci; the scope may be left out), with the summary in the imperative, at most 72 characters. Add a body after a blank line only when the change needs one.",
        Style::Plain => "Write one summary line in the imperative, at most 72 characters. Add a body after a blank line only when the change needs one.",
    };
    let shown: String = diff.chars().take(DIFF_CHARS).collect();
    let cut = if diff.chars().count() > DIFF_CHARS { "\n[the diff is cut here: it is longer than what is shown]\n" } else { "" };
    Completion { system: SYSTEM.into(), prompt: format!("{rule}\n\nThe change:\n\n{shown}{cut}"), max_tokens: 300 }
}

/// The message in a model's answer: a code fence or a pair of quotes around it is dropped, as is the blank space at either end. None when
/// nothing is left.
pub fn tidy_message(answer: &str) -> Option<String> {
    let mut text = answer.trim();
    if let Some(inner) = text.strip_prefix("```") {
        let inner = inner.split_once('\n').map_or("", |(_, rest)| rest);
        text = inner.rsplit_once("```").map_or(inner, |(body, _)| body).trim();
    }
    let unquoted = text.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(text).trim();
    (!unquoted.is_empty()).then(|| unquoted.to_string())
}

pub struct GitAiGroup {
    executors: Arc<dyn Executors>,
    models: Arc<dyn ModelApi>,
}

impl GitAiGroup {
    pub fn new(executors: Arc<dyn Executors>, models: Arc<dyn ModelApi>) -> GitAiGroup {
        GitAiGroup { executors, models }
    }

    async fn generate(&self, params: &Value) -> Result<Value, (String, String)> {
        let p = GenerateParams::parse(params).map_err(|e| (proto_code::INVALID_PARAMS.to_string(), e.0))?;
        let channel = self.executors.get(&p.workspace).ok_or_else(|| (Code::NoExecutor.as_str().to_string(), format!("no executor is attached for workspace {}", p.workspace)))?;
        let place = self.executors.place(&p.workspace);
        let mut args = json!({ "staged": p.staged });
        match place.worktree {
            Some(worktree) => args["worktree"] = json!(worktree),
            None if place.prefix.is_some() => return Err((Code::InvalidArgs.as_str().to_string(), format!("git is not served for workspace {}: it is a directory inside the repository, and git acts on the repository root", p.workspace))),
            None => {}
        }
        let diff = channel.call(exec_git::GIT_DIFF, args).await.map_err(|e| (e.code().to_string(), e.message().to_string()))?.result;
        let diff = diff.get("diff").and_then(Value::as_str).unwrap_or_default();
        if diff.trim().is_empty() {
            return Err((code::NOTHING_TO_COMMIT.to_string(), if p.staged { "nothing is staged".into() } else { "the working tree has no change".into() }));
        }
        let unavailable = |why: String| (code::MODEL_API_UNAVAILABLE.to_string(), why);
        let completed = self.models.complete(commit_message_prompt(diff, p.style)).await.map_err(|e| match e {
            ModelError::Unavailable(why) | ModelError::NotSupported(why) => unavailable(why),
        })?;
        let message = tidy_message(&completed.text).ok_or_else(|| unavailable("the model answered with no message".into()))?;
        Ok(ai::generate_result(&message, &completed.model))
    }
}

#[async_trait]
impl Group for GitAiGroup {
    fn name(&self) -> GroupName {
        ai::GROUP
    }

    fn methods(&self) -> &[Served] {
        &GIT_AI_SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        debug_assert_eq!(call.method, ai::GENERATE_COMMIT_MESSAGE);
        match self.generate(call.params).await {
            Ok(result) => Response::result(call.id, result),
            Err((code, message)) => Response::error(call.id, &code, message),
        }
    }
}
