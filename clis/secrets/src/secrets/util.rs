use regex::Regex;

pub const PLACEHOLDER_VALUE: &str = "PLACEHOLDER_FILL_ME";

/// Masks a secret for display, keeping a few boundary characters for recognizability.
pub fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 6 {
        return "•".repeat(chars.len());
    }
    let head: String = chars[..3].iter().collect();
    let tail: String = chars[chars.len() - 3..].iter().collect();
    format!(
        "{head}{}{tail}",
        "•".repeat(std::cmp::max(3, chars.len() - 6))
    )
}

/// A vault secret that exists but was never filled in: the sentinel, its `_` suffixed forms, or `<angle-bracketed>`.
pub fn is_placeholder(value: Option<&str>) -> bool {
    let Some(value) = value else { return false };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == PLACEHOLDER_VALUE || trimmed.starts_with(&format!("{PLACEHOLDER_VALUE}_")) {
        return true;
    }
    Regex::new(r"^<[^<>]+>$").unwrap().is_match(trimmed)
}

/// `String.prototype.padEnd` for the ASCII keys and labels this tool prints.
pub fn pad_end(s: &str, width: usize) -> String {
    let len = s.chars().count();
    format!("{s}{}", " ".repeat(width.saturating_sub(len)))
}

/// `JSON.stringify` of a string.
pub fn json_string(s: &str) -> String {
    serde_json::to_string(s).expect("string serializes")
}
