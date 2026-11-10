//! `scheduledTasks.*`: the phone's scheduled tasks over the agent API's `schedules` groups. A task is a schedule that runs in its project's
//! workspace (the project's directory picks the `workspace`), with the model's instance as its `driver`; what the phone keeps and the agent
//! API has no field for (the project, the workspace strategy, the model selection, who made it) travels in the schedule's `meta` and comes back as it
//! was saved. What rca does not offer is refused with its reason, never dropped: a webhook's HMAC signature, filters and dedupe rules, and a
//! workspace strategy other than the project's own directory.
use super::queries::{field, QueryFailure};
use super::service::Service;
use crate::ports::{AgentError, Follow, Op};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use t3_contracts::time::iso;

const ERROR: &str = "ScheduledTaskError";

fn error(message: impl Into<String>, task: Option<&str>) -> QueryFailure {
    let mut error = json!({"_tag": ERROR, "message": message.into()});
    if let Some(task) = task {
        error["taskId"] = json!(task);
    }
    QueryFailure::Error(error)
}

fn refused(e: &AgentError, task: Option<&str>) -> QueryFailure {
    error(e.message(), task)
}

/// The agent API's schedule for the phone's, or why rca cannot keep it.
pub fn to_agent_schedule(schedule: &Value) -> Result<Value, String> {
    match schedule["type"].as_str() {
        Some("interval") => Ok(json!({"type": "interval", "every_ms": schedule["everyMs"]})),
        Some("fixed_time") => {
            let mut out = json!({"type": "fixed_time", "time_of_day": schedule["timeOfDay"]});
            if let Some(days) = schedule.get("weekdays").filter(|d| !d.is_null()) {
                out["weekdays"] = days.clone();
            }
            Ok(out)
        }
        Some("webhook") => {
            let asked = |key: &str| schedule.get(key).is_some_and(|v| !v.is_null() && v != &json!([]) && v != &json!({}));
            match ["signature", "filters", "dedupe", "maxDeliveryAgeMinutes"].into_iter().find(|key| asked(key)) {
                Some(key) => Err(format!("a webhook's {key} is not offered by rca: a webhook schedule is reached by its URL token alone")),
                None => Ok(json!({"type": "webhook"})),
            }
        }
        other => Err(format!("{} is no kind of schedule", other.unwrap_or("a schedule with no type"))),
    }
}

fn from_agent_schedule(schedule: &Value) -> Value {
    match schedule["type"].as_str() {
        Some("interval") => json!({"type": "interval", "everyMs": schedule["every_ms"]}),
        Some("fixed_time") => {
            let mut out = json!({"type": "fixed_time", "timeOfDay": schedule["time_of_day"]});
            if let Some(days) = schedule.get("weekdays").filter(|d| !d.is_null()) {
                out["weekdays"] = days.clone();
            }
            out
        }
        _ => json!({"type": "webhook", "signature": null}),
    }
}

fn at(ms: &Value) -> Value {
    ms.as_i64().map_or(Value::Null, |ms| json!(iso(ms)))
}

/// The phone's `ScheduledTask` for the agent API's. `token` is the webhook token a call just made: only then does the task's webhook have an address
/// that carries it.
pub fn task_from_agent(task: &Value, token: Option<&str>) -> Value {
    let meta = &task["meta"];
    let kept = |key: &str, default: Value| meta.get(key).filter(|v| !v.is_null()).cloned().unwrap_or(default);
    let mut model = json!({"instanceId": task["driver"]});
    if !task["model"].is_null() {
        model["model"] = task["model"].clone();
    }
    let mut out = json!({
        "id": task["id"], "title": task["title"], "prompt": task["prompt"], "enabled": task["enabled"],
        "schedule": from_agent_schedule(&task["schedule"]),
        "projectId": kept("projectId", json!("")), "threadId": task["thread"],
        "workspaceStrategy": kept("workspaceStrategy", json!({"type": "root"})), "modelSelection": kept("modelSelection", model),
        "runtimeMode": task["runtime_mode"].as_str().unwrap_or("full-access"), "interactionMode": task["interaction_mode"].as_str().unwrap_or("default"),
        "createdBy": kept("createdBy", json!("user")), "creationSource": kept("creationSource", json!("mobile")),
        "createdAt": at(&task["created_at_ms"]), "updatedAt": at(&task["updated_at_ms"]),
        "nextRunAt": at(&task["next_run_ms"]), "lastRunAt": at(&task["last_run_ms"]),
        "lastRunStatus": task["last_run_status"], "lastRunError": task["last_run_error"], "runCount": task["run_count"],
    });
    if let Some(path) = task["webhook"]["path"].as_str() {
        out["webhook"] = json!({"path": path, "url": token.map_or(Value::Null, |t| json!(format!("{path}/{t}"))), "hasSecret": false});
    }
    out
}

/// What `scheduledTasks.subscribe` last told the app.
pub struct ScheduleFold {
    last: Value,
}

impl Service {
    fn schedule_op(&self, method: &'static str, root: Option<String>, params: Value) -> Result<Value, AgentError> {
        self.agent().run(&Op::Schedule { method, root, params })
    }

    fn scheduled_task(&self, answer: &Value) -> Value {
        task_from_agent(&answer["task"], answer["webhook_token"].as_str())
    }

    /// `scheduledTasks.upsert`
    pub fn scheduled_upsert(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (title, prompt, project) = (field(payload, "title")?, field(payload, "prompt")?, field(payload, "projectId")?);
        let enabled = payload["enabled"].as_bool().ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"enabled\"]".into()))?;
        let id = payload["id"].as_str();
        let root = self.engine().project_root(project).ok_or_else(|| error(format!("project {project} was not found"), id))?;
        let strategy = payload.get("workspaceStrategy").filter(|s| !s.is_null()).cloned().unwrap_or_else(|| json!({"type": "root"}));
        if strategy["type"] != "root" || strategy.get("branch").is_some_and(|b| !b.is_null()) {
            return Err(error("rca runs a scheduled task in the project's directory: a worktree or a branch for each run is not offered", id));
        }
        let schedule = to_agent_schedule(&payload["schedule"]).map_err(|why| error(why, id))?;
        let selection = payload.get("modelSelection").filter(|s| s.is_object()).ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"modelSelection\"]".into()))?;
        let driver = field(selection, "instanceId")?;
        let meta = json!({"projectId": project, "workspaceStrategy": strategy, "modelSelection": selection, "createdBy": payload.get("createdBy").filter(|v| !v.is_null()).cloned().unwrap_or(json!("user")), "creationSource": payload.get("creationSource").filter(|v| !v.is_null()).cloned().unwrap_or(json!("mobile"))});
        let mut params = json!({"title": title, "prompt": prompt, "enabled": enabled, "schedule": schedule, "driver": driver, "meta": meta});
        let optional = [("id", payload.get("id")), ("require_existing", payload.get("requireExisting")), ("model", selection.get("model")), ("thread", payload.get("threadId")), ("runtime_mode", payload.get("runtimeMode")), ("interaction_mode", payload.get("interactionMode"))];
        for (name, value) in optional {
            if let Some(value) = value.filter(|v| !v.is_null()) {
                params[name] = value.clone();
            }
        }
        let answer = self.schedule_op("schedule.upsert", Some(root), params).map_err(|e| refused(&e, id))?;
        Ok(json!({"task": self.scheduled_task(&answer)}))
    }

    /// `scheduledTasks.setEnabled`
    pub fn scheduled_set_enabled(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let id = field(payload, "id")?;
        let enabled = payload["enabled"].as_bool().ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"enabled\"]".into()))?;
        let answer = self.schedule_op("schedule.setEnabled", None, json!({"id": id, "enabled": enabled})).map_err(|e| refused(&e, Some(id)))?;
        Ok(json!({"task": self.scheduled_task(&answer)}))
    }

    /// `scheduledTasks.delete`
    pub fn scheduled_delete(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let id = field(payload, "id")?;
        self.schedule_op("schedule.delete", None, json!({"id": id})).map_err(|e| refused(&e, Some(id)))?;
        Ok(json!({"id": id}))
    }

    /// `scheduledTasks.runNow`: the task's turn started; the run is the thread's and the subscription's.
    pub fn scheduled_run_now(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let id = field(payload, "id")?;
        let answer = self.schedule_op("schedule.runNow", None, json!({"id": id})).map_err(|e| refused(&e, Some(id)))?;
        Ok(json!({"task": self.scheduled_task(&answer)}))
    }

    /// `scheduledTasks.rotateWebhookToken`: the task, with the address of its webhook that holds the new token.
    pub fn scheduled_rotate_token(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let id = field(payload, "id")?;
        let answer = self.schedule_op("schedule.rotateWebhookToken", None, json!({"id": id})).map_err(|e| refused(&e, Some(id)))?;
        Ok(json!({"task": self.scheduled_task(&answer)}))
    }

    fn scheduled_list(&self) -> Result<Value, AgentError> {
        let listed = self.schedule_op("schedule.list", None, json!({}))?;
        let tasks: Vec<Value> = listed["tasks"].as_array().into_iter().flatten().map(|t| task_from_agent(t, None)).collect();
        Ok(json!({"tasks": tasks}))
    }

    /// `scheduledTasks.subscribe`: the tasks now, and again after each change.
    pub fn open_schedules(&self) -> Result<(ScheduleFold, Value), QueryFailure> {
        let first = self.scheduled_list().map_err(|e| refused(&e, None))?;
        Ok((ScheduleFold { last: first.clone() }, first))
    }

    /// Follows the stream on a thread of its own: the tasks are listed again when something happened, and sent when they differ from what was sent.
    pub fn follow_schedules(&self, mut fold: ScheduleFold, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let mut first = true;
            let result = svc.agent().schedule_follow(&mut |event| {
                if stop.load(Ordering::SeqCst) {
                    return Follow::Stop;
                }
                // The first look is the subscription's own: whatever changed between the listing and it is caught here
                let moved = std::mem::take(&mut first) || event.is_some_and(|e| e["type"] == "schedule.changed" || e["type"] == "schedule.run");
                if moved {
                    match svc.scheduled_list() {
                        Ok(list) if list != fold.last => {
                            fold.last = list.clone();
                            if to.send((id.clone(), list)).is_err() {
                                return Follow::Stop;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => svc.log("scheduledTasks.list.failed", &[("message", e.message())]),
                    }
                }
                Follow::Continue
            });
            if let Err(e) = result {
                svc.log("scheduledTasks.follow.stopped", &[("message", e.message())]);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_each_schedule_kind_of_the_phone_when_turned_into_the_agent_apis_and_back_then_it_is_the_same_and_what_rca_does_not_keep_is_refused_by_name() {
        // Given the three kinds the phone saves
        let kinds = [json!({"type": "interval", "everyMs": 120_000}), json!({"type": "fixed_time", "timeOfDay": "09:30", "weekdays": [1, 3]}), json!({"type": "webhook", "signature": null})];
        for kind in kinds {
            // When each is turned into the agent API's and back
            let there = to_agent_schedule(&kind).unwrap();
            // Then the agent API's has snake_case names and the way back is what was given
            assert_eq!(from_agent_schedule(&there), kind, "{there}");
        }
        assert_eq!(to_agent_schedule(&json!({"type": "interval", "everyMs": 60_000})).unwrap(), json!({"type": "interval", "every_ms": 60_000}));
        // And a webhook that asks for what rca does not do is refused naming it, and an unknown kind is refused
        for (key, value) in [("signature", json!({"header": "x-hub-signature-256", "encoding": "hex", "prefix": "sha256="})), ("filters", json!([{"path": "body.action", "op": "exists"}])), ("dedupe", json!({"replayKey": "{{body.id}}"})), ("maxDeliveryAgeMinutes", json!(30))] {
            let mut webhook = json!({"type": "webhook"});
            webhook[key] = value;
            assert!(to_agent_schedule(&webhook).unwrap_err().contains(&format!("a webhook's {key} is not offered")), "{key}");
        }
        assert_eq!(to_agent_schedule(&json!({"type": "webhook", "filters": []})), Ok(json!({"type": "webhook"})));
        assert!(to_agent_schedule(&json!({"type": "cron"})).unwrap_err().contains("cron"));
    }

    #[test]
    fn given_an_agent_api_task_when_shown_to_the_phone_then_the_phones_own_fields_come_back_times_are_iso_and_the_webhook_address_carries_a_token_only_when_one_was_just_made() {
        // Given a webhook task the agent API keeps with the phone's meta, run once
        let task = json!({"id": "s1", "title": "T", "prompt": "p", "enabled": true, "schedule": {"type": "webhook"}, "workspace": "w1", "driver": "claudeAgent", "model": "opus", "thread": null, "runtime_mode": null, "interaction_mode": "plan",
            "created_at_ms": 1_700_000_000_000u64, "updated_at_ms": 1_700_000_000_000u64, "next_run_ms": null, "last_run_ms": 1_700_000_060_000u64, "last_run_status": "failed", "last_run_error": "boom", "run_count": 2,
            "webhook": {"path": "/hooks/v1/schedules/s1", "has_token": true}, "meta": {"projectId": "p1", "workspaceStrategy": {"type": "root"}, "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}, "createdBy": "agent", "creationSource": "mcp"}});
        // When it is shown with and without the token a call just made
        let (fresh, later) = (task_from_agent(&task, Some("tok")), task_from_agent(&task, None));
        // Then the phone's fields are as it saved them, the times are ISO, and only the fresh one has an address with the token
        assert_eq!((fresh["projectId"].clone(), fresh["createdBy"].clone(), fresh["creationSource"].clone(), fresh["interactionMode"].clone(), fresh["runtimeMode"].clone()), (json!("p1"), json!("agent"), json!("mcp"), json!("plan"), json!("full-access")));
        assert_eq!((fresh["createdAt"].clone(), fresh["lastRunAt"].clone(), fresh["nextRunAt"].clone()), (json!("2023-11-14T22:13:20.000Z"), json!("2023-11-14T22:14:20.000Z"), Value::Null));
        assert_eq!((fresh["lastRunStatus"].clone(), fresh["lastRunError"].clone(), fresh["runCount"].clone()), (json!("failed"), json!("boom"), json!(2)));
        assert_eq!(fresh["webhook"], json!({"path": "/hooks/v1/schedules/s1", "url": "/hooks/v1/schedules/s1/tok", "hasSecret": false}));
        assert_eq!(later["webhook"]["url"], Value::Null);
        // And a task the phone never saw has the defaults, and one that is no webhook has no webhook
        let mut bare = task.clone();
        bare["meta"] = Value::Null;
        bare["webhook"] = Value::Null;
        bare["schedule"] = json!({"type": "interval", "every_ms": 60_000});
        let shown = task_from_agent(&bare, None);
        assert_eq!((shown["modelSelection"].clone(), shown["workspaceStrategy"].clone(), shown.get("webhook").is_none()), (json!({"instanceId": "claudeAgent", "model": "opus"}), json!({"type": "root"}), true));
    }
}
