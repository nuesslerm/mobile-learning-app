//! The T3 Code reference scan. The catalog comes from `packages/contracts`: `WS_METHODS` and `ORCHESTRATION_V2_WS_METHODS`
//! (a subscription is an `Rpc.make` with `stream: true`), the `OrchestrationV2Command` and `ProjectMutation` unions, and the
//! endpoints of the environment `HttpApi`, each RPC with the scope the server requires of it. The phone is `apps/mobile` and what it
//! imports, `packages/client-runtime` above all.
use crate::graph::{in_type_union, Graph, Scope, Site};
use crate::lex::{lex, Kind, Token};
use crate::methods::{MethodsList, Row};
use crate::ports::Tree;
use crate::resolve;
use crate::scan::{closing, fetch_call, first_arg, is_test, native_modules, site};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const REFERENCE: &str = "references/t3code";
const CONTRACTS: &str = "packages/contracts/src";
const METHOD_TABLES: [(&str, &str); 2] = [("rpc.ts", "WS_METHODS"), ("orchestrationV2.ts", "ORCHESTRATION_V2_WS_METHODS")];
/// The server's table spreads the client's guarded one first, so its own entries win.
const SCOPE_TABLES: [(&str, &str); 2] = [("packages/contracts/src/clientRpcPermissions.ts", "CLIENT_GUARDED_RPC_SCOPES"), ("apps/server/src/auth/RpcAuthorization.ts", "RPC_REQUIRED_SCOPES")];

fn tokens(tree: &dyn Tree, file: &str) -> Result<Vec<Token>, String> {
    tree.read(&format!("{REFERENCE}/{CONTRACTS}/{file}")).map(|t| lex(&t)).ok_or_else(|| format!("{REFERENCE}/{CONTRACTS}/{file} is missing"))
}

/// The index of the first token after `const <name> =`.
fn declaration(toks: &[Token], name: &str) -> Option<usize> {
    (0..toks.len().saturating_sub(3)).find(|&i| toks[i].ident() == Some("const") && toks[i + 1].ident() == Some(name) && toks[i + 2].is("=")).map(|i| i + 3)
}

/// `{ key: "value", ... }` of `const <name> = {`.
fn string_object(toks: &[Token], name: &str) -> Vec<(String, String)> {
    let Some(open) = declaration(toks, name).filter(|&i| toks[i].is("{")) else { return vec![] };
    let close = closing(toks, open);
    (open + 1..close.saturating_sub(2)).filter(|&i| toks[i - 1].is("{") || toks[i - 1].is(",")).filter_map(|i| Some((toks[i].ident()?.to_string(), toks[i + 2].string().filter(|_| toks[i + 1].is(":"))?.to_string()))).collect()
}

/// The `type` literal of each member struct of `const <name> = Schema.Union([...])`; nested `type` fields are not members.
pub fn union_types(toks: &[Token], name: &str) -> Vec<String> {
    let Some(start) = declaration(toks, name) else { return vec![] };
    let Some(open) = (start..toks.len()).find(|&i| toks[i].is("[")) else { return vec![] };
    let close = closing(toks, open);
    let mut depth = 0i32;
    let mut out = vec![];
    for i in open..close {
        let t = &toks[i];
        if t.kind == Kind::Punct && matches!(t.text.as_str(), "(" | "[" | "{") {
            depth += 1;
        } else if t.kind == Kind::Punct && matches!(t.text.as_str(), ")" | "]" | "}") {
            depth -= 1;
        } else if depth == 3 && t.ident() == Some("type") && toks[i + 1].is(":") {
            let lit = &toks[i + 2..(i + 7).min(close)];
            if lit.len() == 5 && lit[0].ident() == Some("Schema") && lit[2].ident() == Some("Literal") {
                if let Some(v) = lit[4].string() {
                    out.push(v.to_string());
                }
            }
        }
    }
    out
}

/// `(table, key)` of every `Rpc.make(<table>.<key>, { ... stream: true ... })`.
fn streams(toks: &[Token]) -> HashSet<(String, String)> {
    let mut out = HashSet::new();
    for i in 0..toks.len().saturating_sub(7) {
        if toks[i].ident() == Some("Rpc") && toks[i + 1].is(".") && toks[i + 2].ident() == Some("make") && toks[i + 3].is("(") && toks[i + 5].is(".") {
            let close = closing(toks, i + 3);
            let stream = (i + 4..close.saturating_sub(2)).any(|k| toks[k].ident() == Some("stream") && toks[k + 1].is(":") && toks[k + 2].ident() == Some("true"));
            if let (true, Some(table), Some(key)) = (stream, toks[i + 4].ident(), toks[i + 6].ident()) {
                out.insert((table.to_string(), key.to_string()));
            }
        }
    }
    out
}

/// `(table, key) -> scope literal` of every `[<table>.<key>]: <scope const>` entry of the scope tables.
fn scopes(tree: &dyn Tree) -> Result<HashMap<(String, String), String>, String> {
    let auth = tokens(tree, "auth.ts")?;
    let literals: HashMap<&str, &str> = (0..auth.len().saturating_sub(3)).filter(|&i| auth[i].ident() == Some("const") && auth[i + 2].is("=")).filter_map(|i| Some((auth[i + 1].ident()?, auth[i + 3].string()?))).collect();
    let mut out = HashMap::new();
    for (file, name) in SCOPE_TABLES {
        let path = format!("{REFERENCE}/{file}");
        let toks = tree.read(&path).map(|t| lex(&t)).ok_or_else(|| format!("{path} is missing"))?;
        let open = declaration(&toks, name).filter(|&i| toks[i].is("{")).ok_or_else(|| format!("{path} no longer declares {name} where the scan reads it"))?;
        let close = closing(&toks, open);
        for i in open + 1..close.saturating_sub(6) {
            let entry = toks[i].is("[") && toks[i + 2].is(".") && toks[i + 4].is("]") && toks[i + 5].is(":");
            if let (true, Some(table), Some(key), Some(scope)) = (entry, toks[i + 1].ident(), toks[i + 3].ident(), toks[i + 6].ident().and_then(|c| literals.get(c))) {
                out.insert((table.to_string(), key.to_string()), scope.to_string());
            }
        }
    }
    Ok(out)
}

/// `group -> [(endpoint, verb, path)]` of the environment `HttpApi`; a path may be a `const` of the file.
pub fn http_endpoints(toks: &[Token]) -> BTreeMap<String, Vec<(String, String, String)>> {
    let consts: HashMap<&str, &str> = (0..toks.len().saturating_sub(3)).filter(|&i| toks[i].ident() == Some("const") && toks[i + 2].is("=")).filter_map(|i| Some((toks[i + 1].ident()?, toks[i + 3].string()?))).collect();
    let (mut out, mut group) = (BTreeMap::<String, Vec<_>>::new(), None::<String>);
    for i in 0..toks.len().saturating_sub(7) {
        let at = |k: usize, s: &str| toks[i + k].text == s;
        if at(0, "HttpApiGroup") && at(2, "make") && at(3, "(") {
            group = toks[i + 4].string().map(String::from);
        } else if at(0, "HttpApiEndpoint") && at(1, ".") && at(3, "(") && at(5, ",") {
            let path = toks[i + 6].string().or_else(|| consts.get(toks[i + 6].text.as_str()).copied());
            if let (Some(g), Some(name), Some(path)) = (&group, toks[i + 4].string(), path) {
                out.entry(g.clone()).or_default().push((name.to_string(), toks[i + 2].text.to_uppercase(), path.to_string()));
            }
        }
    }
    out
}

struct Catalog {
    /// `(table, key) -> method`.
    methods: BTreeMap<(String, String), String>,
    commands: Vec<String>,
    mutations: Vec<String>,
    http: BTreeMap<String, Vec<(String, String, String)>>,
    rows: BTreeMap<String, Row>,
}

fn catalog(tree: &dyn Tree) -> Result<Catalog, String> {
    let (rpc, orchestration) = (tokens(tree, "rpc.ts")?, tokens(tree, "orchestrationV2.ts")?);
    let mut methods = BTreeMap::new();
    for (file, table) in METHOD_TABLES {
        let toks = if file == "rpc.ts" { &rpc } else { &orchestration };
        for (key, method) in string_object(toks, table) {
            methods.insert((table.to_string(), key), method);
        }
    }
    let stream = streams(&rpc);
    let scopes = scopes(tree)?;
    let commands = union_types(&orchestration, "OrchestrationV2Command");
    let mutations = union_types(&tokens(tree, "project.ts")?, "ProjectMutation");
    let http = http_endpoints(&tokens(tree, "environmentHttp.ts")?);
    if methods.is_empty() || commands.is_empty() || http.is_empty() {
        return Err(format!("{REFERENCE}/{CONTRACTS} no longer has WS_METHODS, the command union or the HttpApi groups where the scan reads them"));
    }
    let mut rows = BTreeMap::new();
    for ((table, key), method) in &methods {
        let mut row = Row::new("rpc", method);
        row.subscription = stream.contains(&(table.clone(), key.clone()));
        if let Some(scope) = scopes.get(&(table.clone(), key.clone())) {
            row.extra.insert("scope".into(), scope.clone().into());
        }
        rows.insert(row.id.clone(), row);
    }
    for (kind, names) in [("command", &commands), ("mutation", &mutations)] {
        for n in names {
            let row = Row::new(kind, n);
            rows.insert(row.id.clone(), row);
        }
    }
    for (group, endpoints) in &http {
        for (name, verb, path) in endpoints {
            let mut row = Row::new("http", &format!("{group}.{name}"));
            row.extra.insert("method".into(), verb.clone().into());
            row.extra.insert("path".into(), path.clone().into());
            rows.insert(row.id.clone(), row);
        }
    }
    Ok(Catalog { methods, commands, mutations, http, rows })
}

/// The HTTP groups a chunk addresses: `makeEnvironmentHttpApiGroupClient(url, "<group>")` or `group: "<group>"`.
fn chunk_groups(toks: &[Token], (a, b): (usize, usize), groups: &BTreeMap<String, Vec<(String, String, String)>>) -> Vec<String> {
    let mut out = vec![];
    for j in a..b {
        let Some(g) = toks[j].string().filter(|g| groups.contains_key(*g)) else { continue };
        let keyed = j >= 2 && toks[j - 1].is(":") && toks[j - 2].ident() == Some("group");
        let client = (j.saturating_sub(12).max(a)..j).any(|k| toks[k].ident().is_some_and(|i| i.contains("HttpApi")));
        if (keyed || client) && !out.iter().any(|o| o == g) {
            out.push(g.to_string());
        }
    }
    out
}

pub fn scan(tree: &dyn Tree, pin: Value) -> Result<MethodsList, String> {
    let mut cat = catalog(tree)?;
    let packages = resolve::packages(tree, &format!("{REFERENCE}/packages"));
    let resolver = |from: &str, spec: &str| {
        resolve::relative(tree, from, spec, &[".ts", ".tsx"]).or_else(|| spec.starts_with("@t3tools/").then(|| resolve::package(&packages, spec)).flatten().filter(|p| tree.is_file(p)))
    };
    let app = format!("{REFERENCE}/apps/mobile/");
    let contracts = format!("{REFERENCE}/packages/contracts/");
    let mut roots: Vec<String> = tree.files(&format!("{REFERENCE}/apps/mobile/src")).into_iter().filter(|f| (f.ends_with(".ts") || f.ends_with(".tsx")) && !is_test(f)).collect();
    roots.push(format!("{REFERENCE}/apps/mobile/index.ts"));
    let scope = Scope { roots, resolve: &resolver, excluded: &is_test, is_app: &|p: &str| p.starts_with(&app), scanned: &|p: &str| !p.starts_with(&contracts) };
    let graph = Graph::build(tree, &scope);
    let tables: HashSet<&str> = METHOD_TABLES.iter().map(|(_, t)| *t).collect();
    let typed: HashSet<&str> = cat.commands.iter().chain(&cat.mutations).map(String::as_str).collect();
    let mutations: HashSet<&str> = cat.mutations.iter().map(String::as_str).collect();
    let mut hits: Vec<(String, String)> = vec![];
    let (mut fetches, mut dynamic, mut groups_of) = (vec![], vec![], HashMap::<(String, usize), Vec<String>>::new());
    graph.each_live(|s: Site| {
        let (toks, at) = (s.toks, s.at);
        let t = &toks[at];
        let here = || site(REFERENCE, s.path, t.line);
        if !s.bound {
            return;
        }
        if let Some(open) = fetch_call(&s) {
            fetches.push(json!({"site": here(), "target": first_arg(toks, open)}));
        }
        if let (true, Some(key)) = (t.ident().is_some_and(|i| i.ends_with("lient")) && toks.get(at + 1).is_some_and(|b| b.is("[")) && toks.get(at + 3).is_some_and(|b| b.is("]")), toks.get(at + 2).and_then(Token::ident).filter(|k| k.starts_with(|c: char| c.is_ascii_lowercase()))) {
            dynamic.push(json!({"site": here(), "method": key}));
        }
        if let (Some(table), Some(dot), Some(key)) = (t.ident().filter(|i| tables.contains(i)), toks.get(at + 1), toks.get(at + 2)) {
            let typeof_ = at > 0 && toks[at - 1].ident() == Some("typeof");
            if let (true, false, Some(method)) = (dot.is("."), typeof_, cat.methods.get(&(table.to_string(), key.text.clone()))) {
                hits.push((format!("rpc:{method}"), here()));
            }
        }
        if let Some(v) = t.string().filter(|v| typed.contains(v) && !in_type_union(toks, at)) {
            let kind = if mutations.contains(v) { "mutation" } else { "command" };
            hits.push((format!("{kind}:{v}"), here()));
        }
        if let (Some(name), true) = (t.ident(), at > 0 && toks[at - 1].is(".") && toks.get(at + 1).is_some_and(|n| n.is("("))) {
            let full = (at >= 3 && toks[at - 3].is(".")).then(|| toks[at - 2].ident()).flatten().filter(|g| cat.http.get(*g).is_some_and(|e| e.iter().any(|(n, _, _)| n == name)));
            let groups = match full {
                Some(g) => vec![g.to_string()],
                None => groups_of.entry((s.path.to_string(), s.chunk.0)).or_insert_with(|| chunk_groups(toks, s.chunk, &cat.http)).clone(),
            };
            for g in groups.iter().filter(|g| cat.http[*g].iter().any(|(n, _, _)| n == name)) {
                hits.push((format!("http:{g}.{name}"), here()));
            }
        }
    });
    for (id, at) in hits {
        if let Some(row) = cat.rows.get_mut(&id) {
            row.reached(at);
        }
    }
    let rows: Vec<Row> = cat.rows.into_values().collect();
    let count = |kind: &str, phone: bool| rows.iter().filter(|r| r.kind == kind && (!phone || r.phone)).count();
    let subs = |phone: bool| rows.iter().filter(|r| r.subscription && (!phone || r.phone)).count();
    let counts = json!({
        "catalog": {"rpc": count("rpc", false), "subscriptions": subs(false), "commands": count("command", false), "mutations": count("mutation", false), "http": count("http", false)},
        "phone": {"rpc": count("rpc", true), "subscriptions": subs(true), "commands": count("command", true), "mutations": count("mutation", true), "http": count("http", true)},
        "files": {"reached": graph.files.len(), "phone": graph.files.keys().filter(|p| graph.is_app(p)).count()},
    });
    let flags = json!({
        "dynamicMethodNames": {"note": "an RPC client indexed by a variable: a generic dispatcher whose methods are counted where their tags are bound", "sites": dynamic},
        "rawFetches": {"note": "fetch calls on URLs the server or a third party hands out; not an endpoint of the environment HttpApi, so not a row", "sites": fetches},
        "nativeModules": native_modules(tree, &format!("{REFERENCE}/apps/mobile/modules"), REFERENCE),
    });
    Ok(MethodsList { app: "t3".into(), pin, counts, rows, flags })
}
