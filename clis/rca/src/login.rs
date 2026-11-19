//! `wf rca login start|respond|complete|cancel|status|logout`: the agent CLIs' own logins at the workspace host, run over the rca service's agent
//! socket (`provider.auth.*`, `provider.list`, `provider.logout`) with no SSH and no T3 or Orca in between. The verb opens the socket with a
//! device bearer minted for the call with the service secret (or `RCA_AGENT_BEARER` when set) and revokes it after. Every verb prints one JSON
//! line, `{ok: true, ...}` or `{ok: false, error, code, next}` with exit 1; a malformed line exits 2.
use crate::rca_client;
use serde_json::{json, Value};
use std::net::TcpStream;
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};
use wf_kit::args::{parse_spec, Positional, Spec};

const TIMEOUT: Duration = Duration::from_secs(60);
const START: Spec = Spec { values: &["method"], positional: Positional::Exactly(1), ..Spec::NONE };
const RESPOND: Spec = Spec { positional: Positional::Exactly(2), ..Spec::NONE };
const COMPLETE: Spec = Spec { values: &["wait"], positional: Positional::Exactly(1), ..Spec::NONE };
const ONE: Spec = Spec { positional: Positional::Exactly(1), ..Spec::NONE };
const NONE: Spec = Spec { positional: Positional::None, ..Spec::NONE };

fn authority() -> String {
    let host = std::env::var("RCA_HOST").ok().filter(|h| !h.is_empty()).unwrap_or_else(|| "remote-chat-agent-prod".into());
    let port = std::env::var("RCA_PORT").ok().filter(|p| !p.is_empty()).unwrap_or_else(|| "8787".into());
    format!("{host}:{port}")
}

/// The bearer the socket is opened with, and the id to revoke after when it was minted here.
fn bearer() -> Result<(String, Option<String>), String> {
    if let Some(token) = std::env::var("RCA_AGENT_BEARER").ok().filter(|t| !t.is_empty()) {
        return Ok((token, None));
    }
    let answered = rca_client::send("POST", "/agent/v1/bearers", Some(&json!({ "label": "wf rca login" })), TIMEOUT)?;
    if answered.status != 201 {
        return Err(format!("the service refused to mint a bearer ({}): {}", answered.status, answered.body["error"].as_str().unwrap_or("no reason given")));
    }
    Ok((answered.body["token"].as_str().unwrap_or_default().to_string(), answered.body["id"].as_str().map(str::to_string)))
}

struct Socket(WebSocket<TcpStream>);

impl Socket {
    fn open(bearer: &str) -> Result<Socket, String> {
        let authority = authority();
        let stream = TcpStream::connect(&authority).map_err(|e| format!("the rca service at {authority} is unreachable: {e}"))?;
        stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
        let mut request = format!("ws://{authority}/agent/v1/socket").into_client_request().map_err(|e| e.to_string())?;
        request.headers_mut().insert("authorization", format!("Bearer {bearer}").parse().map_err(|_| "the bearer is not a valid header value".to_string())?);
        let (socket, _) = tungstenite::client(request, stream).map_err(|e| format!("the agent socket refused the handshake: {e}"))?;
        Ok(Socket(socket))
    }

    /// One request and its result; the service's refusal is `Err((code, message))`.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, (String, String)> {
        let id = 1;
        let io = |e: tungstenite::Error| ("IO".to_string(), e.to_string());
        self.0.send(Message::Text(json!({ "id": id, "method": method, "params": params }).to_string().into())).map_err(io)?;
        loop {
            let Message::Text(text) = self.0.read().map_err(io)? else { continue };
            let Ok(frame) = serde_json::from_str::<Value>(&text) else { continue };
            if frame["id"] != json!(id) {
                continue;
            }
            return match frame.get("error") {
                Some(error) => Err((error["code"].as_str().unwrap_or("ERROR").to_string(), error["message"].as_str().unwrap_or_default().to_string())),
                None => Ok(frame["result"].clone()),
            };
        }
    }
}

/// `wf rca login <verb> ...`: the words after `login`.
pub fn run(rest: &[String]) -> Option<i32> {
    let (verb, more) = rest.split_first()?;
    let (method, params): (&str, Value) = match verb.as_str() {
        "start" => {
            let parsed = parse_spec(more, &START)?;
            let mut params = json!({ "provider": parsed.positional.first()? });
            if let Some(method) = parsed.val("method") {
                params["method"] = json!(method);
            }
            ("provider.auth.start", params)
        }
        "respond" => {
            let parsed = parse_spec(more, &RESPOND)?;
            ("provider.auth.respond", json!({ "auth": parsed.positional.first()?, "text": parsed.positional.get(1)? }))
        }
        "complete" => {
            let parsed = parse_spec(more, &COMPLETE)?;
            let wait: u64 = parsed.val("wait").map_or(Some(300), |w| w.parse().ok())?;
            ("provider.auth.complete", json!({ "auth": parsed.positional.first()?, "timeout_ms": wait * 1000 }))
        }
        "cancel" => ("provider.auth.cancel", json!({ "auth": parse_spec(more, &ONE)?.positional.first()? })),
        "status" => {
            parse_spec(more, &NONE)?;
            ("provider.list", json!({}))
        }
        "logout" => ("provider.logout", json!({ "provider": parse_spec(more, &ONE)?.positional.first()? })),
        _ => return None,
    };
    Some(ask(verb, method, params))
}

fn ask(verb: &str, method: &str, params: Value) -> i32 {
    let failed = |answer: Value| {
        wf_kit::logfmt::mcp("error", "rca", &format!("wf rca login {verb} failed"), &[("error", answer["error"].as_str().unwrap_or_default().to_string())]);
        println!("{answer}");
        1
    };
    let (token, minted) = match bearer() {
        Ok(pair) => pair,
        Err(why) => return failed(json!({"ok": false, "error": why, "code": "UNREACHABLE", "next": "bring the tailnet up (wf vm net up) and check the service (wf vm ops stats)"})),
    };
    let answered = Socket::open(&token).and_then(|mut socket| socket.call(method, params).map_err(|(code, message)| format!("{code}: {message}")).map(|r| (r, socket)));
    if let Some(id) = minted {
        let _ = rca_client::send("DELETE", &format!("/agent/v1/bearers/{}", cli_http::encode_uri_component(&id)), None, TIMEOUT);
    }
    match answered {
        Ok((result, _)) => {
            println!("{}", json!({ "ok": true, "result": result }));
            0
        }
        Err(why) => failed(json!({"ok": false, "error": why, "code": why.split(':').next().unwrap_or("ERROR"), "next": "wf rca login status shows each provider; a login ends with `complete` or `cancel`"})),
    }
}
