//! What both reference scans share: which files are tests, how a site is written, and the token shapes of a call.
use crate::graph::Site;
use crate::lex::{Kind, Token};
use crate::ports::Tree;
use serde_json::{json, Value};

/// Test code, fixtures and fakes: never the phone, so never entered.
pub fn is_test(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    [".test.", ".spec.", ".stories.", "fixture", "test-fake", "test-helper"].iter().any(|m| base.contains(m))
        || base.ends_with(".d.ts")
        || ["/__tests__/", "/__mocks__/", "/test/", "/tests/", "/fixtures/", "/test-support/", "/testing/"].iter().any(|m| path.contains(m))
}

/// `file:line`, relative to the reference checkout.
pub fn site(reference: &str, path: &str, line: u32) -> String {
    format!("{}:{line}", path.strip_prefix(reference).map_or(path, |p| p.trim_start_matches('/')))
}

/// The index of the token closing the bracket opened at `open`.
pub fn closing(toks: &[Token], open: usize) -> usize {
    let mut depth = 0i32;
    for (i, t) in toks.iter().enumerate().skip(open) {
        if t.kind != Kind::Punct {
            continue;
        }
        match t.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    toks.len()
}

/// The first argument of the call whose `(` is at `open`, as source-like text.
pub fn first_arg(toks: &[Token], open: usize) -> String {
    let (mut depth, mut out) = (0i32, String::new());
    for t in toks[open + 1..].iter().filter(|t| !t.nested) {
        if t.kind == Kind::Punct && matches!(t.text.as_str(), "(" | "[" | "{") {
            depth += 1;
        } else if t.kind == Kind::Punct && matches!(t.text.as_str(), ")" | "]" | "}") {
            if depth == 0 {
                break;
            }
            depth -= 1;
        } else if depth == 0 && t.is(",") {
            break;
        }
        match t.kind {
            Kind::Str => out.push_str(&format!("'{}'", t.text)),
            Kind::Template => out.push_str(&format!("`{}`", t.text)),
            _ => out.push_str(&t.text),
        }
    }
    out
}

/// A raw `fetch` (or an injected `fetchImpl`) called at this site: `fetch(x)`, `fetchImpl(x)` or `(a ?? fetch)(x)`, returning
/// the index of the call's `(`. A wrapper that only forwards its own parameters (`(input, init) => fetch(input, init)`) is no site.
pub fn fetch_call(s: &Site) -> Option<usize> {
    let (toks, at) = (s.toks, s.at);
    let name = toks[at].ident()?;
    if !matches!(name, "fetch" | "fetchImpl") || (at > 0 && matches!(toks[at - 1].text.as_str(), "." | "function" | "async")) {
        return None;
    }
    let open = match (toks.get(at + 1), toks.get(at + 2)) {
        (Some(a), _) if a.is("(") => at + 1,
        (Some(a), Some(b)) if a.is(")") && b.is("(") => at + 2,
        _ => return None,
    };
    let args: Vec<&str> = toks[open + 1..closing(toks, open)].iter().filter(|t| !t.is(",")).map(|t| t.text.as_str()).collect();
    let params = at.checked_sub(1).filter(|&k| toks[k].is("=>")).map(|arrow| {
        let close = arrow - 1;
        let open = (0..close).rev().find(|&k| toks[k].is("(")).unwrap_or(close);
        // The parameter names, not their types: an identifier right after `(` or a `,` outside `<...>`.
        let mut angle = 0i32;
        let mut names = vec![];
        for k in open + 1..close {
            match toks[k].text.as_str() {
                "<" => angle += 1,
                ">" => angle -= 1,
                _ if angle == 0 && toks[k].kind == Kind::Ident && (toks[k - 1].is(",") || k == open + 1) => names.push(toks[k].text.as_str()),
                _ => {}
            }
        }
        names
    });
    if params.is_some_and(|p| !p.is_empty() && p == args) {
        return None;
    }
    Some(open)
}

/// Native module directories of the phone (Swift, Kotlin, Java, Objective-C): the scan reads none of them.
pub fn native_modules(tree: &dyn Tree, dir: &str, reference: &str) -> Value {
    let mut by_module = std::collections::BTreeMap::<String, usize>::new();
    for f in tree.files(dir) {
        if [".swift", ".kt", ".java", ".m", ".mm"].iter().any(|e| f.ends_with(e)) && !is_test(&f) {
            let module = f[dir.len() + 1..].split('/').next().unwrap_or("").to_string();
            *by_module.entry(module).or_default() += 1;
        }
    }
    let modules: Vec<Value> = by_module.into_iter().map(|(m, n)| json!({"dir": site(reference, &format!("{dir}/{m}"), 0).trim_end_matches(":0"), "nativeFiles": n})).collect();
    json!({"note": "native module sources are not scanned; a method they send by name is not a row", "modules": modules})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::lex;

    fn site_at<'a>(toks: &'a [Token], name: &str) -> Site<'a> {
        let at = toks.iter().position(|t| t.text == name).unwrap();
        Site { path: "p", toks, at, bound: true, declares: &[], chunk: (0, toks.len()) }
    }

    #[test]
    fn a_fetch_names_its_target_and_a_forwarding_wrapper_is_no_site() {
        let call = lex("const r = await (args.fetchImpl ?? fetch)(url.toString(), { method: 'POST' })");
        let open = fetch_call(&site_at(&call, "fetch")).expect("a site");
        assert_eq!(first_arg(&call, open), "url.toString()");
        for wrapper in ["export const s = make((input, init) => fetch(input, init))", "const f = (input: string, init: Record<string, X>) => fetch(input, init)"] {
            let toks = lex(wrapper);
            assert_eq!(fetch_call(&site_at(&toks, "fetch")), None, "{wrapper}");
        }
        let method = lex("async fetch(client) { return this.fetch(client) }");
        assert_eq!(fetch_call(&site_at(&method, "fetch")), None);
    }

    #[test]
    fn tests_fixtures_and_fakes_are_test_code() {
        for p in ["a/b.test.ts", "a/__tests__/x.ts", "a/x-test-fakes.ts", "a/test-support/x.ts", "a/x.test-fixture.tsx", "a/types.d.ts"] {
            assert!(is_test(p), "{p}");
        }
        assert!(!is_test("a/attestation.ts"));
    }
}
