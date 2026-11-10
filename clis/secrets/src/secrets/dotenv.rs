use regex::Regex;

/// `dotenv.parse`: the same line grammar, in file order, last duplicate wins in its first position.
pub fn parse(src: &str) -> Vec<(String, String)> {
    let line = Regex::new(
        r#"(?m)^\s*(?:export\s+)?([A-Za-z0-9_.-]+)(?:\s*=\s*?|:\s+?)(\s*'(?:\\'|[^'])*'|\s*"(?:\\"|[^"])*"|\s*`(?:\\`|[^`])*`|[^#\r\n]+)?\s*(?:#.*)?$"#,
    )
    .unwrap();
    let normalized = src.replace("\r\n", "\n").replace('\r', "\n");
    let mut out: Vec<(String, String)> = Vec::new();
    for caps in line.captures_iter(&normalized) {
        let key = caps[1].to_string();
        let mut value = caps.get(2).map_or("", |m| m.as_str()).trim().to_string();
        let quote = value.chars().next();
        if let Some(q) = quote.filter(|q| matches!(q, '\'' | '"' | '`')) {
            if value.chars().count() >= 2 && value.ends_with(q) {
                value = value[q.len_utf8()..value.len() - q.len_utf8()].to_string();
            }
            if q == '"' {
                value = value.replace("\\n", "\n").replace("\\r", "\r");
            }
        }
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => out.push((key, value)),
        }
    }
    out
}
