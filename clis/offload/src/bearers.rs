//! `wf offload bearers mint|list|revoke`: the device bearers of the rca service's agent API socket (`/agent/v1/socket`), administered with the
//! service secret over the tailnet like the service's other operator verbs (`wf offload sessions rm`). A bearer is minted once, shown once, and
//! kept by the service as a digest; it opens the socket and nothing else. Every verb prints one JSON object: `{ok: true, ...}` or
//! `{ok: false, error, next}` with exit 1; a malformed line exits 2.
use crate::rca_client;
use serde_json::{json, Value};
use std::time::Duration;
use wf_kit::args::{parse_spec, Positional, Spec};

const TIMEOUT: Duration = Duration::from_secs(60);
const MINT: Spec = Spec { values: &["label"], positional: Positional::None, ..Spec::NONE };
const LIST: Spec = Spec { positional: Positional::None, ..Spec::NONE };
const REVOKE: Spec = Spec { positional: Positional::Exactly(1), ..Spec::NONE };

/// What a mint tells its reader to do with the token it prints.
const STORE_IT: &str = "store the token now, it is not shown again: create it in the vault under the key of the app that holds it (T3_RCA_BEARER for wf t3 serve); wf offload bearers revoke <id> ends it";

/// `wf offload bearers <verb> ...`: the words after `bearers`.
pub fn run(rest: &[String]) -> Option<i32> {
    let (verb, more) = rest.split_first()?;
    match verb.as_str() {
        "mint" => {
            let parsed = parse_spec(more, &MINT)?;
            let Some(label) = parsed.val("label").map(str::trim).filter(|l| !l.is_empty()) else {
                wf_kit::wf_err!("wf offload bearers mint: --label <name> is required");
                return Some(2);
            };
            let body = json!({ "label": label });
            Some(ask("mint", "POST", "/agent/v1/bearers", Some(body), 201, |answer| json!({"ok": true, "id": answer["id"], "label": answer["label"], "createdAt": answer["createdAt"], "token": answer["token"], "next": STORE_IT})))
        }
        "list" => {
            parse_spec(more, &LIST)?;
            Some(ask("list", "GET", "/agent/v1/bearers", None, 200, |answer| json!({"ok": true, "bearers": answer["bearers"]})))
        }
        "revoke" => {
            let parsed = parse_spec(more, &REVOKE)?;
            let id = parsed.positional.first()?;
            let path = format!("/agent/v1/bearers/{}", cli_http::encode_uri_component(id));
            Some(ask("revoke", "DELETE", &path, None, 200, |answer| json!({"ok": true, "revoked": answer["revoked"]})))
        }
        _ => None,
    }
}

/// One request to the service: its answer shaped by `shown` when its status is `expected`, else the failure that names what to do.
fn ask(verb: &str, method: &str, path: &str, body: Option<Value>, expected: u16, shown: impl Fn(&Value) -> Value) -> i32 {
    match rca_client::send(method, path, body.as_ref(), TIMEOUT) {
        Ok(answered) if answered.status == expected => {
            println!("{}", shown(&answered.body));
            0
        }
        Ok(answered) => {
            let reason = answered.body["error"].as_str().unwrap_or("no reason given");
            let next = match answered.status {
                401 => "the service refused the service secret: CHAT_AGENT_API_SECRET must be the running service's",
                404 => "wf offload bearers list shows the ids that exist",
                _ => "see the service's log (wf vm ops logs remote-chat-agent)",
            };
            fail(verb, json!({"ok": false, "error": reason, "status": answered.status, "code": answered.body["code"], "next": next}))
        }
        Err(why) => fail(verb, json!({"ok": false, "error": why, "next": "bring the tailnet up (wf vm net up) and check the service (wf vm ops stats)"})),
    }
}

fn fail(verb: &str, answer: Value) -> i32 {
    wf_kit::logfmt::mcp("error", "offload", &format!("wf offload bearers {verb} failed"), &[("error", answer["error"].as_str().unwrap_or_default().to_string())]);
    println!("{answer}");
    1
}
