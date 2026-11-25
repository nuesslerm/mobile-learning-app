pub mod pairing;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadWrite {
    Repositories,
    CreateWorkspace,
    Threads,
    CreateThread,
    GetThread,
    History,
    Transcript,
    Family,
    Issues,
    GetContext,
    SetContext,
    Models,
    AddTurn,
    ResumeQueue,
    Approval,
    Input,
}

#[derive(Debug)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

impl Failure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Failure>;

pub trait Agent: Send + Sync {
    fn request(&self, action: ReadWrite, params: Value) -> Result<Value>;
}

pub trait Devices: Send + Sync {
    fn exchange(&self, code: &str) -> Result<String>;
    fn authenticated(&self, token: &str) -> Result<bool>;
    fn revoke(&self, token: &str) -> Result<()>;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewThread {
    pub thread: String,
    pub title: String,
    pub driver: String,
    pub task: Option<u64>,
}

pub fn workspaces(agent: &dyn Agent, roots_only: bool) -> Result<Value> {
    let result = agent.request(ReadWrite::Repositories, json!({}))?;
    let rows = result["repos"]
        .as_array()
        .ok_or_else(|| Failure::new("BAD_RESPONSE", "RCA returned no repository list"))?;
    if roots_only {
        let roots: Vec<_> = rows.iter().filter(|row| row["kind"] == "root").collect();
        Ok(json!({"repos": roots}))
    } else {
        Ok(json!({"workspaces": rows}))
    }
}

pub fn threads(agent: &dyn Agent, workspace: &str) -> Result<Value> {
    let result = agent.request(ReadWrite::Threads, json!({}))?;
    let all = result["threads"]
        .as_array()
        .ok_or_else(|| Failure::new("BAD_RESPONSE", "RCA returned no thread list"))?;
    let visible: Vec<_> = all
        .iter()
        .filter(|row| row["workspace"] == workspace)
        .collect();
    Ok(json!({"threads":visible}))
}

pub fn create_thread(agent: &dyn Agent, workspace: &str, new: NewThread) -> Result<Value> {
    if new.thread.is_empty() || new.title.trim().is_empty() || new.driver.is_empty() {
        return Err(Failure::new(
            "INVALID_PARAMS",
            "A thread ID, title and driver are required",
        ));
    }
    let thread = agent.request(
        ReadWrite::CreateThread,
        json!({
            "thread":new.thread,"workspace":workspace,"driver":new.driver,"title":new.title
        }),
    )?;
    agent.request(
        ReadWrite::SetContext,
        json!({"scope":"mobile.thread.workspace","key":new.thread,"value":workspace}),
    )?;
    if let Some(number) = new.task {
        agent.request(
            ReadWrite::SetContext,
            json!({"scope":"mobile.thread.task","key":new.thread,
            "value":{"workspace":workspace,"number":number}}),
        )?;
    }
    Ok(thread)
}

pub fn context(agent: &dyn Agent, thread: &str) -> Result<Value> {
    let args = json!({"thread":thread});
    Ok(json!({
        "thread":agent.request(ReadWrite::GetThread, args.clone())?["thread"],
        "family":agent.request(ReadWrite::Family, args)?,
        "task":agent.request(ReadWrite::GetContext, json!({"scope":"mobile.thread.task","key":thread}))?["value"]
    }))
}

pub fn submit(agent: &dyn Agent, thread: &str, id: &str, prompt: &str) -> Result<Value> {
    if id.is_empty() || prompt.trim().is_empty() {
        return Err(Failure::new(
            "INVALID_PARAMS",
            "A turn ID and prompt are required",
        ));
    }
    let receipt = json!({"thread":thread,"turn":id});
    let snapshot = agent.request(ReadWrite::GetThread, json!({"thread":thread}))?;
    let thread_view = &snapshot["thread"];
    let mut cursor = 0;
    while cursor < thread_view["last_seq"].as_u64().unwrap_or(0) {
        let page = agent.request(
            ReadWrite::History,
            json!({"thread":thread,"after_seq":cursor,"limit":500}),
        )?;
        let events = page["events"]
            .as_array()
            .ok_or_else(|| Failure::new("BAD_RESPONSE", "Missing history events"))?;
        if events
            .iter()
            .any(|e| e["type"] == "queue.started" && e["item"] == id)
        {
            return Ok(receipt);
        }
        if page["more"] != true {
            break;
        }
        cursor = events
            .last()
            .and_then(|e| e["seq"].as_u64())
            .filter(|seq| *seq > cursor)
            .ok_or_else(|| Failure::new("BAD_RESPONSE", "History cursor did not advance"))?;
    }
    let mut params =
        json!({"thread":thread,"item":id,"prompt":prompt,"runtime_mode":"approval-required"});
    if let Some(model) = thread_view["model"].as_str() {
        params["model"] = json!(model);
    } else if let Some(driver) = thread_view["driver"].as_str() {
        let catalog = agent.request(ReadWrite::Models, json!({"driver":driver}))?;
        if let Some(model) = catalog["models"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["is_default"] == true))
        {
            if model["id"] != "default" {
                params["model"] = model["id"].clone();
            }
        }
    }
    match agent.request(ReadWrite::AddTurn, params) {
        Ok(_) => {
            agent.request(ReadWrite::ResumeQueue, json!({"thread":thread}))?;
        }
        Err(failure) if failure.code == "ITEM_ALREADY_STARTED" => {}
        Err(failure) => return Err(failure),
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Recorded {
        calls: Mutex<Vec<ReadWrite>>,
        started: bool,
    }
    impl Agent for Recorded {
        fn request(&self, op: ReadWrite, _: Value) -> Result<Value> {
            self.calls.lock().unwrap().push(op);
            if op == ReadWrite::AddTurn && self.started {
                return Err(Failure::new("ITEM_ALREADY_STARTED", "running"));
            }
            Ok(json!({}))
        }
    }

    #[test]
    fn given_a_lost_response_when_the_same_turn_returns_then_it_is_acknowledged_once() {
        let rca = Recorded {
            calls: Mutex::default(),
            started: true,
        };
        assert_eq!(
            submit(&rca, "thread", "turn", "do it").unwrap(),
            json!({"thread":"thread","turn":"turn"})
        );
        assert_eq!(
            *rca.calls.lock().unwrap(),
            [ReadWrite::GetThread, ReadWrite::AddTurn]
        );
    }

    #[test]
    fn given_a_new_prompt_when_submitted_then_rca_starts_its_queue() {
        let rca = Recorded {
            calls: Mutex::default(),
            started: false,
        };
        submit(&rca, "thread", "turn", "do it").unwrap();
        assert_eq!(
            *rca.calls.lock().unwrap(),
            [
                ReadWrite::GetThread,
                ReadWrite::AddTurn,
                ReadWrite::ResumeQueue
            ]
        );
    }
}
