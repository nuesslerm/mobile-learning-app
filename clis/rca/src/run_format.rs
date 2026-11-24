//! What `opencode run` prints for a turn, from the events the service returns (the turn as `opencode run --format json` prints it):
//! the answer's text parts on stdout, the header and the tool trace on stderr. Tools this does not know print as one generic line.
use serde_json::Value;

const RESET: &str = "\x1b[0m";
const GRAY: &str = "\x1b[90m";

/// Each finished text part, as `opencode run` prints it: the part and a newline.
pub fn text_parts(events: &[Value]) -> Vec<String> {
    events.iter().filter(|e| e["type"] == "text").filter_map(|e| e["part"]["text"].as_str()).map(str::to_string).collect()
}

/// A block tool: its icon and title, the text it printed, then a reset line.
fn block(icon: &str, title: &str, body: &str) -> String {
    let body = body.trim_end();
    let body = if body.is_empty() { String::new() } else { format!("{body}\n") };
    format!("{RESET}{icon} {RESET}{title}\n{body}{RESET}\n")
}

/// A tool that takes one line.
fn inline(icon: &str, title: &str, note: &str) -> String {
    format!("{RESET}{icon} {RESET}{title}{GRAY}{note}{RESET}\n")
}

fn count_note(n: u64) -> String {
    format!(" {n} match{}", if n == 1 { "" } else { "es" })
}

fn tool_line(part: &Value) -> String {
    let tool = part["tool"].as_str().unwrap_or("tool");
    let state = &part["state"];
    let input = &state["input"];
    let title = state["title"].as_str().unwrap_or_default();
    let output = state["output"].as_str().unwrap_or_default();
    let pattern = input["pattern"].as_str().unwrap_or_default();
    match tool {
        "read" => block("→", &format!("Read {title}"), ""),
        "write" => block("←", &format!("Write {title}"), output),
        "edit" => block("←", &format!("Edit {title}"), state["metadata"]["diff"].as_str().unwrap_or(output)),
        "bash" => block("$", input["command"].as_str().unwrap_or(title), state["metadata"]["output"].as_str().unwrap_or(output)),
        "glob" => inline("✱", &format!("Glob \"{pattern}\""), &count_note(state["metadata"]["count"].as_u64().unwrap_or(0))),
        "grep" => inline("✱", &format!("Grep \"{pattern}\""), &count_note(state["metadata"]["matches"].as_u64().unwrap_or(0))),
        other => {
            let first = input.as_object().and_then(|o| o.values().find_map(Value::as_str)).unwrap_or_default();
            inline("⚙", format!("{other} {first}").trim_end(), "")
        }
    }
}

/// The header and the tool lines of the turn, in the order they happened.
pub fn trace(events: &[Value], agent: &str, model: &str) -> String {
    let short = model.rsplit('/').next().unwrap_or(model);
    let mut out = format!("{RESET}\n> {agent} · {short}\n{RESET}\n");
    for event in events.iter().filter(|e| e["type"] == "tool_use") {
        out.push_str(&tool_line(&event["part"]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, state: Value) -> Value {
        json!({ "type": "tool_use", "part": { "type": "tool", "tool": name, "state": state } })
    }

    #[test]
    fn given_the_tools_a_turn_used_when_it_is_traced_then_each_prints_as_opencode_run_prints_it() {
        // Given a read, a write, a bash call, a glob and a grep, and a turn that said DONE
        let events = vec![
            tool("read", json!({ "title": "a.txt", "input": { "filePath": "/w/a.txt" }, "output": "<content>" })),
            tool("write", json!({ "title": "b.txt", "input": {}, "output": "Wrote file successfully." })),
            tool("bash", json!({ "title": "cat b.txt", "input": { "command": "cat b.txt" }, "output": "ORIGINAL TEXT\n", "metadata": { "output": "ORIGINAL TEXT\n" } })),
            tool("glob", json!({ "title": "w", "input": { "pattern": "*.txt" }, "metadata": { "count": 1 } })),
            tool("grep", json!({ "title": "x", "input": { "pattern": "original" }, "metadata": { "matches": 2 } })),
            json!({ "type": "text", "part": { "text": "DONE" } }),
        ];
        // When the trace and the text are rendered
        let trace = trace(&events, "build", "opencode-go/deepseek-v4.1-flash");
        // Then the trace is byte for byte what `opencode run` printed for the same calls, and the text is its own part
        assert_eq!(
            trace,
            "\x1b[0m\n> build · deepseek-v4.1-flash\n\x1b[0m\n\x1b[0m→ \x1b[0mRead a.txt\n\x1b[0m\n\x1b[0m← \x1b[0mWrite b.txt\nWrote file successfully.\n\x1b[0m\n\x1b[0m$ \x1b[0mcat b.txt\nORIGINAL TEXT\n\x1b[0m\n\x1b[0m✱ \x1b[0mGlob \"*.txt\"\x1b[90m 1 match\x1b[0m\n\x1b[0m✱ \x1b[0mGrep \"original\"\x1b[90m 2 matches\x1b[0m\n"
        );
        assert_eq!(text_parts(&events), vec!["DONE".to_string()]);
    }
}
