//! Option parsing for the commands `wf` serves itself: one spec-driven wrapper over `clap`, so every command reads
//! `--name value`, `--name=value`, flags and positionals the same way and a malformed command line gets clap's own error.
//! Clap's help and version flags are off: `wf <command> --help` is served from the stored help texts (`help.rs`).
use clap::{Arg, ArgAction, Command};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

thread_local! {
    static REPORTED: RefCell<Option<String>> = const { RefCell::new(None) };
    static JSON_SEEN: Cell<bool> = const { Cell::new(false) };
}

/// Whether a command line parsed so far carried `--json`, before or after the command: every command's own parse reads it.
pub fn json_seen() -> bool {
    JSON_SEEN.with(Cell::get)
}

/// The error the last failed parse rendered; `main` prints it in place of its generic "does not support" line.
pub fn take_error() -> Option<String> {
    REPORTED.with(|r| r.borrow_mut().take())
}

#[derive(Clone, Copy)]
pub enum Positional {
    None,
    Exactly(usize),
    AtMost(usize),
    Any,
}

/// What a command accepts. `values` take one argument (`--name v`, `--name=v`, a value may start with `-`), `variadic`
/// take one or more words, `optional` take a word only when one follows (`--summary [mode]`), `flags` take none.
pub struct Spec {
    pub values: &'static [&'static str],
    pub flags: &'static [&'static str],
    pub variadic: &'static [&'static str],
    pub optional: &'static [&'static str],
    /// `(short, long)`: `-p` for `--prompt`.
    pub short: &'static [(char, &'static str)],
    pub required: &'static [&'static str],
    pub positional: Positional,
}

impl Spec {
    pub const NONE: Spec = Spec { values: &[], flags: &[], variadic: &[], optional: &[], short: &[], required: &[], positional: Positional::Any };

    /// What the modes of one command accept together, for the command that picks its mode before it parses. Nothing is
    /// required and any positionals are allowed: each mode's own spec still decides what that mode takes. The lists are
    /// built once per process and kept.
    fn union(specs: &[&Spec]) -> Spec {
        fn merged<T: Copy + PartialEq>(lists: impl Iterator<Item = &'static [T]>) -> &'static [T] {
            let mut all: Vec<T> = Vec::new();
            for item in lists.flatten() {
                if !all.contains(item) {
                    all.push(*item);
                }
            }
            Box::leak(all.into_boxed_slice())
        }
        Spec {
            values: merged(specs.iter().map(|s| s.values)),
            flags: merged(specs.iter().map(|s| s.flags)),
            variadic: merged(specs.iter().map(|s| s.variadic)),
            optional: merged(specs.iter().map(|s| s.optional)),
            short: merged(specs.iter().map(|s| s.short)),
            required: &[],
            positional: Positional::Any,
        }
    }
}

#[derive(Clone)]
pub struct Parsed {
    pub opts: HashMap<String, String>,
    /// Every `--name value` in order, for options that may repeat.
    pub all: Vec<(String, String)>,
    pub flags: Vec<String>,
    pub positional: Vec<String>,
    /// Every occurrence in command-line order: a value option with its value, a flag with `None`.
    occurrences: Vec<(String, Option<String>)>,
}

impl Parsed {
    /// The last value given for `name`.
    pub fn val(&self, name: &str) -> Option<&str> {
        self.occurrences.iter().rev().find(|(n, v)| n == name && v.is_some()).and_then(|(_, v)| v.as_deref())
    }

    /// Every value given for `name`, in order.
    pub fn values(&self, name: &str) -> Vec<&str> {
        self.occurrences.iter().filter(|(n, _)| n == name).filter_map(|(_, v)| v.as_deref()).collect()
    }

    pub fn flag(&self, name: &str) -> bool {
        self.occurrences.iter().any(|(n, _)| n == name)
    }
}

fn base() -> Command {
    Command::new("wf").no_binary_name(true).disable_help_flag(true).disable_version_flag(true).disable_help_subcommand(true)
}

fn json_flag() -> Arg {
    Arg::new("json").long("json").hide(true).action(ArgAction::SetTrue).overrides_with("json")
}

/// The frontend `wf` acts on; commands that need a name list `frontend` themselves, every other command reads and drops it.
fn frontend_option() -> Arg {
    Arg::new("frontend").long("frontend").hide(true).num_args(1).allow_hyphen_values(true).action(ArgAction::Append)
}

fn command(spec: &Spec) -> Command {
    let long = |name: &'static str| {
        let arg = Arg::new(name).long(name).required(spec.required.contains(&name));
        match spec.short.iter().find(|(_, l)| *l == name) {
            Some((c, _)) => arg.short(*c),
            None => arg,
        }
    };
    let mut cmd = base().bin_name(crate::help::command_path(&std::env::args().skip(1).collect::<Vec<_>>()));
    for name in spec.values {
        cmd = cmd.arg(long(name).num_args(1).allow_hyphen_values(true).action(ArgAction::Append));
    }
    for name in spec.variadic {
        cmd = cmd.arg(long(name).num_args(1..).action(ArgAction::Append));
    }
    for name in spec.optional {
        cmd = cmd.arg(long(name).num_args(0..=1).default_missing_value("").action(ArgAction::Append));
    }
    for name in spec.flags {
        cmd = cmd.arg(long(name).action(ArgAction::SetTrue).overrides_with(*name));
    }
    // `wf`'s own options belong to every command, wherever they sit: `--json` unless the command lists it, and `--frontend`.
    if !spec.flags.contains(&"json") {
        cmd = cmd.arg(json_flag());
    }
    if !spec.values.contains(&"frontend") {
        cmd = cmd.arg(frontend_option());
    }
    match spec.positional {
        Positional::None => cmd,
        Positional::Exactly(n) => cmd.arg(Arg::new("positional").value_name("ARG").num_args(n..=n).required(n > 0)),
        Positional::AtMost(n) => cmd.arg(Arg::new("positional").value_name("ARG").num_args(0..=n)),
        Positional::Any => cmd.arg(Arg::new("positional").value_name("ARG").num_args(0..)),
    }
}

/// Parses `args` against `spec`. A command line clap refuses is rendered the way clap words it and kept for `take_error`.
pub fn parse_spec(args: &[String], spec: &Spec) -> Option<Parsed> {
    let matches = match command(spec).try_get_matches_from(args) {
        Ok(m) => m,
        Err(e) => {
            REPORTED.with(|r| *r.borrow_mut() = Some(e.render().to_string()));
            return None;
        }
    };
    if matches.get_flag("json") {
        JSON_SEEN.with(|j| j.set(true));
        crate::json_mode::begin();
    }
    let mut seen: Vec<(usize, String, Option<String>)> = Vec::new();
    for name in spec.values.iter().chain(spec.variadic).chain(spec.optional) {
        let values = matches.get_many::<String>(name).into_iter().flatten();
        let indices = matches.indices_of(name).into_iter().flatten();
        seen.extend(indices.zip(values).map(|(i, v)| (i, (*name).to_string(), Some(v.clone()))));
    }
    // Clap keeps only the position of a flag's last occurrence, and a repeated flag counts once.
    for name in spec.flags {
        if matches.get_flag(name) {
            seen.push((matches.indices_of(name).and_then(Iterator::last).unwrap_or(0), (*name).to_string(), None));
        }
    }
    if !spec.flags.contains(&"json") && matches.get_flag("json") {
        seen.push((matches.indices_of("json").and_then(Iterator::last).unwrap_or(0), "json".to_string(), None));
    }
    seen.sort_by_key(|(i, _, _)| *i);
    let occurrences: Vec<(String, Option<String>)> = seen.into_iter().map(|(_, n, v)| (n, v)).collect();
    let mut parsed = Parsed {
        opts: HashMap::new(),
        all: Vec::new(),
        flags: Vec::new(),
        positional: matches.try_get_many::<String>("positional").ok().flatten().map(|v| v.cloned().collect()).unwrap_or_default(),
        occurrences,
    };
    for (name, value) in &parsed.occurrences {
        match value {
            Some(v) => {
                parsed.opts.insert(name.clone(), v.clone());
                parsed.all.push((name.clone(), v.clone()));
            }
            None => parsed.flags.push(name.clone()),
        }
    }
    Some(parsed)
}

/// Parses `args` against what all the modes of a command accept together, to tell which mode was asked for by the options
/// that stand anywhere on the line. `None` when no mode takes the line, with clap's error kept for `take_error`: the union
/// is wider than any one mode, so a line it refuses is refused by every mode.
pub fn probe(args: &[String], modes: &[&Spec]) -> Option<Parsed> {
    parse_spec(args, &Spec::union(modes))
}

/// `wf <command> --json <word>` and `wf <command> --frontend <name> <word>`: the program options that stand between the command
/// and its first word move behind the command's words, where each command's own parse accepts them. The SDK of the browser
/// daemon sends `wf browser --json <command>`. A dangling `--frontend` stays where it is, for the command's parse to refuse.
pub(crate) fn lift_leading_options(rest: Vec<String>) -> Vec<String> {
    let Some((command, words)) = rest.split_first() else { return rest };
    let mut lifted: Vec<String> = Vec::new();
    let mut at = 0;
    while at < words.len() {
        match words[at].as_str() {
            "--json" => lifted.push("--json".into()),
            "--frontend" if at + 1 < words.len() => {
                lifted.extend([words[at].clone(), words[at + 1].clone()]);
                at += 1;
            }
            option if option.starts_with("--frontend=") => lifted.push(option.to_string()),
            _ => break,
        }
        at += 1;
    }
    if lifted.is_empty() {
        return rest;
    }
    let mut out = vec![command.clone()];
    out.extend(words[at..].iter().cloned());
    out.extend(lifted);
    out
}

/// What stands before the command: `wf`'s own options and the command with its words.
#[derive(Debug)]
pub(crate) struct Program {
    /// `--json` was given before the command.
    pub json: bool,
    /// `--help` or `--version` if asked for, then the command and everything after it, verbatim.
    pub rest: Vec<String>,
}

/// Reads the options that stand before the command (`--json`, `--frontend <name>`, `--help`, `-h`, `--version`, `-V`); the first
/// word is the command, and it and everything after it reach the command unread (an external subcommand in clap's terms).
/// A command line clap refuses comes back as its rendered error.
pub(crate) fn split_program(args: &[String]) -> Result<Program, String> {
    let matches = base()
        .bin_name("wf")
        .allow_external_subcommands(true)
        .external_subcommand_value_parser(clap::value_parser!(String))
        .arg(json_flag())
        .arg(frontend_option())
        .arg(Arg::new("help").long("help").short('h').action(ArgAction::SetTrue))
        .arg(Arg::new("version").long("version").short('V').action(ArgAction::SetTrue))
        .try_get_matches_from(args)
        .map_err(|e| e.render().to_string())?;
    let mut rest: Vec<String> = Vec::new();
    for (name, flag) in [("help", "--help"), ("version", "--version")] {
        if matches.get_flag(name) {
            rest.push(flag.to_string());
        }
    }
    if let Some((command, words)) = matches.subcommand() {
        rest.push(command.to_string());
        rest.extend(words.get_many::<String>("").into_iter().flatten().cloned());
    }
    Ok(Program { json: matches.get_flag("json"), rest })
}

/// The common shape without positionals: a stray word is clap's error.
pub fn parse_none(args: &[String], values: &'static [&'static str], flags: &'static [&'static str]) -> Option<Parsed> {
    parse_spec(args, &Spec { values, flags, positional: Positional::None, ..Spec::NONE })
}

/// The common shape: value options and flags, any positionals.
pub fn parse(args: &[String], values: &'static [&'static str], flags: &'static [&'static str]) -> Option<Parsed> {
    parse_spec(args, &Spec { values, flags, ..Spec::NONE })
}
