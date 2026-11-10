//! `/api/source-control`: pull requests through the `gh` CLI, run in the caller's directory (`data/githubSourceControlGateway.ts`).

use super::{respond, truthy, Call};
use crate::api::auth::{authorised, unauthorized};
use crate::api::app::App;
use axum::extract::Path as UrlPath;
use axum::response::Response;
use serde_json::{json, Map, Value};
use std::path::Path;

const PR_FIELDS: &str = "number,title,url,state,headRefName,baseRefName,isDraft";

fn failed(message: &str) -> Response {
    respond(502, &json!({ "error": message, "code": "SOURCE_CONTROL_FAILED" }))
}

fn invalid(message: &str) -> Response {
    respond(400, &json!({ "error": message, "code": "INVALID_REQUEST" }))
}

async fn gh(app: &App, cwd: &str, args: &[&str]) -> Result<String, String> {
    app.commands.run("gh", args, Some(Path::new(cwd)), None).await.map(|o| o.stdout).map_err(|e| if e.stderr.is_empty() { e.message } else { e.stderr })
}

/// The seven summary fields; a field gh did not send is left out and a missing state reads as OPEN.
fn summary(raw: &Value) -> Value {
    let mut out = Map::new();
    for key in ["number", "title", "url"] {
        if let Some(v) = raw.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    out.insert("state".into(), raw.get("state").filter(|s| !s.is_null()).cloned().unwrap_or_else(|| json!("OPEN")));
    for key in ["headRefName", "baseRefName", "isDraft"] {
        if let Some(v) = raw.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    Value::Object(out)
}

pub async fn list(Call { app, req, .. }: Call) -> Response {
    if !authorised(&app, &req) {
        return unauthorized();
    }
    let Some(cwd) = req.query_param("cwd").filter(|c| !c.is_empty()) else {
        return invalid("Missing required parameter: cwd");
    };
    let state = req.query_param("state");
    let mut args = vec!["pr", "list", "--json", PR_FIELDS];
    if let Some(state) = state.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        args.extend(["--state", state]);
    }
    match gh(&app, &cwd, &args).await.and_then(|out| serde_json::from_str::<Value>(&out).map_err(|e| e.to_string())) {
        Ok(Value::Array(items)) => respond(200, &Value::Array(items.iter().map(summary).collect())),
        Ok(_) => failed("raw.map is not a function"),
        Err(message) => failed(&message),
    }
}

/// JavaScript's `Number(text)` for the forms a path segment can take; None is NaN.
fn js_number(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    let radix = |prefix: &[&str], base: u32| prefix.iter().find_map(|p| t.strip_prefix(p)).and_then(|d| u64::from_str_radix(d, base).ok()).map(|n| n as f64);
    radix(&["0x", "0X"], 16)
        .or_else(|| radix(&["0o", "0O"], 8))
        .or_else(|| radix(&["0b", "0B"], 2))
        .or_else(|| match t {
            "Infinity" | "+Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            _ if t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) => t.parse::<f64>().ok(),
            _ => None,
        })
}

fn js_string(number: f64) -> String {
    if number.fract() == 0.0 && number.abs() < 1e21 {
        format!("{}", number as i128)
    } else {
        format!("{number}")
    }
}

pub async fn get(UrlPath(raw_number): UrlPath<String>, Call { app, req, .. }: Call) -> Response {
    let raw_number = raw_number.as_str();
    if !authorised(&app, &req) {
        return unauthorized();
    }
    let cwd = req.query_param("cwd").filter(|c| !c.is_empty());
    let number = js_number(raw_number).filter(|n| n.is_finite());
    let (Some(cwd), Some(number)) = (cwd, number) else {
        return invalid("Missing/invalid parameters: cwd, number");
    };
    match gh(&app, &cwd, &["pr", "view", &js_string(number), "--json", PR_FIELDS]).await.and_then(|out| serde_json::from_str::<Value>(&out).map_err(|e| e.to_string())) {
        Ok(raw) => respond(200, &summary(&raw)),
        Err(message) => failed(&message),
    }
}

pub async fn create(Call { app, req, body }: Call) -> Response {
    if !authorised(&app, &req) {
        return unauthorized();
    }
    let field = |name: &str| body.as_ref().and_then(|b| b.get(name)).filter(|v| truthy(v)).and_then(Value::as_str).map(str::to_string);
    let (Some(cwd), Some(head), Some(base), Some(title)) = (field("cwd"), field("head"), field("base"), field("title")) else {
        return invalid("Missing required fields: cwd, head, base, title");
    };
    let text = body.as_ref().and_then(|b| b.get("body")).and_then(Value::as_str).unwrap_or_default().to_string();
    let draft = body.as_ref().and_then(|b| b.get("draft")).is_some_and(truthy);
    let mut args = vec!["pr", "create", "--head", head.as_str(), "--base", base.as_str(), "--title", title.as_str(), "--body", text.as_str()];
    if draft {
        args.push("--draft");
    }
    match gh(&app, &cwd, &args).await {
        Ok(out) => {
            let url = out.trim().split('\n').next_back().unwrap_or_default().to_string();
            let number = url.split_once("/pull/").map(|(_, rest)| rest.chars().take_while(char::is_ascii_digit).collect::<String>()).and_then(|d| d.parse::<u64>().ok()).unwrap_or(0);
            respond(201, &json!({ "number": number, "title": title, "url": url, "state": "OPEN", "headRefName": head, "baseRefName": base, "isDraft": draft }))
        }
        Err(message) => failed(&message),
    }
}
