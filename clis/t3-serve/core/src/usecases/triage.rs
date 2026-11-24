//! `server.prepareTriage`: a triage session for the phone to open as a thread: the playbook a coding agent follows and the service's own log, written
//! as files at the executor, and the composer text that links the playbook. The pin's server also clones the T3 Code source from a third party's
//! repository; this one does not (owner decision 7: no fixed third-party fetch), and what it diagnoses is the rca service this server runs over, so
//! the bundle holds only that service's own log (`service.logs`) and the facts the phone and this server give about themselves.
use super::queries::QueryFailure;
use super::service::Service;
use crate::ports::Op;
use serde_json::{json, Value};
use t3_contracts::time::iso;

/// The log lines the bundle carries.
const LOG_LINES: u32 = 500;

/// How the playbook names the client: the label, version, system and browser the client gave, as the pin's prompt does.
fn describe_client(client: &Value) -> String {
    let text = |key: &str| client.get(key).and_then(Value::as_str).filter(|v| !v.is_empty());
    let mut parts = vec![text("label").unwrap_or("an unknown client").to_string()];
    parts.extend(text("appVersion").map(|v| format!("version {v}")));
    parts.extend(text("os").map(|v| format!("on {v}")));
    parts.extend(text("browser").map(|v| format!("in {v}")));
    if text("webDeployment") == Some("hosted") {
        parts.push("(app.t3.codes)".into());
    }
    parts.join(" ")
}

fn failed(why: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": "ServerPrepareTriageError", "cause": why}))
}

impl Service {
    /// The playbook, written for an agent that reads the files and reports what it finds; `logs` says what the log file holds.
    fn triage_playbook(&self, client: &Value, logs: &str, at: &str) -> String {
        let env = self.env();
        format!(
            "# Triage session\n\nPrepared {at} for {client}.\n\n## What is being diagnosed\n\nThe phone is talking to `t3 serve` ({version}) on {label} ({os}, {arch}), which runs every action through an rca service \
             and the executor at the workspace host. Nothing else of this install is in this bundle: no T3 home, no T3 logs and no third-party source were fetched.\n\n## Files\n\n\
             - `service-logs.jsonl`: {logs}\n\n## What to do\n\n1. Read the person's description of the problem above these instructions.\n\
             2. Read `service-logs.jsonl`. Each line is one event the service wrote: `at_ms`, `level`, `event` and its fields. Look for `warn` and `error` events around the time of the problem.\n\
             3. Say what the events show, which part failed (the phone, `t3 serve`, the service or the executor) and what the person can try. Say plainly when the log does not hold the answer.\n\
             4. Do not change anything on this machine and do not send these files anywhere.\n",
            client = describe_client(client),
            version = env.server_version,
            label = env.label,
            os = env.os,
            arch = env.arch,
        )
    }

    pub fn prepare_triage(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let now = self.now_ms();
        let stamp = iso(now).replace([':', '.'], "-");
        let (lines, what) = match self.agent().run(&Op::ServiceLogs { limit: LOG_LINES, min_level: "info" }) {
            Ok(answer) => {
                let lines: Vec<String> = answer["lines"].as_array().into_iter().flatten().map(Value::to_string).collect();
                let what = format!("the last {} of the {} events the service keeps, oldest first", lines.len(), answer["kept"].as_u64().unwrap_or(lines.len() as u64));
                (lines, what)
            }
            Err(e) => (Vec::new(), format!("empty: the service's log could not be read ({})", e.message())),
        };
        let dir = format!("triage/{stamp}");
        let write = |name: &str, content: String| self.agent().run(&Op::FileWrite { root: String::new(), path: format!("{dir}/{name}"), content }).map(drop).map_err(|e| failed(&e.message()));
        write("service-logs.jsonl", lines.iter().map(|l| format!("{l}\n")).collect())?;
        write("triage-playbook.md", self.triage_playbook(&payload["client"], &what, &iso(now)))?;
        let root = self.root().map_err(|e| failed(&e))?;
        Ok(json!({"prompt": format!("Read [triage-playbook.md]({root}/{dir}/triage-playbook.md) and follow it. ")}))
    }
}
