//! What an opencode run's two files mean: `lines.jsonl` reads back as its last 5000 non-empty lines, and the runs list newest first.
use crate::domain::js::new_date_ms;
use serde_json::{Map, Value};
use std::cmp::Reverse;

pub const MAX_LINES: usize = 5000;

/// The last [`MAX_LINES`] non-empty lines of a `lines.jsonl` text, which is split on `\n` alone.
pub fn last_lines(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.split('\n').filter(|line| !line.is_empty()).collect();
    lines[lines.len().saturating_sub(MAX_LINES)..].iter().map(|line| (*line).to_string()).collect()
}

/// `new Date(meta.startedAt ?? 0).getTime()`; a time that is no date counts as the epoch.
fn started_ms(meta: &Map<String, Value>) -> i64 {
    match meta.get("startedAt") {
        None | Some(Value::Null) => 0,
        Some(started) => new_date_ms(started).unwrap_or(0),
    }
}

/// Newest `startedAt` first; runs that started at the same time keep the order they came in.
pub fn newest_first(runs: &mut [Map<String, Value>]) {
    runs.sort_by_cached_key(|meta| Reverse(started_ms(meta)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(id: &str, started_at: Option<Value>) -> Map<String, Value> {
        let mut meta = Map::new();
        meta.insert("runId".to_string(), json!(id));
        if let Some(started_at) = started_at {
            meta.insert("startedAt".to_string(), started_at);
        }
        meta
    }

    fn ids(runs: &[Map<String, Value>]) -> Vec<&str> {
        runs.iter().map(|meta| meta["runId"].as_str().expect("a run id")).collect()
    }

    #[test]
    fn given_a_lines_file_when_read_then_empty_lines_go_and_only_the_last_5000_stay() {
        let lines: Vec<String> = (0..5003).map(|n| format!("{{\"n\":{n}}}")).collect();
        let text = format!("\n{}\n\n{}\n", lines[..5001].join("\n"), lines[5001..].join("\n\n"));

        assert_eq!(last_lines(&text), lines[3..]);
    }

    #[test]
    fn given_text_without_a_final_newline_or_with_carriage_returns_when_read_then_only_newlines_split() {
        assert_eq!(last_lines("a\r\nb"), ["a\r", "b"]);
        assert_eq!(last_lines("\n\n"), Vec::<String>::new());
        assert_eq!(last_lines(""), Vec::<String>::new());
        assert_eq!(last_lines("only"), ["only"]);
    }

    #[test]
    fn given_runs_when_listed_then_the_newest_start_comes_first_and_equal_starts_keep_their_order() {
        let mut runs = vec![
            run("a", Some(json!("2026-10-20T10:00:00.000Z"))),
            run("b", Some(json!("2026-10-21T09:00:00.000Z"))),
            run("c", Some(json!("2026-10-21T11:00:00.000Z"))),
            run("d", Some(json!("2026-10-21T09:00:00.000Z"))),
            run("e", Some(json!("2026-10-21T09:00:00.000Z"))),
        ];

        newest_first(&mut runs);

        assert_eq!(ids(&runs), ["c", "b", "d", "e", "a"]);
    }

    #[test]
    fn given_runs_with_no_usable_start_when_listed_then_they_count_as_the_epoch() {
        let mut runs = vec![
            run("missing", None),
            run("null", Some(Value::Null)),
            run("junk", Some(json!("not a date"))),
            run("object", Some(json!({}))),
            run("old", Some(json!("1970-01-01T00:00:01.000Z"))),
            run("number", Some(json!(5000))),
            run("new", Some(json!("2026-10-21T09:00:00.000Z"))),
        ];

        newest_first(&mut runs);

        assert_eq!(ids(&runs), ["new", "number", "old", "missing", "null", "junk", "object"]);
    }
}
