//! The one dispatcher. The full `wf` and the thin test binary of every `wf-<name>` crate run it, each with the commands and the
//! help tables it serves, so a crate's flow tests meet the same argument handling as the real binary.
use crate::{args, env_file, help, usage};
use std::time::Instant;

/// A command: the words after its name, and `None` for a shape it does not serve (reported as unsupported).
pub type Handler = fn(&[String]) -> Option<i32>;

/// What a binary serves.
pub struct Surface {
    /// Printed by `--version`.
    pub version: &'static str,
    /// Every command by its first word.
    pub commands: &'static [(&'static str, Handler)],
    pub help: &'static [&'static [help::Row]],
    pub hooks: help::Hooks,
    /// The usage row of the full `wf`; a thin binary records none.
    pub recorder: Option<Recorder>,
}

/// The usage row: `send_if_asked` runs in the detached copy that posts it, `record` when a command ends.
pub struct Recorder {
    pub send_if_asked: fn() -> Option<i32>,
    pub record: fn(&[String], i32, Instant, &str),
}

/// Serves the command line the process was started with and exits with its code.
pub fn run(surface: &Surface) -> ! {
    help::register(surface.help, surface.hooks);
    if let Some(code) = surface.recorder.as_ref().and_then(|r| (r.send_if_asked)()) {
        std::process::exit(code);
    }
    let started = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let program = match args::split_program(&args) {
        Ok(program) => program,
        Err(error) => {
            eprint!("{error}");
            std::process::exit(2);
        }
    };
    // The options of `wf` itself that stand before the command (`--json`, `--frontend <name>`) are read here: `--json` moves
    // behind the command, where the commands look for it, and the one frontend is the default. After the command, each
    // command's own parse accepts both.
    let mut rest = args::lift_leading_options(program.rest);
    if program.json {
        rest.push("--json".to_string());
    }
    std::env::set_var("WF_APP", "mla-tamagui");
    let code = dispatch(surface, &rest);
    if let Some(recorder) = &surface.recorder {
        (recorder.record)(&rest, code, started, surface.version);
    }
    std::process::exit(code);
}

/// Serves help, then runs the command named by the first word with the words after it.
fn dispatch(surface: &Surface, args: &[String]) -> i32 {
    if let Some(code) = help::serve(args) {
        return code;
    }
    match args.first().map(String::as_str) {
        Some("-V") | Some("--version") => {
            println!("{}", surface.version);
            0
        }
        Some(name) => match surface.commands.iter().find(|(n, _)| *n == name) {
            Some((_, run)) => {
                let loaded = env_file::load();
                run(&args[1..]).unwrap_or_else(|| {
                    env_file::unload(&loaded);
                    unsupported(surface, args)
                })
            }
            None => unsupported(surface, args),
        },
        None => unsupported(surface, args),
    }
}

/// What an argument shape no command serves reports: commander's wording for a command that does not exist, and a usage
/// pointer (exit 2) for a known command given arguments it does not take.
fn unsupported(surface: &Surface, args: &[String]) -> i32 {
    if let Some(error) = args::take_error() {
        usage::note_error(&error);
        eprint!("{error}");
        return 2;
    }
    match args.first().map(String::as_str) {
        Some(name) if surface.commands.iter().any(|(n, _)| *n == name) || help::is_top_level(name) => {
            crate::wf_err!("wf {name}: the Rust wf does not support these arguments (see wf {name} --help)");
            2
        }
        Some(name) => {
            eprintln!("error: unknown command '{name}'");
            1
        }
        None => 1,
    }
}
