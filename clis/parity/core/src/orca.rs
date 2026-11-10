//! The Orca reference scan. The catalog is the host's registry: every `defineMethod` and `defineStreamingMethod` under `src/`
//! (a name may be a constant). The paired-phone allowlist is `MOBILE_RPC_METHOD_ROUTES`, the arrays it maps, their spreads and
//! their flag-gated spreads. The phone is `mobile/` (its app, sources and modules) and what it imports from `src/shared`.
use crate::graph::{in_type_union, Graph, Scope, Site};
use crate::lex::{lex, Kind, Token};
use crate::methods::{MethodsList, Row};
use crate::ports::Tree;
use crate::resolve;
use crate::scan::{closing, fetch_call, first_arg, is_test, native_modules, site};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

pub const REFERENCE: &str = "references/orca";
const ROUTES_FILE: &str = "src/main/runtime/runtime-rpc/runtime-rpc-mobile-method-routing.ts";
const ROUTES: &str = "MOBILE_RPC_METHOD_ROUTES";
const PHONE_DIRS: [&str; 5] = ["mobile/app", "mobile/src", "mobile/modules", "mobile/packages", "mobile/web-entry"];
const EXTS: [&str; 6] = [".ts", ".tsx", ".js", ".jsx", ".mjs", ".mts"];

/// The definitions of one constant name: `(file, value)`.
type Defs<T> = Vec<(String, T)>;

/// The top-level constants of the host's sources: `const NAME = 'x'` and `const NAME = { key: 'x' }`, by file.
#[derive(Default)]
struct Constants {
    strings: HashMap<String, Defs<String>>,
    objects: HashMap<String, Defs<HashMap<String, String>>>,
}

/// Where a method is registered, and whether as a stream.
struct Registered {
    site: String,
    streaming: bool,
}

impl Constants {
    fn read(files: &BTreeMap<String, Vec<Token>>) -> Constants {
        let mut c = Constants::default();
        for (path, toks) in files {
            for i in 0..toks.len().saturating_sub(4) {
                let top = toks[i].col == 0 || (i > 0 && toks[i - 1].ident() == Some("export") && toks[i - 1].col == 0);
                if !(top && toks[i].ident() == Some("const") && toks[i + 2].is("=")) {
                    continue;
                }
                let Some(name) = toks[i + 1].ident() else { continue };
                if let Some(v) = toks[i + 3].string() {
                    c.strings.entry(name.into()).or_default().push((path.clone(), v.into()));
                } else if toks[i + 3].is("{") {
                    let close = closing(toks, i + 3);
                    let fields: HashMap<String, String> = (i + 4..close.saturating_sub(2)).filter(|&k| toks[k + 1].is(":")).filter_map(|k| Some((toks[k].ident()?.to_string(), toks[k + 2].string()?.to_string()))).collect();
                    if !fields.is_empty() {
                        c.objects.entry(name.into()).or_default().push((path.clone(), fields));
                    }
                }
            }
        }
        c
    }

    /// A constant as the file using it sees it: its own definition first, else the only one.
    fn string(&self, from: &str, name: &str) -> Option<&str> {
        pick(self.strings.get(name)?, from).map(String::as_str)
    }

    fn field(&self, from: &str, name: &str, key: &str) -> Option<&str> {
        pick(self.objects.get(name)?, from)?.get(key).map(String::as_str)
    }
}

fn pick<'a, T>(defs: &'a [(String, T)], from: &str) -> Option<&'a T> {
    defs.iter().find(|(p, _)| p == from).or_else(|| (defs.len() == 1).then(|| &defs[0])).map(|(_, v)| v)
}

/// The value at `toks[at]`: a string, a constant or `CONST.key`.
fn value(c: &Constants, from: &str, toks: &[Token], at: usize) -> Option<String> {
    let t = toks.get(at)?;
    if let Some(s) = t.string() {
        return Some(s.into());
    }
    let name = t.ident()?;
    if toks.get(at + 1).is_some_and(|d| d.is(".")) {
        return c.field(from, name, toks.get(at + 2)?.ident()?).map(String::from);
    }
    c.string(from, name).map(String::from)
}

/// Every registered method by name, and the registrations whose name the scan cannot read.
fn registry(c: &Constants, files: &BTreeMap<String, Vec<Token>>) -> (BTreeMap<String, Registered>, Vec<String>) {
    let (mut out, mut unresolved) = (BTreeMap::new(), vec![]);
    for (path, toks) in files {
        for i in 1..toks.len().saturating_sub(2) {
            let Some(kind @ ("defineMethod" | "defineStreamingMethod")) = toks[i].ident() else { continue };
            if !(toks[i + 1].is("(") && toks[i + 2].is("{")) || toks[i - 1].ident() == Some("function") {
                continue;
            }
            let close = closing(toks, i + 2);
            let mut depth = 0;
            let mut name = None;
            for k in i + 2..close {
                let t = &toks[k];
                if t.kind == Kind::Punct && matches!(t.text.as_str(), "(" | "[" | "{") {
                    depth += 1;
                } else if t.kind == Kind::Punct && matches!(t.text.as_str(), ")" | "]" | "}") {
                    depth -= 1;
                } else if depth == 1 && t.ident() == Some("name") && toks[k + 1].is(":") {
                    name = value(c, path, toks, k + 2);
                    break;
                }
            }
            match name {
                Some(n) => {
                    out.insert(n, Registered { site: site(REFERENCE, path, toks[i].line), streaming: kind == "defineStreamingMethod" });
                }
                None => unresolved.push(site(REFERENCE, path, toks[i].line)),
            }
        }
    }
    (out, unresolved)
}

/// The string entries of `const <name> = [...]`, following `...OTHER` spreads and `...(FLAG ? [...] : [])`, each with the
/// flag gating it.
fn array_entries(arrays: &HashMap<String, (String, usize)>, files: &BTreeMap<String, Vec<Token>>, name: &str, flag: Option<&str>, out: &mut Vec<(String, Option<String>)>) {
    let Some((path, open)) = arrays.get(name) else { return };
    let toks = &files[path];
    let close = closing(toks, *open);
    let mut j = open + 1;
    while j < close {
        let t = &toks[j];
        let spread = toks[j..].len() > 3 && t.is(".") && toks[j + 1].is(".") && toks[j + 2].is(".");
        if let Some(v) = t.string() {
            out.push((v.into(), flag.map(String::from)));
        } else if spread && toks[j + 3].ident().is_some_and(|n| arrays.contains_key(n)) {
            array_entries(arrays, files, &toks[j + 3].text, flag, out);
            j += 4;
            continue;
        } else if spread && toks[j + 3].is("(") && toks.get(j + 5).is_some_and(|q| q.is("?")) {
            let gate = toks[j + 4].text.clone();
            let end = closing(toks, j + 3);
            let colon = (j + 6..end).find(|&k| toks[k].is(":") && toks[j + 6..k].iter().filter(|t| t.is("[") || t.is("(")).count() == toks[j + 6..k].iter().filter(|t| t.is("]") || t.is(")")).count()).unwrap_or(end);
            for t in &toks[j + 6..colon] {
                if let Some(v) = t.string() {
                    out.push((v.into(), Some(gate.clone())));
                }
            }
            j = end + 1;
            continue;
        } else if t.is("[") || t.is("{") || t.is("(") {
            j = closing(toks, j) + 1;
            continue;
        }
        j += 1;
    }
}

/// `method -> (route, flag)` of the paired-phone allowlist.
fn allowlist(files: &BTreeMap<String, Vec<Token>>) -> Result<BTreeMap<String, (String, Option<String>)>, String> {
    let mut arrays = HashMap::new();
    for (path, toks) in files {
        for i in 0..toks.len().saturating_sub(3) {
            if toks[i].ident() == Some("const") && toks[i + 1].kind == Kind::Ident {
                if let Some(eq) = (i + 2..toks.len().min(i + 40)).find(|&k| toks[k].is("=")) {
                    if toks.get(eq + 1).is_some_and(|t| t.is("[")) {
                        arrays.entry(toks[i + 1].text.clone()).or_insert((path.clone(), eq + 1));
                    }
                }
            }
        }
    }
    let path = format!("{REFERENCE}/{ROUTES_FILE}");
    let toks = files.get(&path).ok_or_else(|| format!("{path} is missing: the scan reads the phone allowlist ({ROUTES}) there"))?;
    let start = (0..toks.len().saturating_sub(1)).find(|&i| toks[i].ident() == Some(ROUTES)).ok_or_else(|| format!("{path} no longer declares {ROUTES}"))?;
    let open = (start..toks.len()).find(|&i| toks[i].is("(")).unwrap_or(start);
    let close = closing(toks, open);
    let mut out = BTreeMap::new();
    for k in open..close.saturating_sub(4) {
        // `...NAME.map((method) => [method, 'route'] as const)`
        if let (Some(name), true) = (toks[k].ident().filter(|n| arrays.contains_key(*n)), toks[k + 1].is(".") && toks[k + 2].ident() == Some("map")) {
            let map_close = closing(toks, k + 3);
            let route = toks[k + 3..map_close].iter().find_map(Token::string).unwrap_or("").to_string();
            let mut entries = vec![];
            array_entries(&arrays, files, name, None, &mut entries);
            for (method, flag) in entries {
                out.entry(method).or_insert((route.clone(), flag));
            }
        }
    }
    if out.is_empty() {
        return Err(format!("{path}: {ROUTES} maps no array the scan can read"));
    }
    Ok(out)
}

pub fn scan(tree: &dyn Tree, pin: Value) -> Result<MethodsList, String> {
    let host: BTreeMap<String, Vec<Token>> = tree
        .files(&format!("{REFERENCE}/src"))
        .into_iter()
        .filter(|f| (f.ends_with(".ts") || f.ends_with(".tsx")) && !is_test(f))
        .filter_map(|f| Some((f.clone(), lex(&tree.read(&f)?))))
        .collect();
    let constants = Constants::read(&host);
    let (registry, unresolved) = registry(&constants, &host);
    if registry.is_empty() {
        return Err(format!("{REFERENCE}/src registers no method the scan can read (defineMethod)"));
    }
    let allow = allowlist(&host)?;
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for (name, reg) in &registry {
        let mut row = Row::new("rpc", name);
        row.subscription = reg.streaming;
        row.extra.insert("registered".into(), reg.site.clone().into());
        let (route, flag) = allow.get(name).map_or((Value::Null, Value::Null), |(r, f)| (r.clone().into(), f.clone().map_or(Value::Null, Value::from)));
        row.extra.insert("allowlist".into(), route);
        if !flag.is_null() {
            row.extra.insert("flag".into(), flag);
        }
        rows.insert(row.id.clone(), row);
    }
    let resolver = |from: &str, spec: &str| resolve::relative(tree, from, spec, &EXTS);
    let mut roots: Vec<String> = PHONE_DIRS.iter().flat_map(|d| tree.files(&format!("{REFERENCE}/{d}"))).filter(|f| EXTS.iter().any(|e| f.ends_with(e)) && !is_test(f)).collect();
    roots.push(format!("{REFERENCE}/mobile/index.js"));
    let phone = format!("{REFERENCE}/mobile/");
    let scope = Scope { roots, resolve: &resolver, excluded: &is_test, is_app: &|p: &str| p.starts_with(&phone), scanned: &|_| true };
    let graph = Graph::build(tree, &scope);
    let (mut hits, mut fetches, mut dynamic, mut capabilities) = (vec![], vec![], vec![], vec![]);
    let mut e2ee = None;
    graph.each_live(|s: Site| {
        let (toks, at) = (s.toks, s.at);
        let t = &toks[at];
        let here = || site(REFERENCE, s.path, t.line);
        let base = s.path.rsplit('/').next().unwrap_or("");
        if base.contains("e2ee") && graph.is_app(s.path) && e2ee.as_ref().is_none_or(|(b, _): &(String, String)| base.len() < b.len()) {
            e2ee = Some((base.to_string(), site(REFERENCE, s.path, 1)));
        }
        if !s.bound {
            return;
        }
        if let Some(open) = fetch_call(&s) {
            fetches.push((here(), first_arg(toks, open)));
        }
        if let Some(uri) = (at >= 2 && toks[at - 1].is(":") && toks[at - 2].ident() == Some("uri")).then(|| first_arg(toks, at - 1)).filter(|u| u.starts_with("'http") || u.starts_with("`http")) {
            fetches.push((here(), uri));
        }
        if t.ident() == Some("sendRequest") && at > 0 && toks[at - 1].is(".") && toks.get(at + 1).is_some_and(|o| o.is("(")) && toks.get(at + 2).is_some_and(|a| a.kind != Kind::Str) {
            dynamic.push(json!({"site": here(), "method": first_arg(toks, at + 1)}));
        }
        let name = match t.kind {
            Kind::Str if !in_type_union(toks, at) => Some(t.text.clone()),
            Kind::Ident if t.text.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && !(at > 0 && (toks[at - 1].is(".") || toks[at - 1].ident() == Some("const"))) => value(&constants, s.path, toks, at),
            _ => None,
        };
        if let Some(n) = name.filter(|n| registry.contains_key(n)) {
            // A capability flag that happens to share a method's name is no call.
            if !graph.is_app(s.path) && s.declares.iter().any(|d| d.contains("CAPABILIT")) {
                capabilities.push(json!({"name": n, "site": here()}));
                return;
            }
            hits.push((n, here()));
        }
    });
    for (name, at) in hits {
        let row = rows.get_mut(&format!("rpc:{name}")).expect("a registered method");
        row.reached(at);
        row.class = match (row.extra["allowlist"].is_null(), row.extra.get("flag")) {
            (true, _) => Some("offAllowlist".into()),
            (false, Some(_)) => Some("flagged".into()),
            _ => None,
        };
    }
    let mut fetch_index: BTreeMap<String, usize> = BTreeMap::new();
    for (at, target) in fetches {
        let file = at.rsplit_once(':').map_or(at.as_str(), |(f, _)| f).to_string();
        let n = fetch_index.entry(file.clone()).or_default();
        *n += 1;
        let mut row = Row::new("http", &format!("{file}#{n}"));
        row.extra.insert("target".into(), target.into());
        row.reached(at);
        rows.insert(row.id.clone(), row);
    }
    if let Some((_, at)) = e2ee {
        let mut row = Row::new("transport", "e2ee.handshake");
        row.extra.insert("note".into(), "the paired E2EE handshake every request rides on".into());
        row.reached(at);
        rows.insert(row.id.clone(), row);
    }
    let rows: Vec<Row> = rows.into_values().collect();
    let methods = |f: &dyn Fn(&Row) -> bool| rows.iter().filter(|r| r.kind == "rpc" && f(r)).count();
    let counts = json!({
        "catalog": {
            "registered": methods(&|_| true),
            "streaming": methods(&|r| r.subscription),
            "allowlist": allow.len(),
            "allowlistShipped": allow.values().filter(|(_, f)| f.is_none()).count(),
            "allowlistFlagged": allow.values().filter(|(_, f)| f.is_some()).count(),
            "allowlistUnregistered": allow.keys().filter(|m| !registry.contains_key(*m)).cloned().collect::<Vec<_>>(),
        },
        "phone": {
            "methods": methods(&|r| r.phone && r.class.as_deref() != Some("offAllowlist")),
            "flagged": methods(&|r| r.phone && r.class.as_deref() == Some("flagged")),
            "offAllowlist": methods(&|r| r.phone && r.class.as_deref() == Some("offAllowlist")),
            "subscriptions": methods(&|r| r.phone && r.subscription),
            "http": rows.iter().filter(|r| r.kind == "http").count(),
            "transport": rows.iter().filter(|r| r.kind == "transport").count(),
        },
        "files": {"reached": graph.files.len(), "phone": graph.files.keys().filter(|p| graph.is_app(p)).count()},
    });
    let flags = json!({
        "dynamicMethodNames": {"note": "sendRequest calls whose method is not a literal; the names they carry come from elsewhere", "sites": dynamic},
        "capabilityNames": {"note": "capability flags in shared code that share a method's name; no call", "sites": capabilities},
        "unresolvedRegistrations": unresolved,
        "nativeModules": native_modules(tree, &format!("{REFERENCE}/mobile/modules"), REFERENCE),
    });
    Ok(MethodsList { app: "orca".into(), pin, counts, rows, flags })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowlist_follows_spreads_and_marks_flag_gated_entries() {
        // Given a routes map over two arrays, one spreading a third and a flag-gated pair
        let files: BTreeMap<String, Vec<Token>> = [(
            format!("{REFERENCE}/{ROUTES_FILE}"),
            lex("const SESSION = ['s.a'] as const\nexport const DESK = ['d.a', ...(FLAG ? (['d.f1', 'd.f2'] as const) : [])] as const\nexport const HOST = ['h.a', ...SESSION, 'h.b'] as const\nexport const MOBILE_RPC_METHOD_ROUTES = new Map([\n  ...DESK.map((m) => [m, 'paired-desktop'] as const),\n  ...HOST.map((m) => [m, 'execution-host'] as const)\n])\n"),
        )]
        .into();
        // When the allowlist is read
        let allow = allowlist(&files).unwrap();
        // Then every entry is there with its route, and only the gated ones carry the flag
        assert_eq!(allow.keys().collect::<Vec<_>>(), ["d.a", "d.f1", "d.f2", "h.a", "h.b", "s.a"]);
        assert_eq!(allow["s.a"], ("execution-host".into(), None));
        assert_eq!(allow["d.f2"], ("paired-desktop".into(), Some("FLAG".into())));
    }
}
