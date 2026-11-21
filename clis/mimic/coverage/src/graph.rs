//! The phone's code as an import graph, and which of it runs: every chunk of a phone file, and a library declaration once live
//! code of a file importing it names it. A token under an object key no live code reads (`.key`) is unbound.
use crate::lex::{lex, Kind, Token};
use crate::ports::Tree;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    Decl,
    Stmt,
    Import,
    Reexport,
}

#[derive(Debug)]
pub struct Chunk {
    pub names: Vec<String>,
    pub start: usize,
    pub end: usize,
    pub kind: ChunkKind,
}

pub struct SourceFile {
    pub toks: Vec<Token>,
    pub imports: Vec<String>,
    pub reexports: Vec<String>,
    pub chunks: Vec<Chunk>,
}

const DECL_WORDS: [&str; 9] = ["const", "let", "var", "function", "class", "type", "interface", "enum", "namespace"];
const MODIFIERS: [&str; 5] = ["export", "default", "declare", "async", "abstract"];
const NOT_KEYS: [&str; 7] = ["if", "for", "while", "switch", "catch", "function", "return"];
const OBJECT_AFTER: [&str; 15] = ["=", "(", ",", ":", "[", "?", "{", "return", "|", "&", "!", ".", "yield", "await", "satisfies"];

/// Top-level statements start at column 0 at bracket depth 0; each runs until the next one.
pub fn chunks(toks: &[Token]) -> Vec<Chunk> {
    let mut starts = Vec::new();
    let mut depth = 0i32;
    for (i, t) in toks.iter().enumerate() {
        if depth == 0 && t.col == 0 && t.kind == Kind::Ident && !t.nested {
            starts.push(i);
        }
        if t.kind == Kind::Punct {
            match t.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = (depth - 1).max(0),
                _ => {}
            }
        }
    }
    if starts.first() != Some(&0) {
        starts.insert(0, 0);
    }
    starts.push(toks.len());
    starts
        .windows(2)
        .filter(|w| w[0] < w[1])
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let word = |k: usize| toks.get(k).filter(|_| k < b).map(|t| t.text.as_str());
            let mut j = a;
            while word(j).is_some_and(|w| MODIFIERS.contains(&w)) && toks[j].kind == Kind::Ident {
                j += 1;
            }
            let (kind, names) = if word(j).is_some_and(|w| DECL_WORDS.contains(&w)) {
                (ChunkKind::Decl, declared_names(toks, j + 1, b))
            } else if word(a) == Some("import") {
                (ChunkKind::Import, vec![])
            } else if word(a) == Some("export") && (matches!(word(a + 1), Some("{" | "*")) || (word(a + 1) == Some("type") && word(a + 2) == Some("{"))) {
                (ChunkKind::Reexport, vec![])
            } else {
                (ChunkKind::Stmt, vec![])
            };
            Chunk { names, start: a, end: b, kind }
        })
        .collect()
}

fn declared_names(toks: &[Token], mut k: usize, end: usize) -> Vec<String> {
    if toks.get(k).is_some_and(|t| t.is("*")) {
        k += 1;
    }
    match toks.get(k) {
        Some(t) if k < end && t.kind == Kind::Ident => vec![t.text.clone()],
        Some(t) if k < end && (t.is("{") || t.is("[")) => {
            let (mut depth, mut names) = (0, vec![]);
            for t in &toks[k..end] {
                if t.is("{") || t.is("[") {
                    depth += 1;
                } else if t.is("}") || t.is("]") {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if depth == 1 && t.kind == Kind::Ident {
                    names.push(t.text.clone());
                }
            }
            names
        }
        _ => vec![],
    }
}

/// The module specifiers of a chunk: `from "x"`, `import "x"`, `import("x")` and `require("x")`.
fn specifiers<'t>(toks: &'t [Token], chunk: &Chunk) -> Vec<&'t str> {
    (chunk.start..chunk.end)
        .filter_map(|i| {
            let s = toks[i].string()?;
            let prev = |k: usize| i.checked_sub(k).map(|p| toks[p].text.as_str());
            let from = prev(1) == Some("from") && toks[i - 1].kind == Kind::Ident;
            let bare = chunk.kind == ChunkKind::Import && i == chunk.start + 1;
            let call = prev(1) == Some("(") && matches!(prev(2), Some("import" | "require"));
            (from || bare || call).then_some(s)
        })
        .collect()
}

/// Whether the `(` at `toks[j]` opens a call's arguments rather than a group or a parameter list's default.
fn opens_call(toks: &[Token], j: usize, a: usize) -> bool {
    let Some(prev) = (j > a).then(|| &toks[j - 1]) else { return false };
    match prev.kind {
        Kind::Ident => !NOT_KEYS.contains(&prev.text.as_str()) && !matches!(prev.text.as_str(), "typeof" | "await" | "yield" | "in" | "of" | "new" | "else" | "do"),
        Kind::Punct => matches!(prev.text.as_str(), ")" | "]" | ">"),
        _ => false,
    }
}

struct Open {
    object: bool,
    call: bool,
    /// Inside an object handed to a call: the callee, often a library nobody scans, reads its keys.
    muted: bool,
    key: Option<String>,
}

/// For each token of `toks[a..b]`, the object keys it sits under, outermost first. A key whose value is the token itself
/// (`tag: X`) is left out, so `X`'s binding is the key above it, and keys of an object passed as a call argument never gate.
pub fn key_chains(toks: &[Token], a: usize, b: usize) -> Vec<Vec<String>> {
    let mut stack: Vec<Open> = Vec::new();
    let mut out = Vec::with_capacity(b - a);
    for j in a..b {
        let t = &toks[j];
        if t.kind == Kind::Punct && matches!(t.text.as_str(), "{" | "(" | "[") {
            // A `{` after `)`, `=>`, `;` or a keyword opens a block, whose statements are no keys.
            let object = t.text == "{" && j > a && OBJECT_AFTER.contains(&toks[j - 1].text.as_str()) && toks[j - 1].kind != Kind::Str;
            let parent = stack.last();
            let muted = parent.is_some_and(|p| p.muted || (object && p.call));
            stack.push(Open { object, call: t.text == "(" && opens_call(toks, j, a), muted, key: None });
        } else if t.kind == Kind::Punct && matches!(t.text.as_str(), "}" | ")" | "]") {
            stack.pop();
        } else if let Some(Open { object: true, key, .. }) = stack.last_mut() {
            let prev = (j > a).then(|| toks[j - 1].text.as_str());
            let next = toks.get(j + 1).filter(|_| j + 1 < b).map(|n| n.text.as_str());
            if matches!(t.kind, Kind::Ident | Kind::Str) && matches!(prev, Some("{" | ",")) && matches!(next, Some(":" | "(")) && !NOT_KEYS.contains(&t.text.as_str()) {
                *key = Some(t.text.clone());
            } else if t.is(",") {
                *key = None;
            }
        }
        let mut chain: Vec<String> = stack.iter().filter(|o| o.object && !o.muted).filter_map(|o| o.key.clone()).collect();
        if j > 0 && toks[j - 1].is(":") && stack.last().is_some_and(|o| o.object && !o.muted && o.key.is_some()) {
            chain.pop();
        }
        out.push(chain);
    }
    out
}

pub struct Scope<'a> {
    pub roots: Vec<String>,
    pub resolve: &'a dyn Fn(&str, &str) -> Option<String>,
    /// Files never entered (tests, fixtures).
    pub excluded: &'a dyn Fn(&str) -> bool,
    /// Phone files: every chunk of them is live.
    pub is_app: &'a dyn Fn(&str) -> bool,
    /// Library files whose declarations can become live; the contracts that define the catalog are no call sites.
    pub scanned: &'a dyn Fn(&str) -> bool,
}

pub struct Graph {
    pub files: BTreeMap<String, SourceFile>,
    live: HashMap<String, Vec<bool>>,
    app: HashSet<String>,
    member: HashSet<String>,
}

/// One token of a live chunk.
pub struct Site<'g> {
    pub path: &'g str,
    pub toks: &'g [Token],
    pub at: usize,
    /// False when the token sits under an object key nothing reads.
    pub bound: bool,
    /// The names the chunk declares (empty for a statement).
    pub declares: &'g [String],
    /// The chunk's token range.
    pub chunk: (usize, usize),
}

impl Graph {
    pub fn build(tree: &dyn Tree, scope: &Scope) -> Graph {
        let mut files = BTreeMap::new();
        let mut todo: Vec<String> = scope.roots.clone();
        while let Some(path) = todo.pop() {
            if files.contains_key(&path) || (scope.excluded)(&path) {
                continue;
            }
            let Some(text) = tree.read(&path) else { continue };
            let toks = lex(&text);
            let chunks = chunks(&toks);
            let (mut imports, mut reexports) = (vec![], vec![]);
            for chunk in &chunks {
                for spec in specifiers(&toks, chunk) {
                    if let Some(target) = (scope.resolve)(&path, spec) {
                        if chunk.kind == ChunkKind::Reexport {
                            reexports.push(target.clone());
                        }
                        imports.push(target);
                    }
                }
            }
            todo.extend(imports.iter().cloned());
            files.insert(path, SourceFile { toks, imports, reexports, chunks });
        }
        let app: HashSet<String> = files.keys().filter(|p| (scope.is_app)(p)).cloned().collect();
        let mut graph = Graph { files, live: HashMap::new(), app, member: HashSet::new() };
        graph.settle(scope);
        graph
    }

    /// Marks live chunks to a fixpoint, then collects the keys live code reads.
    fn settle(&mut self, scope: &Scope) {
        let files = &self.files;
        let mut seen_by: HashMap<&str, HashSet<&str>> = HashMap::new();
        for (path, file) in files {
            let (mut seen, mut stack): (HashSet<&str>, Vec<&str>) = (HashSet::new(), file.imports.iter().map(String::as_str).collect());
            while let Some(q) = stack.pop() {
                if let Some(f) = files.get(q) {
                    if seen.insert(q) {
                        stack.extend(f.reexports.iter().map(String::as_str));
                    }
                }
            }
            for q in seen {
                seen_by.entry(q).or_default().insert(path.as_str());
            }
        }
        let ids = |file: &SourceFile, c: &Chunk| -> HashSet<String> { file.toks[c.start..c.end].iter().filter(|t| t.kind == Kind::Ident).map(|t| t.text.clone()).collect() };
        let mut live: HashMap<String, Vec<bool>> = HashMap::new();
        let mut named: HashMap<&str, HashSet<String>> = HashMap::new();
        for (path, file) in files {
            let is_app = self.app.contains(path);
            let flags: Vec<bool> = file.chunks.iter().map(|c| is_app || (c.kind == ChunkKind::Stmt && (scope.scanned)(path))).collect();
            for (c, _) in file.chunks.iter().zip(&flags).filter(|(_, l)| **l) {
                named.entry(path.as_str()).or_default().extend(ids(file, c));
            }
            live.insert(path.clone(), flags);
        }
        loop {
            let mut woke: Vec<(&str, usize)> = vec![];
            for (path, file) in files.iter().filter(|(p, _)| !self.app.contains(*p) && (scope.scanned)(p)) {
                let users: Vec<&str> = seen_by.get(path.as_str()).into_iter().flatten().copied().chain([path.as_str()]).collect();
                for (i, c) in file.chunks.iter().enumerate() {
                    if live[path][i] || c.kind != ChunkKind::Decl || c.names.is_empty() {
                        continue;
                    }
                    if users.iter().any(|u| named.get(u).is_some_and(|s| c.names.iter().any(|n| s.contains(n)))) {
                        woke.push((path.as_str(), i));
                    }
                }
            }
            if woke.is_empty() {
                break;
            }
            for (path, i) in woke {
                live.get_mut(path).expect("known file")[i] = true;
                let file = &files[path];
                named.entry(path).or_default().extend(ids(file, &file.chunks[i]));
            }
        }
        let mut member = HashSet::new();
        for (path, file) in files {
            for (c, _) in file.chunks.iter().zip(&live[path]).filter(|(_, l)| **l) {
                for j in c.start + 1..c.end {
                    if file.toks[j].kind == Kind::Ident && file.toks[j - 1].is(".") {
                        member.insert(file.toks[j].text.clone());
                    }
                }
            }
        }
        self.live = live;
        self.member = member;
    }

    pub fn is_app(&self, path: &str) -> bool {
        self.app.contains(path)
    }

    /// Every token of every live chunk, in path order.
    pub fn each_live(&self, mut f: impl FnMut(Site)) {
        for (path, file) in &self.files {
            let is_app = self.app.contains(path);
            for (c, _) in file.chunks.iter().zip(&self.live[path]).filter(|(_, l)| **l) {
                let chains = (!is_app).then(|| key_chains(&file.toks, c.start, c.end));
                for at in c.start..c.end {
                    let bound = chains.as_ref().is_none_or(|ch| ch[at - c.start].first().is_none_or(|k| self.member.contains(k)));
                    f(Site { path, toks: &file.toks, at, bound, declares: &c.names, chunk: (c.start, c.end) });
                }
            }
        }
    }
}

/// Whether `toks[at]` sits beside a `|`: a member of a type union, not a value.
pub fn in_type_union(toks: &[Token], at: usize) -> bool {
    (at > 0 && toks[at - 1].is("|")) || toks.get(at + 1).is_some_and(|t| t.is("|"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MemoryTree;

    fn graph(tree: &MemoryTree, roots: &[&str]) -> Graph {
        let resolve = |from: &str, spec: &str| crate::resolve::relative(tree, from, spec, &[".ts"]);
        let scope = Scope { roots: roots.iter().map(|r| r.to_string()).collect(), resolve: &resolve, excluded: &|p| p.contains(".test."), is_app: &|p| p.starts_with("app/"), scanned: &|_| true };
        Graph::build(tree, &scope)
    }

    fn live_strings(g: &Graph) -> Vec<String> {
        let mut out = vec![];
        g.each_live(|s| {
            if let (Some(v), true) = (s.toks[s.at].string().filter(|v| !v.starts_with('.')), s.bound) {
                out.push(v.to_string());
            }
        });
        out.sort();
        out
    }

    #[test]
    fn a_library_declaration_counts_only_when_the_phone_names_it() {
        // Given a phone file that imports one of two library functions
        let tree = MemoryTree::of(&[
            ("app/main.ts", "import { used } from \"../lib/calls.ts\";\nused();\n"),
            ("lib/calls.ts", "export function used() { call(\"a.used\") }\nexport function unused() { call(\"a.unused\") }\n"),
        ]);
        // When the graph settles
        let g = graph(&tree, &["app/main.ts"]);
        // Then only the used declaration's literal is live
        assert_eq!(live_strings(&g), ["a.used"]);
    }

    #[test]
    fn a_factory_member_counts_only_when_some_live_code_reads_its_key() {
        // Given a factory of two members, of which the phone reads one
        let tree = MemoryTree::of(&[
            ("app/main.ts", "import { atoms } from \"../lib/atoms.ts\";\nconst a = atoms();\na.read;\n"),
            ("lib/atoms.ts", "export function atoms() {\n  return {\n    read: make({ tag: \"m.read\" }),\n    write: make({ tag: \"m.write\" }),\n  };\n}\n"),
        ]);
        // Then the read member's tag is bound and the write member's is not
        assert_eq!(live_strings(&tree_graph(&tree)), ["m.read"]);
    }

    #[test]
    fn a_callback_in_an_object_handed_to_a_library_counts_though_nothing_scanned_reads_its_key() {
        // Given a factory the phone calls, whose resolver hands a library a config object with a callback, and whose returned
        // object holds a member the phone never reads
        let tree = MemoryTree::of(&[
            ("app/main.ts", "import { atoms } from \"../lib/atoms.ts\";\nconst a = atoms();\na.read;\n"),
            (
                "lib/atoms.ts",
                "export function atoms() {\n  const resolver = Resolver.makeWith<Req>({\n    runAll: Effect.fn(function* (all) {\n      return yield* request(\"m.batch\", { all });\n    }),\n  });\n  return {\n    read: make(resolver, { tag: \"m.read\" }),\n    write: make(resolver, { tag: \"m.write\" }),\n  };\n}\n",
            ),
        ]);
        // Then the callback's literal is bound, and the unread member's still is not
        assert_eq!(live_strings(&tree_graph(&tree)), ["m.batch", "m.read"]);
    }

    fn tree_graph(tree: &MemoryTree) -> Graph {
        graph(tree, &["app/main.ts"])
    }

    #[test]
    fn a_barrel_reexport_carries_the_phone_to_the_declaring_file_and_tests_are_never_entered() {
        // Given a phone file importing through an index barrel, and a test beside it
        let tree = MemoryTree::of(&[
            ("app/main.ts", "import { go } from \"../lib/index.ts\";\nimport \"./main.test.ts\";\ngo();\n"),
            ("app/main.test.ts", "call(\"t.test\")\n"),
            ("lib/index.ts", "export * from \"./go.ts\";\n"),
            ("lib/go.ts", "export const go = () => call(\"g.go\");\n"),
        ]);
        assert_eq!(live_strings(&tree_graph(&tree)), ["g.go"]);
    }
}
