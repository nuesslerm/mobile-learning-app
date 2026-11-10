use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

/// One recorded run of an action: the programs it starts, what each prints, the request and the response the executor must give.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    pub name: String,
    /// Files written into the executor's root before the request (`path` to content).
    #[serde(default)]
    pub files: std::collections::BTreeMap<String, String>,
    /// What `gh`, `glab` and `git` print for each argv the request starts, in the replay double's rule format.
    #[serde(default)]
    pub cli: Vec<Value>,
    /// `{id, tool, args}`
    pub request: Value,
    /// The argv of each start the request makes, in order, as the double logs it (`program` then arguments).
    #[serde(default)]
    pub starts: Vec<Vec<String>>,
    /// `{id, ok, result}` or `{id, ok: false, error}`
    pub response: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub action: String,
    pub exchanges: Vec<Exchange>,
}

#[derive(Deserialize)]
struct Step {
    dir: String,
    #[serde(default)]
    channel: Option<String>,
    payload: Value,
}

#[derive(Deserialize)]
struct Interaction {
    id: String,
    op: String,
    input: Value,
    steps: Vec<Step>,
    effects: Vec<Value>,
}

/// The cassette set of this crate (`tests/cassettes/exec-hosted`, a mimic set of kind `imported`: no run of `gh` or `glab` made it).
pub fn cassette_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cassettes/exec-hosted")
}

fn exchange(i: Interaction) -> Exchange {
    let mut cli: Vec<serde_json::Map<String, Value>> = Vec::new();
    let mut response = None;
    for s in &i.steps {
        let key = match s.dir.as_str() {
            "call" => {
                let program = s.channel.clone().unwrap_or_else(|| panic!("{}: a call without a program", i.id));
                cli.push(serde_json::Map::from_iter([("program".to_string(), Value::String(program)), ("args".to_string(), s.payload["args"].clone())]));
                continue;
            }
            "s2c" => {
                response = Some(s.payload.clone());
                continue;
            }
            "stdin" => "stdin",
            "stdout" => "stdout",
            "stderr" => "stderr",
            "exit" => "exit_code",
            other => panic!("{}: a {other} step is not part of a hosted interaction", i.id),
        };
        cli.last_mut().unwrap_or_else(|| panic!("{}: a {key} step comes before its call", i.id)).insert(key.to_string(), s.payload.clone());
    }
    let starts = i.effects.iter().find(|e| e["type"] == "cli-starts").map(|e| e["starts"].clone()).unwrap_or_else(|| panic!("{}: no cli-starts effect", i.id));
    Exchange {
        name: i.input["name"].as_str().unwrap_or_else(|| panic!("{}: input has no name", i.id)).to_string(),
        files: serde_json::from_value(i.input.get("files").cloned().unwrap_or_else(|| Value::Object(Default::default()))).unwrap_or_else(|e| panic!("{}: files: {e}", i.id)),
        cli: cli.into_iter().map(Value::Object).collect(),
        request: i.input["request"].clone(),
        starts: serde_json::from_value(starts).unwrap_or_else(|e| panic!("{}: starts: {e}", i.id)),
        response: response.unwrap_or_else(|| panic!("{}: no response step", i.id)),
    }
}

/// Every action of the cassette set, in the order of the set (the actions' name order); a line that is not an interaction stops the run naming it.
pub fn fixtures() -> Vec<Fixture> {
    let path = cassette_dir().join("interactions.jsonl");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut out: Vec<Fixture> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let i: Interaction = serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {line}: {e}", path.display()));
        let (action, x) = (i.op.clone(), exchange(i));
        match out.iter_mut().find(|f| f.action == action) {
            Some(f) => f.exchanges.push(x),
            None => out.push(Fixture { action, exchanges: vec![x] }),
        }
    }
    out
}
