//! `wf --help`, `wf <command> --help`, `wf help [command]` and a bare command group, served from the help tables that the
//! binary registers at start (`cli::run`): `(command path, a bare invocation prints the help to stderr and exits 1, help text)`.
use std::io::Write;
use std::sync::OnceLock;

pub(crate) type Row = (&'static str, bool, &'static str);

/// What the device harness adds to the table: the verbs of its command table, generated from that table.
#[derive(Clone, Copy)]
pub struct Hooks {
    /// The generated verb list that follows the committed text of a device noun (`browser`, `android`).
    pub noun_section: fn(&str) -> Option<String>,
    /// The help of `<noun> <verb>` for a verb of the device command table.
    pub verb_help: fn(&str, &str) -> Option<String>,
}

fn no_section(_: &str) -> Option<String> {
    None
}

fn no_verb_help(_: &str, _: &str) -> Option<String> {
    None
}

impl Hooks {
    /// For a binary without the device commands.
    pub const NONE: Hooks = Hooks { noun_section: no_section, verb_help: no_verb_help };
}

struct Registry {
    tables: &'static [&'static [Row]],
    hooks: Hooks,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

pub fn register(tables: &'static [&'static [Row]], hooks: Hooks) {
    assert!(REGISTRY.set(Registry { tables, hooks }).is_ok(), "help::register runs once, at start");
}

fn registry() -> &'static Registry {
    REGISTRY.get().expect("help::register runs at start, before any help is served")
}

fn rows() -> impl Iterator<Item = &'static Row> {
    registry().tables.iter().flat_map(|table| table.iter())
}

fn find(path: &str) -> Option<&'static Row> {
    rows().find(|(p, _, _)| *p == path)
}

/// Whether `name` is a top-level command in the help table.
pub(crate) fn is_top_level(name: &str) -> bool {
    rows().any(|(p, _, _)| p.split(' ').next() == Some(name))
}

fn has_children(path: &str) -> bool {
    rows().any(|(p, _, _)| {
        let rest = if path.is_empty() { Some(*p) } else { p.strip_prefix(path).and_then(|r| r.strip_prefix(' ')) };
        rest.is_some_and(|r| !r.is_empty() && !r.contains(' ') && *p != path)
    })
}

fn print_out(text: &str) -> i32 {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
    0
}

fn print_err(text: &str) -> i32 {
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(text.as_bytes());
    let _ = err.flush();
    1
}

/// `wf <command path>` for the words of `args` that name a command in the help table, for usage lines.
pub fn command_path(args: &[String]) -> String {
    let words: Vec<&str> = args.iter().map(String::as_str).take_while(|a| !a.starts_with('-')).collect();
    for n in (1..=words.len()).rev() {
        if find(&words[..n].join(" ")).is_some() {
            return format!("wf {}", words[..n].join(" "));
        }
    }
    "wf".to_string()
}

/// A help text, with the device verbs generated from the command table under the two device nouns.
fn full(path: &str, text: &str) -> String {
    match (registry().hooks.noun_section)(path) {
        Some(section) => format!("{text}{section}"),
        None => text.to_string(),
    }
}

/// `wf browser|android <verb> ... --help` (or -h, anywhere after the verb) for a verb of the device command table.
fn device_help(args: &[String]) -> Option<i32> {
    let [noun, verb, rest @ ..] = args else { return None };
    if !rest.iter().any(|a| a == "--help" || a == "-h") {
        return None;
    }
    (registry().hooks.verb_help)(noun, verb).map(|text| print_out(&text))
}

/// The committed help text of a command path (`browser start-device`).
pub fn text(path: &str) -> Option<&'static str> {
    find(path).map(|(_, _, text)| *text)
}

/// `Some(exit code)` when the argv is a help request this table covers.
pub(crate) fn serve(args: &[String]) -> Option<i32> {
    if args.is_empty() {
        return find("").map(|(_, _, text)| print_err(text));
    }
    if let Some(code) = device_help(args) {
        return Some(code);
    }
    if args[0] == "help" {
        return match args.len() {
            1 => find("").map(|(_, _, text)| print_out(text)),
            2 if !args[1].starts_with('-') => find(&args[1]).map(|(path, _, text)| print_out(&full(path, text))),
            _ => None,
        };
    }
    let (last, rest) = args.split_last()?;
    if (last == "--help" || last == "-h") && rest.iter().all(|a| !a.starts_with('-')) {
        // The longest known command path; anything after a leaf command is its arguments.
        for n in (0..=rest.len()).rev() {
            let path = rest[..n].join(" ");
            if let Some((_, _, text)) = find(&path) {
                if n == rest.len() || !has_children(&path) {
                    return Some(print_out(&full(&path, text)));
                }
                return None;
            }
        }
        return None;
    }
    if args.iter().all(|a| !a.starts_with('-')) {
        let path = args.join(" ");
        if let Some((_, true, text)) = find(&path) {
            return Some(print_err(&full(&path, text)));
        }
    }
    None
}
