//! The voice-edit DSL's server half (`@lib/voice-edit-dsl`): the command and alternative schemas, the lenient JSON the planner's
//! replies need, the field-name drift the models show, the size limits, the validation of an alternative against the code it edits,
//! and the pass body a python block header is given. Offsets are UTF-16 code units, as the client counts them, so the code is held
//! as `u16`s wherever an offset is read.

use crate::domain::schema::parse;
use serde::de::{self, Deserializer};
use serde::Deserialize;
use serde_json::{json, Map, Value};

/// An integer that is at least `MIN`.
fn at_least<'de, D: Deserializer<'de>, const MIN: i64>(deserializer: D) -> Result<i64, D::Error> {
    let number = i64::deserialize(deserializer)?;
    if number < MIN {
        return Err(de::Error::invalid_value(de::Unexpected::Signed(number), &format!("an integer of at least {MIN}").as_str()));
    }
    Ok(number)
}

fn non_negative<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    at_least::<D, 0>(deserializer)
}

fn positive<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    at_least::<D, 1>(deserializer)
}

fn non_empty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.is_empty() {
        return Err(de::Error::invalid_length(0, &"a string of at least 1 character"));
    }
    Ok(text)
}

fn unit_interval<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let number = f64::deserialize(deserializer)?;
    if !(0.0..=1.0).contains(&number) {
        return Err(de::Error::invalid_value(de::Unexpected::Float(number), &"a number from 0 to 1"));
    }
    Ok(number)
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    InsertAfterLine {
        #[serde(deserialize_with = "non_negative")]
        line: i64,
        #[serde(deserialize_with = "non_empty")]
        text: String,
    },
    ReplaceLines {
        #[serde(deserialize_with = "positive")]
        start_line: i64,
        #[serde(deserialize_with = "positive")]
        end_line: i64,
        new_text: String,
    },
    DeleteLines {
        #[serde(deserialize_with = "positive")]
        start_line: i64,
        #[serde(deserialize_with = "positive")]
        end_line: i64,
    },
    ReplaceRange {
        #[serde(deserialize_with = "non_negative")]
        start: i64,
        #[serde(deserialize_with = "non_negative")]
        end: i64,
        new_text: String,
    },
    InsertAt {
        #[serde(deserialize_with = "non_negative")]
        position: i64,
        text: String,
    },
    DeleteRange {
        #[serde(deserialize_with = "non_negative")]
        start: i64,
        #[serde(deserialize_with = "non_negative")]
        end: i64,
    },
    RenameSymbol {
        #[serde(deserialize_with = "non_negative")]
        position: i64,
        #[serde(deserialize_with = "non_empty")]
        new_name: String,
    },
    MoveRange {
        #[serde(deserialize_with = "non_negative")]
        start: i64,
        #[serde(deserialize_with = "non_negative")]
        end: i64,
        #[serde(deserialize_with = "non_negative")]
        insert_at: i64,
    },
    WrapWith {
        #[serde(deserialize_with = "non_negative")]
        start: i64,
        #[serde(deserialize_with = "non_negative")]
        end: i64,
        before: String,
        after: String,
    },
    ExtractBlock {
        #[serde(deserialize_with = "non_negative")]
        start: i64,
        #[serde(deserialize_with = "non_negative")]
        end: i64,
        #[serde(deserialize_with = "non_empty")]
        new_name: String,
    },
}

impl Command {
    pub fn to_json(&self) -> Value {
        match self {
            Self::InsertAfterLine { line, text } => json!({ "type": "insertAfterLine", "line": line, "text": text }),
            Self::ReplaceLines { start_line, end_line, new_text } => json!({ "type": "replaceLines", "startLine": start_line, "endLine": end_line, "newText": new_text }),
            Self::DeleteLines { start_line, end_line } => json!({ "type": "deleteLines", "startLine": start_line, "endLine": end_line }),
            Self::ReplaceRange { start, end, new_text } => json!({ "type": "replaceRange", "start": start, "end": end, "newText": new_text }),
            Self::InsertAt { position, text } => json!({ "type": "insertAt", "position": position, "text": text }),
            Self::DeleteRange { start, end } => json!({ "type": "deleteRange", "start": start, "end": end }),
            Self::RenameSymbol { position, new_name } => json!({ "type": "renameSymbol", "position": position, "newName": new_name }),
            Self::MoveRange { start, end, insert_at } => json!({ "type": "moveRange", "start": start, "end": end, "insertAt": insert_at }),
            Self::WrapWith { start, end, before, after } => json!({ "type": "wrapWith", "start": start, "end": end, "before": before, "after": after }),
            Self::ExtractBlock { start, end, new_name } => json!({ "type": "extractBlock", "start": start, "end": end, "newName": new_name }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alternative {
    #[serde(deserialize_with = "non_empty")]
    pub id: String,
    #[serde(deserialize_with = "non_empty")]
    pub label: String,
    #[serde(deserialize_with = "non_empty")]
    pub summary: String,
    #[serde(deserialize_with = "unit_interval")]
    pub confidence: f64,
    pub valid: Option<bool>,
    pub invalid_reason: Option<String>,
    pub commands: Vec<Command>,
}

impl Alternative {
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".into(), json!(self.id));
        out.insert("label".into(), json!(self.label));
        out.insert("summary".into(), json!(self.summary));
        out.insert("confidence".into(), json!(self.confidence));
        if let Some(valid) = self.valid {
            out.insert("valid".into(), json!(valid));
        }
        if let Some(reason) = &self.invalid_reason {
            out.insert("invalidReason".into(), json!(reason));
        }
        out.insert("commands".into(), Value::Array(self.commands.iter().map(Command::to_json).collect()));
        Value::Object(out)
    }
}

// ---- parsing: what the schemas accept ---------------------------------------------------------------------------------------

/// One command, as serde reads it (the error is serde's message).
pub fn parse_command(value: &Value) -> Result<Command, String> {
    parse(value)
}

/// `editAlternativeSchema`.
pub fn parse_alternative(value: &Value) -> Result<Alternative, String> {
    parse(value)
}

/// `voiceEditResponseSchema`: every alternative must parse, or the whole response is refused.
#[derive(Debug, Deserialize)]
pub struct Response {
    pub transcript: String,
    pub alternatives: Vec<Alternative>,
}

pub fn parse_response(value: &Value) -> Result<Response, String> {
    parse(value)
}

/// `editSourceSchema`: how the client produced the transcript.
pub fn edit_source(value: Option<&str>) -> Option<&'static str> {
    ["voice", "text", "plan-voice", "plan-text"].into_iter().find(|s| Some(*s) == value)
}

// ---- lenient JSON and field-name drift --------------------------------------------------------------------------------------

/// Escapes control characters that sit inside a string literal and leaves everything else byte for byte as it was.
pub fn repair_json_control_chars(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut in_string, mut escaped) = (false, false);
    for ch in text.chars() {
        if in_string {
            if escaped {
                out.push(ch);
                escaped = false;
                continue;
            }
            if ch == '\\' {
                out.push(ch);
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
                out.push(ch);
                continue;
            }
            let code = ch as u32;
            if code < 0x20 {
                match code {
                    0x0a => out.push_str("\\n"),
                    0x0d => out.push_str("\\r"),
                    0x09 => out.push_str("\\t"),
                    _ => out.push_str(&format!("\\u{code:04x}")),
                }
                continue;
            }
            out.push(ch);
            continue;
        }
        if ch == '"' {
            in_string = true;
        }
        out.push(ch);
    }
    out
}

/// `JSON.parse` that tolerates raw line breaks inside strings; the original error otherwise.
pub fn parse_lenient_json(text: &str) -> Result<Value, String> {
    match serde_json::from_str(text) {
        Ok(value) => Ok(value),
        Err(first) => {
            let repaired = repair_json_control_chars(text);
            if repaired == text {
                return Err(first.to_string());
            }
            serde_json::from_str(&repaired).map_err(|e| e.to_string())
        }
    }
}

/// The models settle on `newText` for insertAfterLine about as often as on `text`, and the other way round for replaceLines; both
/// name the same block. A document that conforms comes back equal.
pub fn normalize_response(raw: Value) -> Value {
    let Value::Object(mut envelope) = raw else { return raw };
    let Some(Value::Array(alternatives)) = envelope.get_mut("alternatives") else { return Value::Object(envelope) };
    for alternative in alternatives.iter_mut() {
        let Some(commands) = alternative.get_mut("commands").and_then(Value::as_array_mut) else { continue };
        for command in commands.iter_mut() {
            let Some(fields) = command.as_object_mut() else { continue };
            match fields.get("type").and_then(Value::as_str) {
                Some("insertAfterLine") if !fields.contains_key("text") && fields.get("newText").is_some_and(Value::is_string) => rename_key(fields, "newText", "text"),
                Some("replaceLines") if !fields.contains_key("newText") && fields.get("text").is_some_and(Value::is_string) => rename_key(fields, "text", "newText"),
                _ => {}
            }
        }
    }
    Value::Object(envelope)
}

fn rename_key(fields: &mut Map<String, Value>, from: &str, to: &str) {
    let rebuilt: Map<String, Value> = std::mem::take(fields).into_iter().map(|(k, v)| (if k == from { to.to_string() } else { k }, v)).collect();
    *fields = rebuilt;
}

// ---- size limits -------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditSizeLimits {
    pub max_inserted_lines: usize,
    pub max_inserted_lines_total: usize,
}

pub const DEFAULT_EDIT_SIZE_LIMITS: EditSizeLimits = EditSizeLimits { max_inserted_lines: 6, max_inserted_lines_total: 10 };

/// `/\b(?:write|implement|code|solve|create|build)\b[^.?!]*\b(?:solution|algorithm)\b/i`, by hand: the regex engine here has no
/// ASCII-only word boundary. A keyword and a later target word, with no sentence end between them.
fn whole_solution_ask(text: &str) -> bool {
    let mut words: Vec<(usize, usize, String)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        let word = c.is_ascii_alphanumeric() || c == '_';
        match (word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                words.push((s, i, text[s..i].to_ascii_lowercase()));
                start = None;
            }
            _ => {}
        }
    }
    words.iter().enumerate().filter(|(_, w)| ["write", "implement", "code", "solve", "create", "build"].contains(&w.2.as_str())).any(|(i, keyword)| {
        for later in &words[i + 1..] {
            if text[keyword.1..later.0].contains(['.', '?', '!']) {
                return false;
            }
            if later.2 == "solution" || later.2 == "algorithm" {
                return true;
            }
        }
        false
    })
}

/// "Write a two pointer solution" asks for everything in one breath, so such an ask gets the size of a first step.
pub fn edit_size_limits_for(transcript: Option<&str>) -> EditSizeLimits {
    match transcript {
        Some(t) if whole_solution_ask(t) => EditSizeLimits { max_inserted_lines: 3, max_inserted_lines_total: 3 },
        _ => DEFAULT_EDIT_SIZE_LIMITS,
    }
}

// ---- line commands, clamping and snapping -----------------------------------------------------------------------------------

type Text<'a> = &'a [u16];

const NL: u16 = b'\n' as u16;

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn is_ident(unit: Option<u16>) -> bool {
    unit.is_some_and(|u| matches!(u, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a | 0x5f))
}

fn at(text: Text, index: i64) -> Option<u16> {
    usize::try_from(index).ok().and_then(|i| text.get(i).copied())
}

fn line_starts(text: Text) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.iter().enumerate().filter(|(_, u)| **u == NL).map(|(i, _)| i + 1));
    starts
}

/// The offset command a line-anchored one means against `text`; out-of-range lines clamp to the buffer.
fn resolve_line_command(text: Text, command: &Command) -> Command {
    let starts = line_starts(text);
    let ends_with_newline = text.last() == Some(&NL);
    let line_count = std::cmp::max(1, starts.len() as i64 - i64::from(ends_with_newline));
    let end_of_line = |line: i64| -> i64 { starts.get(line as usize).map_or(text.len() as i64, |next| *next as i64 - 1) };
    let clamp_line = |line: i64| line.clamp(1, line_count);
    match command {
        Command::InsertAfterLine { line, text: block } => {
            let block = block.trim_end_matches('\n');
            if *line <= 0 {
                return Command::InsertAt { position: 0, text: format!("{block}\n") };
            }
            Command::InsertAt { position: end_of_line(clamp_line(*line)), text: format!("\n{block}") }
        }
        Command::ReplaceLines { start_line, end_line, new_text } => resolve_lines(text, &starts, clamp_line(*start_line.min(end_line)), clamp_line(*start_line.max(end_line)), end_of_line, Some(new_text)),
        Command::DeleteLines { start_line, end_line } => resolve_lines(text, &starts, clamp_line(*start_line.min(end_line)), clamp_line(*start_line.max(end_line)), end_of_line, None),
        other => other.clone(),
    }
}

fn resolve_lines(text: Text, starts: &[usize], a: i64, b: i64, end_of_line: impl Fn(i64) -> i64, replacement: Option<&String>) -> Command {
    let start = starts.get((a - 1) as usize).map_or(0, |s| *s as i64);
    let end = end_of_line(b);
    match replacement {
        Some(new_text) if !new_text.is_empty() => Command::ReplaceRange { start, end, new_text: new_text.trim_end_matches('\n').to_string() },
        _ => {
            // The newline after the block goes too, so no blank line is left behind; at the end of the file, the one before it.
            if end < text.len() as i64 {
                Command::DeleteRange { start, end: end + 1 }
            } else {
                Command::DeleteRange { start: (start - 1).max(0), end }
            }
        }
    }
}

fn clamp_command(len: i64, command: &Command) -> Command {
    let fit = |n: i64| n.clamp(0, len);
    match command {
        Command::InsertAt { position, text } => Command::InsertAt { position: fit(*position), text: text.clone() },
        Command::RenameSymbol { position, new_name } => Command::RenameSymbol { position: fit(*position), new_name: new_name.clone() },
        Command::MoveRange { start, end, insert_at } => Command::MoveRange { start: fit(*start), end: fit(*end), insert_at: fit(*insert_at) },
        Command::ReplaceRange { start, end, new_text } => Command::ReplaceRange { start: fit(*start), end: fit(*end), new_text: new_text.clone() },
        Command::DeleteRange { start, end } => Command::DeleteRange { start: fit(*start), end: fit(*end) },
        Command::WrapWith { start, end, before, after } => Command::WrapWith { start: fit(*start), end: fit(*end), before: before.clone(), after: after.clone() },
        Command::ExtractBlock { start, end, new_name } => Command::ExtractBlock { start: fit(*start), end: fit(*end), new_name: new_name.clone() },
        other => other.clone(),
    }
}

fn blank(text: Text) -> bool {
    text.iter().all(|u| *u == 0x20 || *u == 0x09)
}

fn at_line_boundary(text: Text, offset: i64) -> bool {
    if offset <= 0 || offset >= text.len() as i64 {
        return true;
    }
    at(text, offset - 1) == Some(NL) || at(text, offset) == Some(NL)
}

/// JavaScript's `lastIndexOf('\n', from)`: a negative start still looks at index 0.
fn last_newline_before(text: Text, from: i64) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    let from = from.max(0) as usize;
    (0..=from.min(text.len() - 1)).rev().find(|i| text[*i] == NL)
}

fn line_start_of(text: Text, offset: i64) -> i64 {
    last_newline_before(text, offset - 1).map_or(0, |i| i as i64 + 1)
}

fn line_end_of(text: Text, offset: i64) -> i64 {
    let from = usize::try_from(offset).unwrap_or(0);
    text.iter().enumerate().skip(from).find(|(_, u)| **u == NL).map_or(text.len() as i64, |(i, _)| i as i64)
}

pub use remote_chat_agent_render::js::is_js_space;

/// `/^[ \t\n]/.test(block)`.
fn starts_with_blank(block: &str) -> bool {
    block.starts_with([' ', '\t', '\n'])
}

/// `/\n[ \t]+\S/.test(block)`: a later line that is indented.
fn has_indented_later_line(block: &str) -> bool {
    let chars: Vec<char> = block.chars().collect();
    chars.iter().enumerate().any(|(i, c)| {
        if *c != '\n' {
            return false;
        }
        let mut j = i + 1;
        while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
            j += 1;
        }
        j > i + 1 && j < chars.len() && !is_js_space(chars[j])
    })
}

/// A block whose first line is bare while its later lines are indented lost its first line's indentation: it gets the one of the
/// line it lands on.
fn reindent_bare_first_line(text: Text, line_start: i64, block: &str) -> String {
    if starts_with_blank(block) || !has_indented_later_line(block) {
        return block.to_string();
    }
    let from = usize::try_from(line_start).unwrap_or(0).min(text.len());
    let indent: String = text[from..].iter().take_while(|u| **u == 0x20 || **u == 0x09).map(|u| char::from(*u as u8)).collect();
    format!("{indent}{block}")
}

fn snap_to_identifier(text: Text, offset: i64) -> i64 {
    if is_ident(at(text, offset)) {
        return offset;
    }
    let line_start = line_start_of(text, offset);
    let line_end = line_end_of(text, offset);
    for d in 1..=(line_end - line_start) {
        if offset - d >= line_start && is_ident(at(text, offset - d)) {
            return offset - d;
        }
        if offset + d < line_end && is_ident(at(text, offset + d)) {
            return offset + d;
        }
    }
    offset
}

fn slice(text: Text, from: i64, to: i64) -> Vec<u16> {
    let (from, to) = (from.clamp(0, text.len() as i64) as usize, to.clamp(0, text.len() as i64) as usize);
    if from >= to {
        Vec::new()
    } else {
        text[from..to].to_vec()
    }
}

fn snap_command(text: Text, command: Command) -> Command {
    match command {
        Command::RenameSymbol { position, new_name } => Command::RenameSymbol { position: snap_to_identifier(text, position), new_name },
        Command::InsertAt { position, text: block } => {
            if !block.contains('\n') {
                return Command::InsertAt { position, text: block };
            }
            let line_start = line_start_of(text, position);
            let line_end = line_end_of(text, position);
            if line_end > line_start && blank(&slice(text, line_start, line_end)) {
                // A whitespace-only line, the starter's indented stub body: fill it rather than start a new one.
                return Command::InsertAt { position: line_end, text: block.strip_prefix('\n').unwrap_or(&block).to_string() };
            }
            let starts_line = position == 0 || at(text, position - 1) == Some(NL);
            if starts_line {
                let trimmed = reindent_bare_first_line(text, position, block.trim_start_matches('\n'));
                let followed_by_content = position < text.len() as i64 && at(text, position) != Some(NL);
                let needs_trailing = followed_by_content && !trimmed.ends_with('\n');
                return Command::InsertAt { position, text: if needs_trailing { format!("{trimmed}\n") } else { trimmed } };
            }
            if !at_line_boundary(text, position) && blank(&slice(text, line_start, position)) {
                // Only indentation before the position: the block belongs above that line.
                return Command::InsertAt { position: line_start, text: if block.ends_with('\n') { block } else { format!("{block}\n") } };
            }
            let landed = if at_line_boundary(text, position) { position } else { line_end_of(text, position) };
            let after_line_content = landed > 0 && at(text, landed - 1) != Some(NL);
            let needs_newline = after_line_content && !block.starts_with('\n');
            Command::InsertAt { position: landed, text: if needs_newline { format!("\n{block}") } else { block } }
        }
        Command::ReplaceRange { start, end, new_text } => {
            if !new_text.contains('\n') {
                return Command::ReplaceRange { start, end, new_text };
            }
            let new_start = if at_line_boundary(text, start) { start } else { line_start_of(text, start) };
            let new_end = if at_line_boundary(text, end) { end } else { line_end_of(text, end) };
            let swallowed = slice(text, new_start, start);
            let swallowed_indent = !swallowed.is_empty() && swallowed.iter().all(|u| *u == 0x20 || *u == 0x09);
            let keep_indent = swallowed_indent && !starts_with_blank(&new_text);
            let new_text = if keep_indent { format!("{}{new_text}", String::from_utf16_lossy(&swallowed)) } else { reindent_bare_first_line(text, new_start, &new_text) };
            Command::ReplaceRange { start: new_start, end: new_end, new_text }
        }
        other => other,
    }
}

/// The half-open span a command reads from, in the original text's offsets.
fn command_span(command: &Command) -> (i64, i64) {
    match command {
        Command::InsertAt { position, .. } | Command::RenameSymbol { position, .. } => (*position, *position),
        Command::MoveRange { start, end, insert_at } => ((*start).min(*insert_at), (*end).max(*insert_at)),
        Command::ReplaceRange { start, end, .. } | Command::DeleteRange { start, end } | Command::WrapWith { start, end, .. } | Command::ExtractBlock { start, end, .. } => (*start, *end),
        Command::InsertAfterLine { .. } | Command::ReplaceLines { .. } | Command::DeleteLines { .. } => (0, 0),
    }
}

fn inserted_line_count(command: &Command) -> usize {
    let added = match command {
        Command::ReplaceRange { new_text, .. } => new_text.clone(),
        Command::InsertAt { text, .. } => text.clone(),
        Command::WrapWith { before, after, .. } => format!("{before}{after}"),
        _ => String::new(),
    };
    if added.is_empty() {
        0
    } else {
        added.split('\n').count()
    }
}

// ---- validation ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    pub valid: bool,
    pub reason: Option<String>,
    pub failed_command_index: Option<usize>,
}

fn check_range(len: i64, start: i64, end: i64, label: &str) -> Result<(), String> {
    if start > end {
        return Err(format!("{label} start ({start}) > end ({end})"));
    }
    if end > len {
        return Err(format!("{label} end ({end}) exceeds text length ({len})"));
    }
    if start < 0 || end < 0 {
        return Err(format!("{label} range contains negative index"));
    }
    Ok(())
}

fn validate_command(text: Text, command: &Command) -> Result<(), String> {
    let len = text.len() as i64;
    match command {
        Command::ReplaceRange { start, end, .. } => check_range(len, *start, *end, "replaceRange"),
        Command::InsertAt { position, .. } if *position > len => Err(format!("insertAt position {position} exceeds text length {len}")),
        Command::DeleteRange { start, end } => check_range(len, *start, *end, "deleteRange"),
        Command::RenameSymbol { position, .. } => {
            if *position > len {
                return Err(format!("renameSymbol position {position} exceeds text length {len}"));
            }
            // The position has been pulled onto the nearest identifier; with none to find, the rename would match the empty string.
            if !is_ident(at(text, *position)) {
                return Err(format!("renameSymbol position {position} is not on an identifier"));
            }
            Ok(())
        }
        Command::MoveRange { start, end, .. } => check_range(len, *start, *end, "moveRange"),
        Command::WrapWith { start, end, .. } => check_range(len, *start, *end, "wrapWith"),
        Command::ExtractBlock { start, end, .. } => check_range(len, *start, *end, "extractBlock"),
        _ => Ok(()),
    }
}

/// Every command's offsets refer to the original text, so each is checked against that, not against the result of those before it.
pub fn validate_commands(code: &str, raw: &[Command], limits: EditSizeLimits) -> Validation {
    let text = units(code);
    let text: Text = &text;
    let mut seen: Vec<(i64, i64)> = Vec::new();
    let mut inserted_total = 0;
    for (i, command) in raw.iter().enumerate() {
        let resolved = resolve_line_command(text, command);
        let clamped = snap_command(text, clamp_command(text.len() as i64, &resolved));
        let fail = |reason: String| Validation { valid: false, reason: Some(reason), failed_command_index: Some(i) };
        if let Err(reason) = validate_command(text, &clamped) {
            return fail(reason);
        }
        let span = command_span(&clamped);
        if let Some(other) = seen.iter().find(|o| o.0 < span.1 && span.0 < o.1) {
            return fail(format!(
                "command range [{}, {}) overlaps an earlier command's [{}, {}) -- both refer to the code before any command ran",
                span.0, span.1, other.0, other.1
            ));
        }
        let inserted = inserted_line_count(&clamped);
        if inserted > limits.max_inserted_lines {
            return fail(format!(
                "this step writes {inserted} lines at once (limit {}) -- say it as smaller steps, e.g. declare the variables, then add the loop, then the condition",
                limits.max_inserted_lines
            ));
        }
        inserted_total += inserted;
        if inserted_total > limits.max_inserted_lines_total {
            return fail(format!("this step writes {inserted_total} lines in total (limit {}) -- say it as smaller steps", limits.max_inserted_lines_total));
        }
        seen.push(span);
    }
    Validation { valid: true, reason: None, failed_command_index: None }
}

pub fn validate_alternative(alternative: &Alternative, code: &str, limits: EditSizeLimits) -> Alternative {
    let result = validate_commands(code, &alternative.commands, limits);
    Alternative { valid: Some(result.valid), invalid_reason: result.reason, ..alternative.clone() }
}

// ---- block bodies ---------------------------------------------------------------------------------------------------------------

const BLOCK_KEYWORDS: [&str; 11] = ["if", "elif", "else", "for", "while", "try", "except", "finally", "with", "def", "class"];

/// `^(\s*)(?:if|elif|...)\b.*:\s*$`: the leading whitespace when `line` opens a block, else None.
fn block_header(line: &str) -> Option<String> {
    let rest = line.trim_start_matches(is_js_space);
    let indent = &line[..line.len() - rest.len()];
    let word: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    if !BLOCK_KEYWORDS.contains(&word.as_str()) {
        return None;
    }
    let tail = rest[word.len()..].trim_end_matches(is_js_space);
    let body = tail.strip_suffix(':')?;
    (!body.contains(['\r', '\u{2028}', '\u{2029}'])).then(|| indent.to_string())
}

fn indent_of(line: &str) -> usize {
    line.chars().count() - line.trim_start_matches(is_js_space).chars().count()
}

/// A spoken "loop over the numbers" opens a block and says nothing of its body, and a model that stops at the header leaves python
/// that does not parse. An insert or replacement that ends on a block header with nothing deeper after it gets a `pass` body.
pub fn complete_block_bodies(alternative: &Alternative, code: &str, language: &str) -> Alternative {
    if language.to_lowercase() != "python" {
        return alternative.clone();
    }
    let lines: Vec<&str> = code.split('\n').collect();
    let commands = alternative
        .commands
        .iter()
        .map(|command| {
            let (body, next_index) = match command {
                Command::InsertAfterLine { line, text } => (text, *line),
                Command::ReplaceLines { end_line, new_text, .. } => (new_text, *end_line),
                other => return other.clone(),
            };
            let last = body.split('\n').next_back().unwrap_or_default();
            let Some(indent) = block_header(last) else { return command.clone() };
            let next = usize::try_from(next_index).ok().and_then(|i| lines.get(i));
            if next.is_some_and(|n| !n.trim_matches(is_js_space).is_empty() && indent_of(n) > indent.chars().count()) {
                return command.clone();
            }
            let completed = format!("{body}\n{indent}    pass");
            match command {
                Command::InsertAfterLine { line, .. } => Command::InsertAfterLine { line: *line, text: completed },
                Command::ReplaceLines { start_line, end_line, .. } => Command::ReplaceLines { start_line: *start_line, end_line: *end_line, new_text: completed },
                other => other.clone(),
            }
        })
        .collect();
    Alternative { commands, ..alternative.clone() }
}

// ---- prompts --------------------------------------------------------------------------------------------------------------------

const SYSTEM_V1: &str = include_str!("../../assets/voice_edit_system_v1.txt");
const SYSTEM_V2: &str = include_str!("../../assets/voice_edit_system_v2.txt");

pub fn system_prompt(variant: &str) -> &'static str {
    if variant == "v2" {
        SYSTEM_V2
    } else {
        SYSTEM_V1
    }
}

pub fn user_prompt(variant: &str, transcript: &str, code: &str, language: &str) -> String {
    if variant == "v2" {
        let numbered: Vec<String> = code.split('\n').enumerate().map(|(i, line)| format!("{}| {line}", i + 1)).collect();
        return format!("Language: {language}\nCode:\n{}\nInstruction: \"{transcript}\"\nJSON:", numbered.join("\n"));
    }
    format!("Language: {language}\n\nCurrent code:\n```\n{code}\n```\n\nTranscript: \"{transcript}\"\n\nOutput ONLY the JSON object.")
}
