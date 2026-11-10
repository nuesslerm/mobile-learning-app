//! The `agent.requests` group: a turn asks through the MCP tools `approval_prompt` and `ask_user`, which the service answers itself after recording the request.
//! `approval.answer`, `input.answer` and `input.dismiss` close it in the log and give the waiting call its answer, which the CLI reads as the tool's result.
use crate::port::Command;
use crate::refusal::{respond, to_value, Answer};
use crate::runner::Runner;
use agent_api_agent::requests::{self as api, ApprovalAnswerParams, ApprovalAnswerResult, AnswerResult, Decision, InputAnswerParams, InputDismissParams};
use agent_api_agent::ThreadEvent;
use agent_api_proto::{GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::mcp::{ServiceTools, APPROVAL_PROMPT, ASK_USER};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

pub const SERVED: [Served; 3] = [Served::service(api::APPROVAL_ANSWER, &[]), Served::service(api::INPUT_ANSWER, &[]), Served::service(api::INPUT_DISMISS, &[])];

/// What a person's answer was, as the waiting tool call learns it.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Approval(Decision),
    Input(String),
    Dismissed,
}

struct Waiter {
    thread: String,
    reply: oneshot::Sender<Reply>,
}

/// The requests that wait for a person, by id, and the tools the person allowed for the rest of a thread's session.
#[derive(Default)]
pub struct Waiting {
    open: Mutex<HashMap<String, Waiter>>,
    granted: Mutex<HashSet<(String, String)>>,
}

impl Waiting {
    fn open(&self) -> std::sync::MutexGuard<'_, HashMap<String, Waiter>> {
        self.open.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn granted(&self) -> std::sync::MutexGuard<'_, HashSet<(String, String)>> {
        self.granted.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Starts waiting for the answer to `request` of `thread`.
    pub fn wait(&self, thread: &str, request: &str) -> oneshot::Receiver<Reply> {
        let (reply, answer) = oneshot::channel();
        self.open().insert(request.to_string(), Waiter { thread: thread.to_string(), reply });
        answer
    }

    /// Gives the call waiting on `request` of `thread` its answer; whether one was waiting.
    pub fn resolve(&self, thread: &str, request: &str, reply: Reply) -> bool {
        let mut open = self.open();
        match open.get(request) {
            Some(waiter) if waiter.thread == thread => open.remove(request).is_some_and(|w| w.reply.send(reply).is_ok()),
            _ => false,
        }
    }

    pub fn forget(&self, request: &str) {
        self.open().remove(request);
    }

    /// Lets go of every request of `thread` that waits: the turn that asked is over, so nobody can answer it.
    pub fn release(&self, thread: &str) {
        self.open().retain(|_, waiter| waiter.thread != thread);
    }

    pub fn grant(&self, thread: &str, tool: &str) {
        self.granted().insert((thread.to_string(), tool.to_string()));
    }

    pub fn is_granted(&self, thread: &str, tool: &str) -> bool {
        self.granted().contains(&(thread.to_string(), tool.to_string()))
    }

    pub fn forget_grants(&self, thread: &str) {
        self.granted().retain(|(granted, _)| granted != thread);
    }
}

/// An MCP `tools/call` result with `body` as its text.
fn content(body: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": body}], "isError": is_error})
}

fn allow(input: &Value) -> Value {
    content(json!({"behavior": "allow", "updatedInput": input}).to_string(), false)
}

fn deny(why: &str, interrupt: bool) -> Value {
    let mut answer = json!({"behavior": "deny", "message": why});
    if interrupt {
        answer["interrupt"] = json!(true);
    }
    content(answer.to_string(), false)
}

impl Runner {
    /// The turn the thread runs, for the envelope of what it asks.
    fn asking_turn(&self, thread: &str) -> Option<String> {
        self.engine().get(thread).ok().flatten().and_then(|view| view.turn)
    }

    /// `approval_prompt`: the thread's turn asks whether `tool_name` may run with `input`, and waits for `approval.answer`. A tool the person allowed
    /// for the session is allowed again without asking.
    async fn approval_prompt(&self, thread: &str, args: &Value) -> Value {
        let tool = args["tool_name"].as_str().unwrap_or_default().to_string();
        let input = args["input"].clone();
        if self.waiting().is_granted(thread, &tool) {
            return allow(&input);
        }
        let approval = format!("approval-{}", self.engine().new_id());
        let answer = self.waiting().wait(thread, &approval);
        let asked = ThreadEvent::ApprovalRequested { approval: approval.clone(), tool: tool.clone(), args: input.clone() };
        if let Err(e) = self.engine().record(thread, self.asking_turn(thread).as_deref(), asked) {
            self.waiting().forget(&approval);
            return deny(&format!("The service could not ask the person: {}", e.0), false);
        }
        match answer.await {
            Ok(Reply::Approval(Decision::Accept)) => allow(&input),
            Ok(Reply::Approval(Decision::AcceptForSession)) => {
                self.waiting().grant(thread, &tool);
                allow(&input)
            }
            Ok(Reply::Approval(Decision::Decline)) => deny("The person declined this tool call.", false),
            Ok(Reply::Approval(Decision::Cancel)) => deny("The person declined this tool call and ended the turn.", true),
            _ => deny("The turn ended before the person answered.", false),
        }
    }

    /// `ask_user`: the thread's turn asks the person a question and waits for `input.answer` (its text is the tool's result) or `input.dismiss`.
    async fn ask_user(&self, thread: &str, args: &Value) -> Value {
        let prompt = args["question"].as_str().unwrap_or_default().to_string();
        let options: Vec<String> = args["options"].as_array().map(|all| all.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        let request = format!("input-{}", self.engine().new_id());
        let answer = self.waiting().wait(thread, &request);
        if let Err(e) = self.engine().record(thread, self.asking_turn(thread).as_deref(), ThreadEvent::InputRequested { request: request.clone(), prompt, options }) {
            self.waiting().forget(&request);
            return content(format!("The service could not ask the person: {}", e.0), true);
        }
        match answer.await {
            Ok(Reply::Input(text)) => content(text, false),
            Ok(Reply::Dismissed) => content("The person closed the question without answering it.".into(), false),
            _ => content("The turn ended before the person answered.".into(), true),
        }
    }
}

#[async_trait]
impl ServiceTools for Runner {
    async fn call(&self, thread: &str, tool: &str, args: &Value) -> Value {
        match tool {
            APPROVAL_PROMPT => self.approval_prompt(thread, args).await,
            ASK_USER => self.ask_user(thread, args).await,
            other => content(format!("this service answers no tool {other}"), true),
        }
    }
}

pub struct AgentRequestsGroup {
    runner: Arc<Runner>,
}

impl AgentRequestsGroup {
    pub fn new(runner: Arc<Runner>) -> AgentRequestsGroup {
        AgentRequestsGroup { runner }
    }

    fn approval_answer(&self, params: &Value) -> Answer {
        let p = ApprovalAnswerParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::ApprovalAnswer { approval: p.approval.clone(), decision: p.decision })?;
        self.runner.waiting().resolve(&p.thread, &p.approval, Reply::Approval(p.decision));
        if p.decision == Decision::Cancel {
            self.runner.interrupt(&p.thread, None);
        }
        to_value(&ApprovalAnswerResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, approval: p.approval, decision: p.decision })
    }

    fn input_answer(&self, params: &Value) -> Answer {
        let p = InputAnswerParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::InputAnswer { request: p.request.clone(), answer: p.answer.clone() })?;
        self.runner.waiting().resolve(&p.thread, &p.request, Reply::Input(p.answer));
        to_value(&AnswerResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, request: p.request })
    }

    fn input_dismiss(&self, params: &Value) -> Answer {
        let p = InputDismissParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::InputDismiss { request: p.request.clone() })?;
        self.runner.waiting().resolve(&p.thread, &p.request, Reply::Dismissed);
        to_value(&AnswerResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, request: p.request })
    }
}

#[async_trait]
impl Group for AgentRequestsGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, .. } = call;
        let answer = match method.name() {
            "approval.answer" => self.approval_answer(params),
            "input.answer" => self.input_answer(params),
            "input.dismiss" => self.input_dismiss(params),
            other => unreachable!("{other} is no method of agent.requests (SERVED)"),
        };
        respond(id, answer)
    }
}
