//! Syntax highlighting for the diff viewer: tree-sitter grammars compiled in, one line of HTML per line of code, with inline
//! colours of the GitHub dark theme (the page's own CSS is written for that). A language without a grammar is plain text.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tree_sitter::Language;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

const NAMES: &[&str] = &[
    "attribute", "comment", "constant", "constant.builtin", "constructor", "function", "function.builtin", "function.method", "keyword", "number", "operator",
    "property", "punctuation", "punctuation.bracket", "punctuation.delimiter", "string", "string.special", "tag", "type", "type.builtin", "variable",
    "variable.builtin", "variable.parameter", "escape", "embedded", "label",
];

fn colour(index: usize) -> Option<&'static str> {
    let name = *NAMES.get(index)?;
    Some(match name {
        n if n.starts_with("comment") => "#8b949e",
        n if n.starts_with("string") || n == "escape" => "#a5d6ff",
        n if n.starts_with("keyword") || n == "operator" || n == "tag" => "#ff7b72",
        n if n.starts_with("function") || n == "constructor" => "#d2a8ff",
        n if n.starts_with("constant") || n == "number" || n == "property" || n == "attribute" || n == "label" => "#79c0ff",
        n if n.starts_with("type") => "#ffa657",
        "variable.builtin" | "variable.parameter" => "#ffa657",
        _ => return None,
    })
}

/// The language name the viewer uses for a file, by extension (`detectLangByFile`).
pub fn language_for_file(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "ts" | "tsx" | "mts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "json" | "json5" => "json",
        "md" | "mdx" => "markdown",
        "py" => "python",
        "rb" => "ruby",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "c" => "c",
        "cpp" => "cpp",
        "cs" => "csharp",
        "sh" | "bash" | "zsh" => "bash",
        "css" | "less" => "css",
        "scss" => "scss",
        "html" | "htm" => "html",
        "xml" | "svg" => "xml",
        "yaml" | "yml" => "yaml",
        "sql" => "sql",
        "tf" | "hcl" => "hcl",
        "dockerfile" => "dockerfile",
        "makefile" => "makefile",
        _ => "text",
    }
}

fn build(lang: &str) -> Option<HighlightConfiguration> {
    let (language, highlights, locals): (Language, String, &str) = match lang {
        "typescript" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            format!("{}\n{}", tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_typescript::HIGHLIGHTS_QUERY),
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        "javascript" => (tree_sitter_javascript::LANGUAGE.into(), tree_sitter_javascript::HIGHLIGHT_QUERY.into(), tree_sitter_javascript::LOCALS_QUERY),
        "json" => (tree_sitter_json::LANGUAGE.into(), tree_sitter_json::HIGHLIGHTS_QUERY.into(), ""),
        "python" => (tree_sitter_python::LANGUAGE.into(), tree_sitter_python::HIGHLIGHTS_QUERY.into(), ""),
        "rust" => (tree_sitter_rust::LANGUAGE.into(), tree_sitter_rust::HIGHLIGHTS_QUERY.into(), ""),
        "go" => (tree_sitter_go::LANGUAGE.into(), tree_sitter_go::HIGHLIGHTS_QUERY.into(), ""),
        "java" => (tree_sitter_java::LANGUAGE.into(), tree_sitter_java::HIGHLIGHTS_QUERY.into(), ""),
        "c" => (tree_sitter_c::LANGUAGE.into(), tree_sitter_c::HIGHLIGHT_QUERY.into(), ""),
        "cpp" => (tree_sitter_cpp::LANGUAGE.into(), tree_sitter_cpp::HIGHLIGHT_QUERY.into(), ""),
        "ruby" => (tree_sitter_ruby::LANGUAGE.into(), tree_sitter_ruby::HIGHLIGHTS_QUERY.into(), tree_sitter_ruby::LOCALS_QUERY),
        "bash" => (tree_sitter_bash::LANGUAGE.into(), tree_sitter_bash::HIGHLIGHT_QUERY.into(), ""),
        "css" => (tree_sitter_css::LANGUAGE.into(), tree_sitter_css::HIGHLIGHTS_QUERY.into(), ""),
        "html" => (tree_sitter_html::LANGUAGE.into(), tree_sitter_html::HIGHLIGHTS_QUERY.into(), ""),
        "yaml" => (tree_sitter_yaml::LANGUAGE.into(), tree_sitter_yaml::HIGHLIGHTS_QUERY.into(), ""),
        _ => return None,
    };
    let mut config = HighlightConfiguration::new(language, lang, &highlights, "", locals).ok()?;
    config.configure(NAMES);
    Some(config)
}

fn config_for(lang: &str) -> Option<&'static HighlightConfiguration> {
    static CONFIGS: OnceLock<Mutex<HashMap<String, Option<&'static HighlightConfiguration>>>> = OnceLock::new();
    let mut map = CONFIGS.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(|e| e.into_inner());
    *map.entry(lang.to_string()).or_insert_with(|| build(lang).map(|c| &*Box::leak(Box::new(c))))
}

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// One HTML string per input line; plain escaped text for a language without a grammar or a failed parse.
pub fn highlight_lines(lines: &[String], lang: &str) -> Vec<String> {
    let plain = || lines.iter().map(|l| escape(l)).collect::<Vec<_>>();
    if lines.is_empty() {
        return Vec::new();
    }
    let Some(config) = config_for(lang) else { return plain() };
    let source = lines.join("\n");
    let mut highlighter = Highlighter::new();
    let Ok(events) = highlighter.highlight(config, source.as_bytes(), None, None, |_| None) else { return plain() };
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut current = String::new();
    let mut stack: Vec<usize> = Vec::new();
    for event in events {
        let Ok(event) = event else { return plain() };
        match event {
            HighlightEvent::HighlightStart(h) => stack.push(h.0),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                let style = stack.iter().rev().find_map(|i| colour(*i));
                for (i, part) in source[start..end].split('\n').enumerate() {
                    if i > 0 {
                        out.push(std::mem::take(&mut current));
                    }
                    if part.is_empty() {
                        continue;
                    }
                    match style {
                        Some(c) => current.push_str(&format!("<span style=\"color:{c}\">{}</span>", escape(part))),
                        None => current.push_str(&escape(part)),
                    }
                }
            }
        }
    }
    out.push(current);
    while out.len() < lines.len() {
        out.push(escape(&lines[out.len()]));
    }
    out.truncate(lines.len());
    out
}
