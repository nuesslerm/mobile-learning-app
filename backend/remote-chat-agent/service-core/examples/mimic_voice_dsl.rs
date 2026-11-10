//! The candidate of the voice-edit DSL cassette (`rs/tests/cassettes/voice-edit-dsl`): the Rust port behind a `mimic-driver/1` process,
//! which `wf mimic replay` feeds the recorded inputs. `--tweak` plants a defect (a valid verdict loses its flag) so a test can show a
//! replay reaches the port.
use rca_service_core::domain::voice_dsl::{self as dsl, Command};
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};

const WHOLE_SOLUTION: &str = "write a two pointer solution";

fn limits(l: dsl::EditSizeLimits) -> Value {
    json!({"maxInsertedLines": l.max_inserted_lines, "maxInsertedLinesTotal": l.max_inserted_lines_total})
}

fn answer(op: &str, input: &Value, tweak: bool) -> Result<Value, String> {
    let text = |key: &str| input[key].as_str().ok_or_else(|| format!("{op}: input.{key} is not a string")).map(str::to_string);
    Ok(match op {
        "dsl.validate" => {
            let commands = input["commands"].as_array().ok_or("dsl.validate: input.commands is not a list")?;
            let commands: Vec<Command> = commands.iter().map(dsl::parse_command).collect::<Result<_, _>>()?;
            let size = if input["whole"].as_bool() == Some(true) { dsl::edit_size_limits_for(Some(WHOLE_SOLUTION)) } else { dsl::DEFAULT_EDIT_SIZE_LIMITS };
            let got = dsl::validate_commands(&text("code")?, &commands, size);
            let mut out = Map::new();
            if !(tweak && got.valid) {
                out.insert("valid".into(), json!(got.valid));
            }
            if let Some(reason) = got.reason {
                out.insert("reason".into(), json!(reason));
            }
            if let Some(index) = got.failed_command_index {
                out.insert("failedCommandIndex".into(), json!(index));
            }
            Value::Object(out)
        }
        "dsl.blocks" => dsl::complete_block_bodies(&dsl::parse_alternative(&input["alternative"])?, &text("code")?, &text("language")?).to_json(),
        "dsl.repair" => json!(dsl::repair_json_control_chars(&text("text")?)),
        "dsl.limits" => limits(dsl::edit_size_limits_for(Some(&text("transcript")?))),
        "dsl.normalize" => dsl::normalize_response(input["input"].clone()),
        _ => return Err(format!("unknown op {op}")),
    })
}

fn main() {
    let tweak = std::env::args().any(|a| a == "--tweak");
    let out = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.expect("stdin is text")).unwrap_or(Value::Null);
        let reply = if request.get("hello").is_some() {
            json!({"hello": {"driver": {"name": "rca-service-core/voice_dsl", "version": "1", "protocol": "mimic-driver/1"}, "coverage": false}})
        } else {
            match answer(request["op"].as_str().unwrap_or(""), &request["input"], tweak) {
                Ok(payload) => json!({"id": request["id"], "steps": [{"dir": "ret", "kind": "json", "payload": payload}], "effects": [], "outcome": {"status": "ok", "reference_accepts": true}}),
                Err(error) => json!({"id": request["id"], "error": error}),
            }
        };
        writeln!(out.lock(), "{reply}").expect("stdout is open");
    }
}
