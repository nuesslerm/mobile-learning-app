//! `wf bucket state read|write`: the state ref (`crate::state`) for a workflow step or an agent.
use crate::state;
use serde_json::json;
use std::io::Read;
use wf_kit::args::{parse_spec, Positional, Spec};

const READ: Spec = Spec { positional: Positional::Exactly(1), ..Spec::NONE };
const WRITE: Spec = Spec { values: &["file", "message"], positional: Positional::Exactly(1), ..Spec::NONE };

pub fn run(args: &[String]) -> Option<i32> {
    match args {
        [state, verb, rest @ ..] if state == "state" && verb == "read" => read(rest),
        [state, verb, rest @ ..] if state == "state" && verb == "write" => write(rest),
        _ => None,
    }
}

fn fail(verb: &str, error: &str) -> Option<i32> {
    wf_kit::wf_err!("wf bucket state {verb}: {error}");
    println!("{}", json!({ "ok": false, "error": error }));
    Some(1)
}

fn read(args: &[String]) -> Option<i32> {
    let parsed = parse_spec(args, &READ)?;
    let path = parsed.positional.first()?;
    match state::read(path) {
        Ok(Some(content)) => println!("{}", json!({ "ok": true, "found": true, "path": path, "content": content })),
        Ok(None) => println!("{}", json!({ "ok": true, "found": false, "path": path })),
        Err(error) => return fail("read", &error),
    }
    Some(0)
}

fn write(args: &[String]) -> Option<i32> {
    let parsed = parse_spec(args, &WRITE)?;
    let path = parsed.positional.first()?;
    let body = match parsed.val("file") {
        Some(file) => std::fs::read(file).map_err(|e| format!("cannot read {file}: {e}")),
        None => {
            let mut body = Vec::new();
            std::io::stdin().read_to_end(&mut body).map(|_| body).map_err(|e| format!("cannot read stdin: {e}"))
        }
    };
    let body = match body {
        Ok(body) => body,
        Err(error) => return fail("write", &error),
    };
    let message = parsed.val("message").map(str::to_string).unwrap_or_else(|| format!("state: {path}"));
    match state::write(&[(path.as_str(), body.as_slice())], &message) {
        Ok(commit) => println!("{}", json!({ "ok": true, "path": path, "commit": commit, "ref": state::REF })),
        Err(error) => return fail("write", &error),
    }
    Some(0)
}
