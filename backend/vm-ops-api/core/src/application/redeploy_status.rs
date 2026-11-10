//! `GET /redeploy/status`: the self-swap replaces this container, so its outcome is only readable from the host's files, through two
//! read-only siblings: the last redeploy's log and the redeploy history.
use crate::application::ports::Siblings;
use crate::application::scripts::redeploy::{RedeployEnv, REDEPLOY_HISTORY_FILE, REDEPLOY_LOG};
use crate::application::scripts::redeploy_status::{history_sibling_args, redeploy_status_sibling_args};
use crate::domain::js::{self, JsString};
use serde_json::Value;
use std::sync::Arc;

const LOG_UNITS: usize = 4000;
const STDERR_UNITS: usize = 2000;

/// What the two siblings read.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// The log sibling exited 0; the reply is 200 then, and 502 with the same body when it did not.
    pub ok: bool,
    pub log: JsString,
    pub stderr: JsString,
    pub history: Vec<Value>,
}

pub struct RedeployStatus {
    siblings: Arc<dyn Siblings>,
    env: RedeployEnv,
}

impl RedeployStatus {
    pub fn new(siblings: Arc<dyn Siblings>, env: RedeployEnv) -> Self {
        Self { siblings, env }
    }

    /// The history is read whatever the log's exit was; a history sibling that fails reads as empty.
    pub async fn read(&self) -> Status {
        let (dir, image) = (self.env.vm_dir.as_str(), self.env.sibling_image());
        let log = self.siblings.run(&redeploy_status_sibling_args(dir, &image, REDEPLOY_LOG)).await;
        let history = self.siblings.run(&history_sibling_args(dir, &image, REDEPLOY_HISTORY_FILE)).await;
        Status {
            ok: log.code == Some(0),
            log: JsString::from(log.stdout.as_str()).last(LOG_UNITS),
            stderr: JsString::from(log.stderr.as_str()).last(STDERR_UNITS),
            history: parse_history(&history.stdout),
        }
    }
}

/// The history's lines that parse to a truthy value, in order: a blank line, one that is no JSON and `null`, `0`, `false` and `""`
/// are dropped, as `.filter(Boolean)` drops them.
pub fn parse_history(raw: &str) -> Vec<Value> {
    let truthy = |value: &Value| match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    };
    raw.split('\n').map(|line| line.trim_matches(js::is_space)).filter(|line| !line.is_empty()).filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(truthy).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::SiblingResult;
    use crate::testing::{FakeSiblings, SiblingCall};
    use serde_json::json;

    const LAST: &str = r#"{"ts":"2026-10-21T09:00:00Z","key":"vm-ops-api","outcome":"redeploy.succeeded","exitCode":0}"#;

    fn status(siblings: FakeSiblings) -> (Arc<FakeSiblings>, RedeployStatus) {
        let siblings = Arc::new(siblings);
        let env = RedeployEnv { image_namespace: "ns".into(), vm_dir: "/home/ubuntu/vm-ops-api".into(), ..RedeployEnv::default() };
        (siblings.clone(), RedeployStatus::new(siblings, env))
    }

    #[test]
    fn given_history_text_when_parsed_then_only_truthy_json_lines_stay_in_order_and_each_is_trimmed() {
        let raw = format!("{{\"a\":1}}\n{{oops\n\n   {LAST}   \nnull\n0\n-0\nfalse\n\"\"\n42\n[]\n");

        let parsed = parse_history(&raw);

        assert_eq!(parsed, [json!({ "a": 1 }), serde_json::from_str::<Value>(LAST).expect("JSON"), json!(42), json!([])]);
    }

    #[tokio::test]
    async fn given_the_two_siblings_when_the_status_is_read_then_the_log_keeps_4000_units_and_the_history_is_parsed() {
        // Given a log of 5000 units, a stderr of 3000 and a history of one line, answered by two read-only siblings
        let siblings = FakeSiblings::new()
            .with_run("cat last-redeploy.log", vec![SiblingResult::exited(0, &"l".repeat(5000), &"e".repeat(3000))])
            .with_run("cat redeploy-history.jsonl", vec![SiblingResult::exited(0, &format!("{LAST}\n"), "")]);
        let (siblings, status) = status(siblings);

        // When the status is read
        let read = status.read().await;

        // Then the log and stderr are the last 4000 and 2000 units, the history parsed, and both siblings were read-only on the image
        assert_eq!((read.ok, read.log.len(), read.stderr.len(), read.history.len()), (true, 4000, 2000, 1));
        let argvs: Vec<Vec<String>> = siblings.calls().into_iter().map(|call| match call {
            SiblingCall::Run(args) | SiblingCall::Detached(args) => args,
        }).collect();
        assert_eq!(argvs.len(), 2);
        for args in &argvs {
            assert_eq!(&args[..8], ["run", "--rm", "-v", "/home/ubuntu/vm-ops-api:/home/ubuntu/vm-ops-api:ro", "-w", "/home/ubuntu/vm-ops-api", "ns/vm-ops-api-rs:latest", "sh"]);
        }
    }

    #[tokio::test]
    async fn given_a_log_sibling_that_fails_when_the_status_is_read_then_it_is_not_ok_and_the_history_is_still_read() {
        // Given a log sibling that exits 125 with no output, and a history sibling nothing answers
        let (siblings, status) = status(FakeSiblings::new().with_run("cat last-redeploy.log", vec![SiblingResult::exited(125, "", "no image")]));

        // When the status is read
        let read = status.read().await;

        // Then it is not ok, carries the stderr, has no history, and both siblings ran
        assert_eq!((read.ok, read.stderr.to_text_lossy(), read.history), (false, "no image".to_string(), Vec::new()));
        assert_eq!(siblings.calls().len(), 2);
    }
}
