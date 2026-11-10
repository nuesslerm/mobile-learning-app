//! `opencode run` and `wf offload ask` side by side with a real model: the same prompt, options and model through both, in checkouts of
//! the same shape, and what each leaves (exit code, stdout, the JSON events, files, session) compared. It runs with the CLI e2e tests
//! (the `cargo-e2e` task of this crate, so Nx replays it while the crate and the option table are unchanged); it needs the `opencode`
//! binary on PATH (the cli-e2e job installs it), the vault key `OPENCODE_GO_API_KEY` and the tailnet to the service. The model is
//! pinned and the prompts are deterministic, but a model may still word an answer differently, so the checks are on the marker each
//! prompt asks for and on the structure of what is printed, not on whole answers.
use serde_json::Value;
use std::process::Command;
use std::sync::OnceLock;

const MODEL: &str = "opencode-go/deepseek-v4.1-flash";
const READ_ONLY: &str = include_str!("../../../../.opencode/agents/read-only.md");

pub(crate) struct Out {
    pub(crate) code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// The vault key the opencode binary authenticates with, read by the test and handed only to the child it starts.
fn key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        let out = Command::new(std::env::var("WF_BIN").unwrap_or_else(|_| "wf".into())).args(["secret", "get", "OPENCODE_GO_API_KEY", "--backend", "bitwarden"]).output().expect("the wf binary starts");
        assert!(out.status.success() && !out.stdout.is_empty(), "OPENCODE_GO_API_KEY is not in the vault for this machine: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    })
}

fn plain(bytes: &[u8]) -> String {
    let raw = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The caller name a test's asks go under: every session the test makes is the service's to hold for the test only, and `purge` drops them.
pub(crate) fn caller_of(dir: &std::path::Path) -> String {
    format!("e2e-{}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Tells the service the test is done with the sessions it made: they are deleted from opencode (which gives their memory back) and from
/// the service's disk now, not when they expire. A service that cannot be reached is left to its expiry.
pub(crate) fn purge(caller: &str) {
    let _ = Command::new(crate::support::wf_binary()).args(["offload", "sessions", "rm", "--caller", caller]).env("WF_USAGE", "off").output();
}

/// A checkout and the home the opencode binary keeps its sessions in, one pair per side so neither sees the other's.
pub(crate) struct Side {
    pub(crate) dir: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Drop for Side {
    fn drop(&mut self) {
        purge(&caller_of(self.dir.path()));
    }
}

pub(crate) fn side(files: &[(&str, &str)]) -> Side {
    let dir = tempfile::Builder::new().prefix("wf-parity-").tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    for (path, content) in files.iter().chain(&[(".opencode/agents/read-only.md", READ_ONLY)]) {
        let target = dir.path().join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, content).unwrap();
    }
    Side { dir, home: tempfile::tempdir().unwrap() }
}

/// One npm cache for every opencode the tests start: each has a home of its own, and opencode installs the packages of its plugins and
/// tools there, a hundred megabytes a home.
fn npm_cache() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("wf-parity-npm-cache");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// At most this many model-driving processes at once, whatever the number of test threads: the provider answers 429 to a crowd, and a
/// run it refuses is a failure of the test, not of what it checks (`WF_PARITY_CONCURRENCY`, four by default).
struct Limiter {
    used: std::sync::Mutex<usize>,
    free: std::sync::Condvar,
    limit: usize,
}

fn limiter() -> &'static Limiter {
    static LIMITER: OnceLock<Limiter> = OnceLock::new();
    LIMITER.get_or_init(|| Limiter { used: std::sync::Mutex::new(0), free: std::sync::Condvar::new(), limit: std::env::var("WF_PARITY_CONCURRENCY").ok().and_then(|v| v.parse().ok()).unwrap_or(4) })
}

/// Holds one of the slots until it is dropped.
struct Slot;

fn slot() -> Slot {
    let limiter = limiter();
    let mut used = limiter.used.lock().unwrap();
    while *used >= limiter.limit {
        used = limiter.free.wait(used).unwrap();
    }
    *used += 1;
    Slot
}

impl Drop for Slot {
    fn drop(&mut self) {
        let limiter = limiter();
        *limiter.used.lock().unwrap() -= 1;
        limiter.free.notify_one();
    }
}

/// Runs the command to its end with `input` (or an empty stdin) and collects what it printed.
fn finish(mut command: Command, input: Option<&str>) -> Out {
    use std::io::Write;
    let _slot = slot();
    let mut child = command.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().expect("the command starts");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.unwrap_or_default().as_bytes());
    }
    let out = child.wait_with_output().expect("the command ends");
    Out { code: out.status.code().unwrap_or(-1), stdout: plain(&out.stdout), stderr: plain(&out.stderr) }
}

impl Side {
    pub(crate) fn opencode(&self, args: &[&str]) -> Out {
        self.opencode_in(args, None)
    }

    pub(crate) fn ask(&self, args: &[&str]) -> Out {
        self.ask_in(args, None)
    }

    /// `opencode run` with `input` on its stdin (none: nothing to read).
    pub(crate) fn opencode_in(&self, args: &[&str], input: Option<&str>) -> Out {
        let mut command = Command::new("opencode");
        command.arg("run").args(args).args(["-m", MODEL]).current_dir(self.dir.path()).env("HOME", self.home.path()).env("PWD", self.dir.path()).env("OPENCODE_API_KEY", key()).env("npm_config_cache", npm_cache());
        finish(command, input)
    }

    /// `opencode run` with the arguments as they are (the model among them).
    pub(crate) fn opencode_without_model(&self, args: &[&str]) -> Out {
        let mut command = Command::new("opencode");
        command.arg("run").args(args).current_dir(self.dir.path()).env("HOME", self.home.path()).env("PWD", self.dir.path()).env("OPENCODE_API_KEY", key()).env("npm_config_cache", npm_cache());
        finish(command, None)
    }

    /// `wf offload ask` with the arguments as they are (the model among them).
    pub(crate) fn ask_without_model(&self, args: &[&str]) -> Out {
        let mut command = Command::new(crate::support::wf_binary());
        command.args(["offload", "ask"]).args(args).args(["--timeout-ms", "600000", "--caller", &caller_of(self.dir.path())]).current_dir(self.dir.path()).env("WF_USAGE", "off");
        finish(command, None)
    }

    /// `wf offload ask` with `input` on its stdin.
    pub(crate) fn ask_in(&self, args: &[&str], input: Option<&str>) -> Out {
        let mut command = Command::new(crate::support::wf_binary());
        command.args(["offload", "ask"]).args(args).args(["-m", MODEL, "--timeout-ms", "600000", "--caller", &caller_of(self.dir.path())]).current_dir(self.dir.path()).env("WF_USAGE", "off");
        finish(command, input)
    }

    /// Both CLIs, the same arguments, on this pair of sides.
    pub(crate) fn both(a: &Side, b: &Side, args: &[&str]) -> (Out, Out) {
        (a.opencode(args), b.ask(args))
    }

    /// Both CLIs on a prompt that reads files by tool: `opencode run` sees the checkout it runs in, while `ask` has no checkout on the
    /// service and sees only the files it is told to send (`--files`, which stages them without attaching them to the message).
    pub(crate) fn both_reading(a: &Side, b: &Side, args: &[&str], files: &[&str]) -> (Out, Out) {
        let listed = files.join(",");
        let mut with_files = args.to_vec();
        with_files.extend(["--files", &listed]);
        (a.opencode(args), b.ask(&with_files))
    }

    pub(crate) fn read(&self, file: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(file)).unwrap_or_default()
    }
}

pub(crate) fn say(out: &Out, marker: &str) -> bool {
    out.stdout.to_uppercase().contains(&marker.to_uppercase())
}

/// Both ran, and both said `marker` on stdout.
pub(crate) fn both_say(marker: &str, opencode: &Out, ask: &Out) {
    for (who, out) in [("opencode run", opencode), ("wf offload ask", ask)] {
        assert_eq!(out.code, 0, "{who} exited {}:\n{}{}", out.code, out.stdout, out.stderr);
        assert!(say(out, marker), "{who} did not say {marker}:\n{}{}", out.stdout, out.stderr);
    }
}

/// The events `--format json` printed, one per line.
pub(crate) fn events(out: &Out) -> Vec<Value> {
    out.stdout.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect()
}

pub(crate) fn session_of(out: &Out) -> String {
    events(out).iter().find_map(|v| v["sessionID"].as_str().map(str::to_string)).unwrap_or_else(|| panic!("no sessionID in:\n{}{}", out.stdout, out.stderr))
}

/// The names of the tools the turn called, in order.
pub(crate) fn tools(out: &Out) -> Vec<String> {
    events(out).iter().filter(|e| e["type"] == "tool_use").filter_map(|e| e["part"]["tool"].as_str().map(str::to_string)).collect()
}

/// An event's shape: its kind, its own keys and its part's keys, so two runs compare without their ids, times and text.
pub(crate) fn shape(event: &Value) -> (String, Vec<String>, Vec<String>) {
    let keys = |v: &Value| {
        let mut k: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        k.sort();
        k
    };
    (event["type"].as_str().unwrap_or_default().to_string(), keys(event), keys(&event["part"]))
}

pub(crate) const PONG: &str = "Reply with exactly the word PONG and nothing else.";

#[test]
fn a_plain_prompt_prints_the_same_to_stdout_and_exits_the_same() {
    crate::parity::retrying(|| {
        // Given the same prompt for both
        let (a, b) = (side(&[]), side(&[]));
        // When each answers it
        let (opencode, ask) = Side::both(&a, &b, &[PONG]);
        // Then both say PONG and exit 0, stdout is the text and a newline, and each printed its header on stderr
        both_say("PONG", &opencode, &ask);
        assert_eq!((opencode.stdout.trim(), ask.stdout.trim()), ("PONG", "PONG"));
        assert!(opencode.stdout.ends_with('\n') && ask.stdout.ends_with('\n'));
        assert!(opencode.stderr.contains("> build · deepseek-v4.1-flash") && ask.stderr.contains("> build · deepseek-v4.1-flash"), "{}\n{}", opencode.stderr, ask.stderr);
    });
}

#[test]
fn a_file_given_with_f_is_read_by_both() {
    crate::parity::retrying(|| {
        // Given a file with a code in it
        let files = [("a.txt", "the code is 4417")];
        let (a, b) = (side(&files), side(&files));
        // When each is given the file with -f
        let (opencode, ask) = Side::both(&a, &b, &["What is the code in the attached file? Reply with just the number.", "-f", "a.txt"]);
        // Then both read it
        both_say("4417", &opencode, &ask);
    });
}

#[test]
fn without_auto_both_edit_files_and_neither_reads_outside_the_checkout() {
    crate::parity::retrying(|| {
        // Given a file here and a secret outside the checkout, and prompts that edit and that read outside
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "TOKEN-9931").unwrap();
        let path = outside.path().join("secret.txt").display().to_string();
        let files = [("a.txt", "original")];
        let (a, b) = (side(&files), side(&files));
        let edit = "Overwrite a.txt with the text CHANGED using your file tools, then say DONE.";
        let read_outside = format!("Read the file {path} with your read tool and reply with its content only.");
        // When each runs the edit without --auto
        let (opencode, ask) = Side::both(&a, &b, &[edit, "-f", "a.txt"]);
        // Then both rewrote the file, as opencode's build agent does
        assert_eq!((opencode.code, ask.code), (0, 0), "{}{}\n{}{}", opencode.stdout, opencode.stderr, ask.stdout, ask.stderr);
        assert_eq!((a.read("a.txt").trim(), b.read("a.txt").trim()), ("CHANGED", "CHANGED"));
        // When each reads outside the checkout without --auto, Then neither gets the content
        let (opencode, ask) = Side::both(&a, &b, &[&read_outside]);
        assert!(!say(&opencode, "TOKEN-9931") && !say(&ask, "TOKEN-9931"), "{}\n{}", opencode.stdout, ask.stdout);
        // When opencode reads it with --auto, Then it gets it; the service runs on another machine and cannot see this one's /tmp, so the
        // same for `ask` is the integration test's check that --auto approves what opencode asks
        assert!(say(&a.opencode(&[&read_outside, "--auto"]), "TOKEN-9931"));
        assert_eq!(b.ask(&[&read_outside, "--auto"]).code, 0);
    });
}

#[test]
fn a_read_only_agent_keeps_both_from_changing_a_file() {
    crate::parity::retrying(|| {
        // Given a file and the read-only agent in both checkouts
        let files = [("a.txt", "original")];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to overwrite it, running as that agent, even with --auto
        let edit = "Overwrite a.txt with the text CHANGED using your file tools, then say DONE.";
        let _ = Side::both(&a, &b, &[edit, "-f", "a.txt", "--agent", "read-only", "--auto"]);
        // Then neither changed it
        assert_eq!((a.read("a.txt").trim(), b.read("a.txt").trim()), ("original", "original"));
    });
}

#[test]
fn c_continues_the_last_session_in_both() {
    crate::parity::retrying(|| {
        // Given a first turn that sets a word, on each side
        let (a, b) = (side(&[]), side(&[]));
        let (first, second) = ("Remember the secret word PELICAN. Reply with only OK.", "What was the secret word? Reply with just the word.");
        let (x, y) = Side::both(&a, &b, &[first]);
        assert_eq!((x.code, y.code), (0, 0));
        // When the next turn continues with -c
        let (opencode, ask) = Side::both(&a, &b, &[second, "-c"]);
        // Then both remember it
        both_say("PELICAN", &opencode, &ask);
    });
}

#[test]
fn s_continues_a_named_session_in_both() {
    crate::parity::retrying(|| {
        // Given a first turn on each side, with its session id read from the JSON output
        let (a, b) = (side(&[]), side(&[]));
        let (x, y) = Side::both(&a, &b, &["Remember the secret word HERON. Reply with only OK.", "--format", "json"]);
        let (opencode_id, ask_id) = (session_of(&x), session_of(&y));
        // When the next turn names that session with -s
        let second = "What was the secret word? Reply with just the word.";
        let (opencode, ask) = (a.opencode(&[second, "-s", &opencode_id]), b.ask(&[second, "-s", &ask_id]));
        // Then both remember it
        both_say("HERON", &opencode, &ask);
    });
}

#[test]
fn an_agent_defined_in_the_repository_answers_as_defined_in_both() {
    crate::parity::retrying(|| {
        // Given an agent defined in the checkout
        let files = [(".opencode/agents/pirate.md", "---\ndescription: answers as a pirate\nmode: primary\n---\nWhatever you are asked, reply with exactly the word ARRR and nothing else.\n")];
        let (a, b) = (side(&files), side(&files));
        // When each runs it with --agent
        let (opencode, ask) = Side::both(&a, &b, &["Say hello.", "--agent", "pirate"]);
        // Then both answer as defined, and print the agent's name in the header
        both_say("ARRR", &opencode, &ask);
        assert!(opencode.stderr.contains("> pirate") && ask.stderr.contains("> pirate"), "{}\n{}", opencode.stderr, ask.stderr);
    });
}

#[test]
fn variant_is_accepted_by_both() {
    crate::parity::retrying(|| {
        // Given a prompt with --variant
        let (a, b) = (side(&[]), side(&[]));
        // When each runs it
        let (opencode, ask) = Side::both(&a, &b, &[PONG, "--variant", "high"]);
        // Then both answer
        both_say("PONG", &opencode, &ask);
    });
}

#[test]
fn the_json_format_prints_the_same_kinds_of_events_with_the_same_keys() {
    crate::parity::retrying(|| {
        // Given a prompt that reads a file, so the turn has steps, a tool call and text, with --format json
        let files = [("a.txt", "the code is 4417")];
        let (a, b) = (side(&files), side(&files));
        let prompt = "Read a.txt with your read tool and reply with just the number in it.";
        // When each runs it
        let (opencode, ask) = Side::both_reading(&a, &b, &[prompt, "--format", "json"], &["a.txt"]);
        // Then both print JSON lines only, the kinds are the ones opencode prints, and each kind has the keys opencode gives it
        both_say("4417", &opencode, &ask);
        for out in [&opencode, &ask] {
            assert!(out.stdout.lines().all(|l| serde_json::from_str::<Value>(l).is_ok()), "{}", out.stdout);
        }
        let kinds = |out: &Out| events(out).iter().map(|e| e["type"].as_str().unwrap_or_default().to_string()).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(kinds(&opencode), kinds(&ask));
        for kind in ["step_start", "tool_use", "text", "step_finish"] {
            let first = |out: &Out| events(out).iter().find(|e| e["type"] == kind).map(shape);
            assert!(first(&opencode).is_some(), "opencode printed no {kind}");
            assert_eq!(first(&opencode), first(&ask), "the {kind} event differs");
        }
        assert_eq!(tools(&opencode), tools(&ask));
    });
}

#[test]
fn an_unknown_option_is_refused_by_both() {
    crate::parity::retrying(|| {
        // Given an option that does not exist for either
        let side = side(&[]);
        // When each is given it
        let (opencode, ask) = (side.opencode(&["p", "--no-such-option"]), side.ask(&["p", "--no-such-option"]));
        // Then both exit non-zero and say nothing on stdout
        assert!(opencode.code != 0 && ask.code != 0, "{}\n{}", opencode.stderr, ask.stderr);
        assert_eq!(ask.stdout, "");
    });
}

#[test]
fn a_session_carries_files_and_tool_work_across_three_turns() {
    crate::parity::retrying(|| {
        // Given an empty checkout on each side and three turns that build on each other: write, append, then read back and count
        let (a, b) = (side(&[]), side(&[]));
        let (write, append, count) = (
            "Create the file notes.txt with exactly one line: alpha",
            "Add a second line to notes.txt: beta. Keep the first line.",
            "Read notes.txt with your read tool and reply with the number of lines in it, as a digit only.",
        );
        // When each side runs the first turn, then continues the session twice with -c
        let first = Side::both(&a, &b, &[write]);
        let second = Side::both(&a, &b, &[append, "-c"]);
        let third = Side::both(&a, &b, &[count, "-c"]);
        // Then the files agree after every turn's edit, and the last answer counts both lines on both sides
        for (who, out) in [("opencode", &first.0), ("ask", &first.1), ("opencode", &second.0), ("ask", &second.1)] {
            assert_eq!(out.code, 0, "{who}: {}{}", out.stdout, out.stderr);
        }
        let normalized = |s: &Side| s.read("notes.txt").lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("|");
        assert_eq!((normalized(&a), normalized(&b)), ("alpha|beta".to_string(), "alpha|beta".to_string()));
        both_say("2", &third.0, &third.1);
    });
}

#[test]
fn two_sessions_keep_their_own_state_when_taken_up_by_id_in_turn() {
    crate::parity::retrying(|| {
        // Given two sessions on each side, each told a different word
        let (a, b) = (side(&[]), side(&[]));
        let ask_word = "What was the secret word? Reply with just the word.";
        let mut ids = Vec::new();
        for word in ["MAPLE", "BIRCH"] {
            let (x, y) = Side::both(&a, &b, &[&format!("Remember the secret word {word}. Reply with only OK."), "--format", "json"]);
            ids.push((session_of(&x), session_of(&y)));
        }
        // When they are taken up out of order by id, and then the last one is continued with -c
        let first_again = (a.opencode(&[ask_word, "-s", &ids[0].0]), b.ask(&[ask_word, "-s", &ids[0].1]));
        let second_again = (a.opencode(&[ask_word, "-s", &ids[1].0]), b.ask(&[ask_word, "-s", &ids[1].1]));
        let last = Side::both(&a, &b, &[ask_word, "-c"]);
        // Then each answers with its own word, and -c continues the one used last
        both_say("MAPLE", &first_again.0, &first_again.1);
        both_say("BIRCH", &second_again.0, &second_again.1);
        both_say("BIRCH", &last.0, &last.1);
    });
}

#[test]
fn a_file_changed_between_turns_is_seen_when_it_is_sent_again() {
    crate::parity::retrying(|| {
        // Given a session whose first turn read a file, and the file then changed by the user
        let (a, b) = (side(&[("config.txt", "mode=slow")]), side(&[("config.txt", "mode=slow")]));
        let ask_mode = "What is the value of mode in config.txt? Reply with just the value.";
        both_say("slow", &a.opencode(&[ask_mode, "-f", "config.txt"]), &b.ask(&[ask_mode, "-f", "config.txt"]));
        for s in [&a, &b] {
            std::fs::write(s.dir.path().join("config.txt"), "mode=fast").unwrap();
        }
        // When the next turn sends the file again with -f, continuing the session
        let (opencode, ask) = (a.opencode(&[ask_mode, "-c", "-f", "config.txt"]), b.ask(&[ask_mode, "-c", "-f", "config.txt"]));
        // Then both see the new value
        both_say("fast", &opencode, &ask);
    });
}

#[test]
fn a_task_that_searches_runs_a_script_and_edits_calls_the_same_tools_in_both() {
    crate::parity::retrying(|| {
        // Given a checkout with a few files, one holding a marker
        let files = [("src/one.txt", "alpha"), ("src/two.txt", "the marker is FOXGLOVE"), ("src/three.txt", "gamma")];
        let (a, b) = (side(&files), side(&files));
        let prompt = "Use your grep tool to find which file under src contains the word FOXGLOVE, then write that file's name to found.txt, then run `cat found.txt` with your bash tool, then reply with the file name only.";
        // When each runs it with --auto and --format json
        let (opencode, ask) = Side::both_reading(&a, &b, &[prompt, "--auto", "--format", "json"], &["src/one.txt", "src/two.txt", "src/three.txt"]);
        // Then both exit 0, wrote found.txt, called grep, write and bash, and printed the name in a text event
        assert_eq!((opencode.code, ask.code), (0, 0), "{}\n{}", opencode.stdout, ask.stdout);
        for (who, out, s) in [("opencode", &opencode, &a), ("ask", &ask, &b)] {
            let used = tools(out);
            assert!(["grep", "write", "bash"].iter().all(|t| used.iter().any(|u| u == t)), "{who} called {used:?}");
            assert!(s.read("found.txt").contains("two.txt"), "{who}: found.txt is {:?}", s.read("found.txt"));
            assert!(events(out).iter().any(|e| e["type"] == "text" && e["part"]["text"].as_str().is_some_and(|t| t.contains("two.txt"))), "{who}: {}", out.stdout);
        }
    });
}

/// A live-model test body, tried up to three times: the model's wording varies from run to run, so a miss is retried and only three in a
/// row fail the test. Every failed attempt is printed with its number.
pub(crate) fn retrying(body: impl Fn() + std::panic::RefUnwindSafe) {
    run_attempts(3, body);
}

fn run_attempts(attempts: usize, body: impl Fn() + std::panic::RefUnwindSafe) {
    let mut misses = Vec::new();
    for attempt in 1..=attempts {
        match std::panic::catch_unwind(&body) {
            Ok(()) => return,
            Err(why) => {
                let text = why.downcast_ref::<String>().cloned().or_else(|| why.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                eprintln!("attempt {attempt}/{attempts} failed: {text}");
                misses.push(format!("attempt {attempt}/{attempts}: {text}"));
            }
        }
    }
    panic!("{}", misses.join("\n"));
}

#[cfg(test)]
mod retry_tests {
    use super::run_attempts;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn given_a_body_that_misses_twice_when_retried_then_the_third_attempt_passes_the_test() {
        // Given a body that panics on its first two calls
        let calls = AtomicUsize::new(0);
        // When it runs under the retry, Then the test passes on the third attempt
        run_attempts(3, || assert!(calls.fetch_add(1, Ordering::SeqCst) >= 2, "model said something else"));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn given_a_body_that_always_misses_when_retried_then_the_test_fails_with_every_attempt_named() {
        // Given a body that always panics
        // When it runs under the retry
        let failed = std::panic::catch_unwind(|| run_attempts(3, || panic!("no ARRR")));
        // Then it fails after three attempts and says what each one saw
        let text = failed.unwrap_err().downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(text.contains("attempt 1/3: no ARRR") && text.contains("attempt 3/3: no ARRR"), "{text}");
    }
}
