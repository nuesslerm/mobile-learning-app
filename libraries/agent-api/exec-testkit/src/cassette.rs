//! A mimic cassette set (`cassette.json` header plus `interactions.jsonl`, docs/plans/mimic/plan.md) read for replay: the kit never records,
//! it plays what a set holds. A set says in its header how it was made (`provenance.kind`: `authored`, `imported` or `recorded`); only a set
//! of kind `recorded` is from a run of the reference CLI.
use crate::hosted::Rule;
use crate::script::{Step as ScriptStep, Turn};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub dir: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Interaction {
    pub id: String,
    #[serde(default)]
    pub op: Option<String>,
    #[serde(default)]
    pub input: Value,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub effects: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct Provenance {
    pub kind: String,
    pub note: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct Cassette {
    pub provenance: Provenance,
    pub interactions: Vec<Interaction>,
}

impl Cassette {
    /// The set in `dir`. A missing file, a header that is no header or an interaction that is not one stops the run naming it.
    pub fn open(dir: &Path) -> Cassette {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{}: {e}", dir.join(name).display()));
        let header: Value = serde_json::from_str(&read("cassette.json")).unwrap_or_else(|e| panic!("{}/cassette.json: {e}", dir.display()));
        let text = |k: &str| header["provenance"][k].as_str().unwrap_or_default().to_string();
        let interactions: Vec<Interaction> = read("interactions.jsonl").lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{}/interactions.jsonl: {l}: {e}", dir.display()))).collect();
        assert_eq!(header["interactions"].as_u64(), Some(interactions.len() as u64), "{}: the header counts other interactions than the file holds", dir.display());
        Cassette { provenance: Provenance { kind: text("kind"), note: text("note"), source: text("source") }, interactions }
    }

    pub fn interaction(&self, id: &str) -> &Interaction {
        self.interactions.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("no interaction {id}"))
    }
}

impl Interaction {
    /// The text of every `stdout` step with no channel: the lines a CLI printed, in order.
    pub fn lines(&self) -> Vec<String> {
        self.steps.iter().filter(|s| s.dir == "stdout" && s.channel.is_none()).filter_map(|s| s.payload.as_str().map(String::from)).collect()
    }

    /// The scripted CLI's turn that plays this interaction: it prints the stdout lines, writes the stderr and ends with the recorded exit code.
    pub fn turn(&self) -> Turn {
        let mut turn = Turn { steps: self.lines().into_iter().map(|text| ScriptStep::Line { text }).collect(), ..Turn::default() };
        for s in self.steps.iter().filter(|s| s.channel.is_none()) {
            match (s.dir.as_str(), &s.payload) {
                ("stderr", Value::String(text)) => turn.stderr.push_str(text),
                ("exit", Value::Number(code)) => turn.exit_code = code.as_i64().unwrap_or_default() as i32,
                _ => {}
            }
        }
        turn
    }

    /// The rules of the fake `gh` and `glab` that play this interaction: each `call` step opens one, the `stdin`, `stdout`, `stderr` and `exit`
    /// steps after it fill it.
    pub fn rules(&self) -> Vec<Rule> {
        let mut rules: Vec<Rule> = Vec::new();
        for s in &self.steps {
            let Some(program) = &s.channel else { continue };
            if s.dir == "call" {
                let args = s.payload["args"].as_array().unwrap_or_else(|| panic!("{}: a call without args", self.id)).iter().map(|a| a.as_str().unwrap_or_default().to_string()).collect();
                rules.push(Rule { program: program.clone(), args, ..Rule::default() });
                continue;
            }
            let rule = rules.last_mut().unwrap_or_else(|| panic!("{}: a {} step comes before its call", self.id, s.dir));
            let text = || s.payload.as_str().unwrap_or_default().to_string();
            match s.dir.as_str() {
                "stdin" => rule.stdin = Some(text()),
                "stdout" => rule.stdout = text(),
                "stderr" => rule.stderr = text(),
                "exit" => rule.exit_code = s.payload.as_i64().unwrap_or_default() as i32,
                other => panic!("{}: a {other} step is not part of a rule", self.id),
            }
        }
        rules
    }
}
