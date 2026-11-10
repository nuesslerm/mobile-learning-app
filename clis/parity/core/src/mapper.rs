//! What a mapper answers today, read from its own tables: for `t3 serve`, the RPC, command and project-mutation lists and the
//! HTTP routes of `t3-http`. Answering is not serving: a row the mapper answers stays pending until a run bundle proves it.
use crate::lex::lex;
use crate::methods::Row;
use crate::ports::Tree;
use std::collections::BTreeMap;

/// `(row kind, file, const)` of each list `t3 serve` answers from.
pub const T3_TABLES: [(&str, &str, &str); 3] = [
    ("rpc", "clis/t3-serve/contracts/src/rpc.rs", "IMPLEMENTED_RPCS"),
    ("command", "clis/t3-serve/core/src/domain/decider.rs", "SUPPORTED"),
    ("mutation", "clis/t3-serve/core/src/domain/command.rs", "PROJECT_TYPES"),
];
pub const T3_ROUTES: &str = "clis/t3-serve/http/src/http.rs";

/// The strings of `const <name>: [&str; N] = [...]` in Rust source.
pub fn const_list(text: &str, name: &str) -> Option<Vec<String>> {
    let at = text.find(&format!("const {name}:"))?;
    let open = at + text[at..].find("= [")? + 2;
    let close = open + text[open..].find("];")?;
    Some(lex(&text[open..=close]).into_iter().filter_map(|t| t.string().map(String::from)).collect())
}

/// A string literal, or a `const NAME: &str` of the file, starting at the head of `s`.
fn literal(s: &str, consts: &BTreeMap<String, String>) -> Option<String> {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('"') {
        return rest.split_once('"').map(|(v, _)| v.to_string());
    }
    let name: String = s.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    consts.get(&name).cloned()
}

/// `(method or None for any, path pattern)` of each route `t3-http` matches; a path parameter is `:param`.
pub fn routes(text: &str) -> Vec<(Option<String>, String)> {
    let consts: BTreeMap<String, String> = text
        .lines()
        .filter_map(|l| {
            let rest = l.trim_start().strip_prefix("const ")?;
            let (name, value) = rest.split_once(": &str = ")?;
            Some((name.to_string(), value.trim_end_matches(';').trim_matches('"').to_string()))
        })
        .collect();
    let arm = |i: usize| -> Option<(String, usize)> {
        let rest = &text[i..];
        let method: String = rest.strip_prefix("(\"")?.chars().take_while(|c| c.is_ascii_uppercase()).collect();
        (!method.is_empty() && rest[2 + method.len()..].starts_with("\", ")).then(|| (method.clone(), i + 5 + method.len()))
    };
    let arms: Vec<(usize, String, usize)> = text.match_indices("(\"").filter_map(|(i, _)| arm(i).map(|(m, after)| (i, m, after))).collect();
    let fns: Vec<usize> = text.match_indices("fn ").map(|(i, _)| i).collect();
    let mut out = vec![];
    for (k, (start, method, after)) in arms.iter().enumerate() {
        let end = arms.get(k + 1).map_or(text.len(), |a| a.0).min(fns.iter().copied().find(|f| f > start).unwrap_or(text.len()));
        let (head, body) = text[*after..end].split_once("=>").unwrap_or((&text[*after..end], ""));
        let m = Some(method.clone());
        if let Some(p) = literal(head, &consts).filter(|_| head.trim_start().starts_with('"')) {
            out.push((m.clone(), p));
        }
        for (i, _) in head.match_indices("path == ") {
            out.extend(literal(&head[i + 8..], &consts).map(|p| (m.clone(), p)));
        }
        for (i, _) in head.match_indices("starts_with(") {
            let Some(prefix) = literal(&head[i + 12..], &consts) else { continue };
            if body.contains("split_once('/')") {
                for (j, _) in body.match_indices("Some((") {
                    let seg = body[j..].split_once(", ").and_then(|(_, r)| literal(r, &consts));
                    out.extend(seg.map(|s| (m.clone(), format!("{prefix}:param/{s}"))));
                }
            }
        }
    }
    // `strip_prefix(P)` is a parameter after P; the first `matches!` after it, before the next fn, names the segments after that.
    for (i, _) in text.match_indices("strip_prefix(") {
        let Some(prefix) = literal(&text[i + 13..], &consts) else { continue };
        let method = arms.iter().rev().find(|a| a.0 < i && !fns.iter().any(|f| *f > a.0 && *f < i)).map(|a| a.1.clone());
        let next_fn = fns.iter().copied().find(|f| *f > i).unwrap_or(text.len());
        let paired = text[i..next_fn].find("matches!(").map(|j| i + j);
        match paired.and_then(|j| text[j..next_fn].split_once(')').map(|(m, _)| m.to_string())) {
            Some(m) => out.extend(m.split(", ").skip(1).flat_map(|alts| alts.split('|')).filter_map(|s| literal(s, &consts)).map(|s| (method.clone(), format!("{prefix}:param/{s}")))),
            None => out.push((method, format!("{prefix}:param"))),
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `/a/:threadId/b` to `/a/:param/b`.
fn pattern(path: &str) -> String {
    path.split('/').map(|s| if s.starts_with(':') { ":param" } else { s }).collect::<Vec<_>>().join("/")
}

/// `row id -> where the mapper answers it`, for every phone row `t3 serve` answers.
pub fn t3_answers(tree: &dyn Tree, rows: &[Row]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (kind, file, name) in T3_TABLES {
        for value in tree.read(file).and_then(|t| const_list(&t, name)).unwrap_or_default() {
            out.insert(format!("{kind}:{value}"), format!("{file} {name}"));
        }
    }
    let routes = tree.read(T3_ROUTES).map(|t| routes(&t)).unwrap_or_default();
    for row in rows.iter().filter(|r| r.kind == "http") {
        let (Some(verb), Some(path)) = (row.extra.get("method").and_then(|v| v.as_str()), row.extra.get("path").and_then(|v| v.as_str())) else { continue };
        let want = pattern(path);
        if routes.iter().any(|(m, p)| m.as_deref().is_none_or(|m| m == verb) && pattern(p) == want) {
            out.insert(row.id.clone(), format!("{T3_ROUTES} routes"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_const_list_reads_its_strings() {
        let text = "pub const SUPPORTED: [&str; 2] = [\n    \"thread.create\",\n    \"run.interrupt\",\n];\nfn f(x: &'static str) {}";
        assert_eq!(const_list(text, "SUPPORTED").unwrap(), ["thread.create", "run.interrupt"]);
        assert_eq!(const_list(text, "MISSING"), None);
    }

    #[test]
    fn match_arms_guards_and_split_segments_become_routes() {
        // Given the arm shapes t3-http uses
        let text = r#"const THREADS: &str = "/api/orchestration/threads/";
pub fn handle() {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/auth/session") => respond(),
        ("POST", path) if path == "/api/auth/websocket-ticket" || path.starts_with("/api/auth/websocket-ticket/") => {
            let use_ = path.strip_prefix("/api/auth/websocket-ticket/").unwrap_or("rpc");
        }
        ("GET", path) if path.starts_with(THREADS) => {
            let answer = match path[THREADS.len()..].split_once('/') {
                Some((thread, "history")) => a(),
                Some((thread, "transcript")) => b(),
                _ => c(),
            };
        }
    }
}
"#;
        // Then each becomes a method and a path pattern
        let r = routes(text);
        for want in [("GET", "/api/auth/session"), ("POST", "/api/auth/websocket-ticket"), ("POST", "/api/auth/websocket-ticket/:param"), ("GET", "/api/orchestration/threads/:param/history"), ("GET", "/api/orchestration/threads/:param/transcript")] {
            assert!(r.contains(&(Some(want.0.into()), want.1.into())), "{want:?} not in {r:?}");
        }
        assert_eq!(r.len(), 5, "{r:?}");
    }

    #[test]
    fn a_strip_prefix_pairs_only_with_the_first_matches_after_it_in_its_fn() {
        // Given two functions, each stripping a prefix, and a matches! that belongs to the second only
        let text = r#"const A: &str = "/a/";
fn one(path: &str) {
    let rest = path.strip_prefix(A);
}
fn two(path: &str) {
    let rest = path.strip_prefix("/b/");
    if matches!(tail, "x" | "y") {}
    if matches!(other, "z") {}
}
"#;
        // Then the first is a bare parameter and the second takes the segments of its first matches! only
        assert_eq!(routes(text), [(None, "/a/:param".into()), (None, "/b/:param/x".into()), (None, "/b/:param/y".into())]);
    }
}
