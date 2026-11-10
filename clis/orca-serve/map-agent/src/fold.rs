//! rca's neutral thread events folded into the phone's journal render items. Pure: events in, items out.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The head of a tool output the phone is sent; the whole item comes from `agentSession.item`.
const HEAD_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub turn: Option<String>,
    /// `seq` of the event that opened the item, and of the newest that changed it (the item's revision).
    pub sequence: u64,
    pub revision: u64,
    pub body: Value,
}

impl Item {
    /// The item as the phone's `AgentJournalRenderItem`. rca's events carry no clock, so `observedAt` is 0.
    pub fn render(&self) -> Value {
        let mut v = json!({"itemId": self.id, "revision": self.revision, "body": self.body, "sequence": self.sequence, "observedAt": 0, "turnScope": {"kind": "thread"}});
        if let Some(turn) = &self.turn {
            v["turnScope"] = json!({"kind": "turn", "turnId": turn});
        }
        v
    }

    pub fn is_pending_prompt(&self) -> bool {
        matches!(self.body["kind"].as_str(), Some("approval" | "question")) && self.body["resolution"]["state"] == "pending"
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Folded {
    pub items: Vec<Item>,
    pub last_seq: u64,
    pub running_turn: Option<String>,
}

impl Folded {
    pub fn pending_prompt_ids(&self) -> Vec<String> {
        self.items.iter().filter(|i| i.is_pending_prompt()).map(|i| i.id.clone()).collect()
    }

    /// The newest turn record, as `AgentSessionLatestTurn`.
    pub fn latest_turn(&self) -> Value {
        match self.items.iter().rev().find(|i| i.body["kind"] == "turn") {
            Some(i) => json!({"itemId": i.id, "observedAt": 0, "turn": {"turnId": i.body["turnId"], "state": i.body["state"]}}),
            None => Value::Null,
        }
    }
}

pub fn bounded(text: &str) -> Value {
    let mut end = text.len().min(HEAD_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let digest: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    json!({"head": &text[..end], "byteLength": text.len(), "digest": digest, "truncated": end < text.len()})
}

fn text_block(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

pub fn approval_options() -> Value {
    json!([{"id": "accept", "label": "Allow"}, {"id": "accept_for_session", "label": "Allow for this session"}, {"id": "decline", "label": "Deny"}, {"id": "cancel", "label": "Deny and stop"}])
}

/// Folds `events` (the `event` objects of `thread.history`, oldest first) into items.
pub fn fold(events: &[Value]) -> Folded {
    let mut out = Folded::default();
    let mut tool_outputs: Vec<(String, String)> = Vec::new();
    for e in events {
        let seq = e["seq"].as_u64().unwrap_or(0);
        let turn = e["turn"].as_str().map(str::to_string);
        out.last_seq = out.last_seq.max(seq);
        match e["type"].as_str().unwrap_or_default() {
            "turn.started" => {
                out.running_turn = turn.clone();
                let id = format!("turn:{}", turn.clone().unwrap_or_default());
                out.items.push(Item { id, turn: turn.clone(), sequence: seq, revision: seq, body: json!({"kind": "turn", "turnId": turn, "state": "running"}) });
            }
            "turn.finished" => {
                let state = if e["reason"] == "interrupted" { "interrupted" } else { "completed" };
                if out.running_turn == turn {
                    out.running_turn = None;
                }
                let id = format!("turn:{}", turn.clone().unwrap_or_default());
                if let Some(i) = out.items.iter_mut().find(|i| i.id == id) {
                    i.body["state"] = state.into();
                    i.revision = seq;
                }
            }
            kind @ ("message.delta" | "reasoning.delta") => {
                let role = if kind == "reasoning.delta" { "reasoning" } else { e["role"].as_str().unwrap_or("assistant") };
                let text = e["text"].as_str().unwrap_or_default();
                match out.items.last_mut() {
                    Some(i) if i.turn == turn && i.body["kind"] == "message" && i.body["role"] == role => {
                        let joined = format!("{}{text}", i.body["blocks"][0]["text"].as_str().unwrap_or_default());
                        i.body["blocks"] = json!([text_block(&joined)]);
                        i.revision = seq;
                    }
                    _ => {
                        let id = format!("i{seq}");
                        if role == "user" {
                            let turn_id = format!("turn:{}", turn.clone().unwrap_or_default());
                            if let Some(t) = out.items.iter_mut().find(|i| i.id == turn_id) {
                                t.body["userItemId"] = id.clone().into();
                            }
                        }
                        out.items.push(Item { id, turn, sequence: seq, revision: seq, body: json!({"kind": "message", "role": role, "blocks": [text_block(text)]}) });
                    }
                }
            }
            "tool.started" => {
                let call = e["call"].as_str().unwrap_or_default().to_string();
                out.items.push(Item { id: format!("tool:{call}"), turn, sequence: seq, revision: seq, body: json!({"kind": "tool-call", "name": e["tool"], "input": e["args"], "callId": call, "state": "running"}) });
            }
            "tool.output" => {
                let call = e["call"].as_str().unwrap_or_default().to_string();
                let data = e["data"].as_str().unwrap_or_default();
                match tool_outputs.iter_mut().find(|(c, _)| *c == call) {
                    Some((_, o)) => o.push_str(data),
                    None => tool_outputs.push((call.clone(), data.to_string())),
                }
                if let Some(i) = out.items.iter_mut().find(|i| i.id == format!("tool:{call}")) {
                    i.body["output"] = bounded(&tool_outputs.iter().find(|(c, _)| *c == call).map(|(_, o)| o.clone()).unwrap_or_default());
                    i.revision = seq;
                }
            }
            "tool.finished" => {
                let call = e["call"].as_str().unwrap_or_default();
                if let Some(i) = out.items.iter_mut().find(|i| i.id == format!("tool:{call}")) {
                    i.body["state"] = if e["ok"] == false { "failed" } else { "completed" }.into();
                    i.revision = seq;
                }
            }
            "file.changed" => {
                let text = format!("{} {}", e["op"].as_str().unwrap_or("edit"), e["path"].as_str().unwrap_or_default());
                out.items.push(Item { id: format!("i{seq}"), turn, sequence: seq, revision: seq, body: json!({"kind": "status", "text": text}) });
            }
            "plan.updated" => {
                let id = format!("plan:{}", turn.clone().unwrap_or_default());
                let body = json!({"kind": "status", "presentation": "plan", "text": e["plan"]});
                match out.items.iter_mut().find(|i| i.id == id) {
                    Some(i) => {
                        i.body = body;
                        i.revision = seq;
                    }
                    None => out.items.push(Item { id, turn, sequence: seq, revision: seq, body }),
                }
            }
            "approval.requested" => {
                let approval = e["approval"].as_str().unwrap_or_default();
                let tool = e["tool"].as_str().unwrap_or_default();
                let body = json!({"kind": "approval", "title": tool, "detail": e["args"].to_string(), "options": approval_options(), "resolution": {"state": "pending", "selectedOptionId": null, "resolvedBy": null, "resolvedAt": null}});
                out.items.push(Item { id: format!("approval:{approval}"), turn, sequence: seq, revision: seq, body });
            }
            "input.requested" => {
                let request = e["request"].as_str().unwrap_or_default();
                let options: Vec<Value> = e["options"].as_array().map(|o| o.iter().filter_map(Value::as_str).map(|s| json!({"id": s, "label": s})).collect()).unwrap_or_default();
                let body = json!({"kind": "question", "question": e["prompt"], "options": options, "resolution": {"state": "pending", "selectedOptionId": null, "resolvedBy": null, "resolvedAt": null}});
                out.items.push(Item { id: format!("question:{request}"), turn, sequence: seq, revision: seq, body });
            }
            "approval.answered" => resolve(&mut out, &format!("approval:{}", e["approval"].as_str().unwrap_or_default()), "resolved", e["decision"].clone(), seq),
            "input.answered" => resolve(&mut out, &format!("question:{}", e["request"].as_str().unwrap_or_default()), "resolved", e["answer"].clone(), seq),
            "input.dismissed" => resolve(&mut out, &format!("question:{}", e["request"].as_str().unwrap_or_default()), "cancelled", Value::Null, seq),
            "error" => {
                let text = format!("{}: {}", e["code"].as_str().unwrap_or_default(), e["message"].as_str().unwrap_or_default());
                out.items.push(Item { id: format!("i{seq}"), turn, sequence: seq, revision: seq, body: json!({"kind": "status", "text": text, "tone": "error"}) });
            }
            _ => {}
        }
    }
    out
}

fn resolve(out: &mut Folded, id: &str, state: &str, selected: Value, seq: u64) {
    if let Some(i) = out.items.iter_mut().find(|i| i.id == id) {
        i.body["resolution"] = json!({"state": state, "selectedOptionId": selected, "resolvedBy": "rca", "resolvedAt": null});
        i.revision = seq;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events() -> Vec<Value> {
        include_str!("../../../../libraries/agent-api/agent-api-agent/tests/fixtures/thread-events.jsonl").lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["event"].clone()).collect()
    }

    #[test]
    fn given_the_recorded_events_of_two_turns_when_folded_then_each_event_is_in_an_item_whose_revision_is_its_last_event_and_the_unanswered_prompts_are_pending() {
        // Given the recorded events of thread t1 (twenty events, two turns)
        let recorded = events();
        // When folded
        let folded = fold(&recorded);
        // Then the head is the last event, no turn runs, and the revision of every item is a seq of the log at or after its opening
        assert_eq!((folded.last_seq, folded.running_turn.clone()), (recorded.len() as u64, None));
        assert!(folded.items.iter().all(|i| i.sequence <= i.revision && i.revision <= folded.last_seq));
        // And the tool call c2 is failed with its stderr, and the approval and the question nobody answered are pending
        let c2 = folded.items.iter().find(|i| i.id == "tool:c2").unwrap();
        assert_eq!((c2.body["state"].clone(), c2.body["output"]["head"].clone()), (json!("failed"), json!("1 failing\n")));
        assert_eq!(folded.pending_prompt_ids(), vec!["approval:ap1".to_string(), "question:rq1".to_string()]);
        // And two assistant deltas of one turn are one message only when nothing came between them
        let assistant: Vec<&str> = folded.items.iter().filter(|i| i.body["role"] == "assistant").map(|i| i.body["blocks"][0]["text"].as_str().unwrap()).collect();
        assert_eq!(assistant, vec!["I will read the handler.", "Fixed."]);
    }

    #[test]
    fn given_an_answer_and_a_dismissal_in_the_log_when_folded_then_the_prompts_are_resolved_and_cancelled_and_no_longer_current() {
        // Given the recorded log followed by the answer to the approval and the dismissal of the question
        let mut log = events();
        log.push(json!({"seq": 21, "thread": "t1", "turn": "u2", "type": "approval.answered", "approval": "ap1", "decision": "accept"}));
        log.push(json!({"seq": 22, "thread": "t1", "turn": "u2", "type": "input.dismissed", "request": "rq1"}));
        // When folded
        let folded = fold(&log);
        // Then both prompts left the pending set, with the states the phone reads
        assert!(folded.pending_prompt_ids().is_empty());
        let approval = folded.items.iter().find(|i| i.id == "approval:ap1").unwrap();
        assert_eq!((approval.body["resolution"]["state"].clone(), approval.body["resolution"]["selectedOptionId"].clone(), approval.revision), (json!("resolved"), json!("accept"), 21));
        assert_eq!(folded.items.iter().find(|i| i.id == "question:rq1").unwrap().body["resolution"]["state"], "cancelled");
    }

    #[test]
    fn given_a_tool_output_past_the_head_limit_when_folded_then_the_phone_gets_the_head_the_true_length_and_a_truncated_mark() {
        // Given a tool that printed 20 KiB
        let big = "x".repeat(20 * 1024);
        let log = vec![json!({"seq": 1, "turn": "u", "type": "tool.started", "call": "c", "tool": "t", "args": {}}), json!({"seq": 2, "turn": "u", "type": "tool.output", "call": "c", "stream": "stdout", "data": big})];
        // When folded
        let out = &fold(&log).items[0].body["output"];
        // Then
        assert_eq!((out["head"].as_str().unwrap().len(), out["byteLength"].clone(), out["truncated"].clone(), out["digest"].as_str().unwrap().len()), (8 * 1024, json!(20 * 1024), json!(true), 64));
    }
}
