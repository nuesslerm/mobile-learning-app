//! The device commands: the one table the `wf browser|android <verb>` CLI, the Lua `device` and the help and `test --api`
//! text are made from (docs/plans/harness-api/contract.md). Lua names are snake_case, CLI verbs the same words in kebab-case.
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Str,
    Int,
    Num,
    Bool,
    Sel,
    Point,
    Path,
    /// A Lua function in a script; a Lua expression on the CLI.
    Lua,
    /// Any JSON value (a Lua table).
    Json,
    /// `{{left, top, right, bottom}, ...}`; `--ignore L,T,R,B`, repeatable, on the CLI.
    Rects,
    Enum(&'static [&'static str]),
}

impl Kind {
    pub fn name(self) -> String {
        match self {
            Kind::Str => "string".into(),
            Kind::Int => "int".into(),
            Kind::Num => "number".into(),
            Kind::Bool => "bool".into(),
            Kind::Sel => "selector".into(),
            Kind::Point => "point".into(),
            Kind::Path => "path".into(),
            Kind::Lua => "function".into(),
            Kind::Json => "json".into(),
            Kind::Rects => "rects".into(),
            Kind::Enum(v) => v.join("|"),
        }
    }

    fn metavar(self) -> String {
        match self {
            Kind::Int => "MS".into(),
            Kind::Num => "N".into(),
            Kind::Point => "X,Y".into(),
            Kind::Path => "PATH".into(),
            Kind::Lua => "EXPR".into(),
            Kind::Json => "JSON".into(),
            Kind::Rects => "L,T,R,B".into(),
            Kind::Enum(v) => v.join("|"),
            _ => "TEXT".into(),
        }
    }
}

pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    /// The default as JSON text, when there is a fixed one.
    pub default: Option<&'static str>,
    pub positional: bool,
    pub required: bool,
    pub doc: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Platforms {
    Both,
    /// Android only, and why.
    Android(&'static str),
    /// Browser only, and why.
    Browser(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Surface {
    Both,
    LuaOnly,
    CliOnly,
}

pub struct Command {
    pub name: &'static str,
    pub maestro: Option<&'static str>,
    pub params: &'static [Param],
    pub platforms: Platforms,
    /// The Android lease guard runs before it.
    pub guarded: bool,
    /// It changes the screen or the device, so a condition may not call it.
    pub action: bool,
    pub surface: Surface,
    pub result: &'static str,
    pub doc: &'static str,
}

impl Command {
    pub fn verb(&self) -> String {
        self.name.replace('_', "-")
    }

    pub fn param(&self, name: &str) -> Option<&'static Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// The first sentence of the doc.
    pub fn summary(&self) -> &'static str {
        self.doc.split(". ").next().unwrap_or(self.doc).trim_end_matches('.')
    }

    /// Why the command is not on `platform`, when it is not.
    pub fn absent_on(&self, platform: &str) -> Option<&'static str> {
        match self.platforms {
            Platforms::Android(why) if platform != "android" => Some(why),
            Platforms::Browser(why) if platform != "browser" => Some(why),
            _ => None,
        }
    }

    pub fn is_wait(&self) -> bool {
        matches!(self.name, "extended_wait_until" | "assert_visible" | "assert_not_visible")
    }

    /// `device:tap_on(sel [, {optional, label}])`; the options table is bracketed only when none of it is required.
    pub fn lua_signature(&self) -> String {
        let positional: Vec<String> = self.params.iter().filter(|p| p.positional).map(|p| if p.required { p.name.to_string() } else { format!("[{}]", p.name) }).collect();
        let options: Vec<&str> = self.params.iter().filter(|p| !p.positional).map(|p| p.name).collect();
        if positional.is_empty() && self.params.iter().any(|p| p.required || p.name == "visible") {
            return format!("device:{}{{{}}}", self.name, options.join(", "));
        }
        let table = format!("{{{}}}", options.join(", "));
        let needed = self.params.iter().any(|p| !p.positional && p.required);
        let args = match (positional.is_empty(), options.is_empty(), needed) {
            (_, true, _) => positional.join(", "),
            (true, false, _) => format!("[{table}]"),
            (false, false, true) => format!("{}, {table}", positional.join(", ")),
            (false, false, false) => format!("{} [, {table}]", positional.join(", ")),
        };
        format!("device:{}({args})", self.name)
    }
}

const fn opt(name: &'static str, kind: Kind, default: Option<&'static str>, doc: &'static str) -> Param {
    Param { name, kind, default, positional: false, required: false, doc }
}

const fn pos(name: &'static str, kind: Kind, doc: &'static str) -> Param {
    Param { name, kind, default: None, positional: true, required: true, doc }
}

const OPTIONAL: Param = opt("optional", Kind::Bool, Some("false"), "a miss returns null instead of failing, after a short lookup (WF_OPTIONAL_LOOKUP_MS, 1000), with no evidence");
const LABEL: Param = opt("label", Kind::Str, None, "names the step in report.json, timing.jsonl and the error");
const SEL: Param = pos("sel", Kind::Sel, "{id=RE, text=RE, css=S} or a string meaning {text=...}");
const TIMEOUT: Param = opt("timeout", Kind::Int, None, "ms; default WF_WAIT_BUDGET_MS (10000), or WF_OPTIONAL_LOOKUP_MS (1000) with optional; 0 looks once");

const SELECTORS: &str = "A selector is {id=RE, text=RE, css=S, point='x,y'|'x%,y%'} (every given field must hold) or a bare string meaning {text=...}. id matches the test id (data-testid; Android's resource-id, whole or after its last '/'); text the element's text, its accessibility label or an input's value; css is browser only; point is tap_on's only. A pattern matches Maestro's way: case-insensitive, a full match or literal equality ('Run (local)' finds that text), a newline in the value read as a space; it keeps to the grammar Java, JavaScript and Rust share (literals, ., [...], \\d \\w \\s, escapes of ^$\\.*+?()[]{}|/, quantifiers, (...) (?:...), |, ^ $). The old 'id:X' strings are refused with the table form to use.";

pub static COMMANDS: &[Command] = &[
    Command {
        name: "open_link",
        maestro: Some("openLink"),
        params: &[pos("link", Kind::Str, "a URL or deep link"), OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{link}",
        doc: "Opens a URL or deep link in the app. It returns once the platform took the link, not once the screen rendered, so follow it with a wait. Browser: the test's first call opens its session; Android: a VIEW intent to Expo Go.",
    },
    Command {
        name: "tap_on",
        maestro: Some("tapOn"),
        params: &[Param { doc: "{id=RE, text=RE, css=S, point='x,y'|'x%,y%'} or a string meaning {text=...}", ..SEL }, OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{x, y, bounds}",
        doc: "Waits until the element is visible (up to WF_WAIT_BUDGET_MS, or WF_OPTIONAL_LOOKUP_MS with optional), waits until its box holds still across two looks 100 ms apart (Maestro's settle, at most WF_SETTLE_MS, 1000), then taps inside its on-screen part. A point is tapped at once. Tapping a field is how the keyboard comes up. A miss fails with the evidence of a timed-out wait.",
    },
    Command {
        name: "swipe",
        maestro: Some("swipe"),
        params: &[pos("start", Kind::Point, "'x,y' in tap units or 'x%,y%'"), pos("end", Kind::Point, "'x,y' in tap units or 'x%,y%'"), opt("duration", Kind::Int, None, "ms the finger takes; a short one flings; unset, 400 paced by the run's WF_TEST_SPEED"), OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{start, end, duration}",
        doc: "Moves one finger from start to end over duration ms. In Lua the points are positional (end is a keyword): device:swipe('200,600', '200,200', {duration=600}).",
    },
    Command {
        name: "scroll_until_visible",
        maestro: Some("scrollUntilVisible"),
        params: &[
            Param { name: "element", kind: Kind::Sel, default: None, positional: false, required: true, doc: "the element to bring into view" },
            opt("direction", Kind::Enum(&["DOWN", "UP", "LEFT", "RIGHT"]), Some("\"DOWN\""), "where the content lies: DOWN brings content from below, so the finger moves up"),
            opt("from", Kind::Sel, None, "the element whose box the swipes run in (default: the screen)"),
            opt("timeout", Kind::Int, Some("20000"), "ms"),
            opt("visibility_percentage", Kind::Int, Some("100"), "how much of the element must be on screen"),
            OPTIONAL,
            LABEL,
        ],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{bounds, text, swipes}",
        doc: "Swipes until visibility_percentage of the element is on screen: 600 ms drags (paced by WF_TEST_SPEED), UP and DOWN from the centre of from to 10% inside its edge opposite direction, LEFT and RIGHT across it from 10% inside one edge to 10% inside the other.",
    },
    Command {
        name: "input_text",
        maestro: Some("inputText"),
        params: &[pos("text", Kind::Str, "the text, typed key by key"), OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{typed, ms}",
        doc: "Types the text at the caret of the focused field, one key at a time with human pacing. It refuses while the keyboard is down: tap the field first.",
    },
    Command {
        name: "press_key",
        maestro: Some("pressKey"),
        params: &[pos("key", Kind::Enum(&["Enter", "Backspace", "Tab", "Remote Dpad Left", "Remote Dpad Right", "Remote Dpad Up", "Remote Dpad Down", "Ctrl+End"]), "case-insensitive"), OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{key}",
        doc: "Presses one named key: Enter, Backspace, Tab, the arrows by Maestro's names (Remote Dpad Left, Right, Up, Down) and Ctrl+End. Letters go through input_text. It refuses while the keyboard is down: tap the field first.",
    },
    Command {
        name: "back",
        maestro: Some("back"),
        params: &[OPTIONAL, LABEL],
        platforms: Platforms::Android("a page has no system Back key, and history back has no caller"),
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{}",
        doc: "Presses the system Back key (scripts close Expo Go's developer menu with it). It refuses while the keyboard is up: tap Done.",
    },
    Command {
        name: "extended_wait_until",
        maestro: Some("extendedWaitUntil"),
        params: &[
            opt("visible", Kind::Sel, None, "wait until this element is visible"),
            opt("not_visible", Kind::Sel, None, "wait until no visible element matches"),
            opt("condition", Kind::Lua, None, "a read-only function (CLI: a Lua expression) called until it returns a truthy value"),
            TIMEOUT,
            opt("interval", Kind::Int, Some("200"), "most ms between condition checks, at least 50 (the browser checks sooner when the page changes, never within 50 ms); condition only"),
            OPTIONAL,
            LABEL,
        ],
        platforms: Platforms::Both,
        guarded: false,
        action: false,
        surface: Surface::Both,
        result: "visible: the element {bounds, text}; not_visible: true; condition: its first truthy value; an optional miss: null (CLI: {met, value, ms, checks})",
        doc: "Waits for exactly one of visible, not_visible or condition, as Maestro's extendedWaitUntil and Cypress's waitUntil. A condition may only read (get_*); an action inside it raises, and an error inside it fails the wait at once. A timeout keeps looking WF_WAIT_DIAGNOSE_MS (8000) more to say slow or broken, then fails with evidence: a screenshot, the hierarchy, the element's state, the keyboard and the last warnings.",
    },
    Command {
        name: "assert_visible",
        maestro: Some("assertVisible"),
        params: &[SEL, TIMEOUT, OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: false,
        action: false,
        surface: Surface::Both,
        result: "the element {bounds, text}; an optional miss: null (CLI: {met, value, ms, checks})",
        doc: "extended_wait_until{visible=sel}: the same wait, defaults and evidence, under its own span name.",
    },
    Command {
        name: "assert_not_visible",
        maestro: Some("assertNotVisible"),
        params: &[SEL, TIMEOUT, OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: false,
        action: false,
        surface: Surface::Both,
        result: "true; an optional miss: null (CLI: {met, value, ms, checks})",
        doc: "extended_wait_until{not_visible=sel}: met at the first look where no visible element matches.",
    },
    Command {
        name: "take_screenshot",
        maestro: Some("takeScreenshot"),
        params: &[pos("path", Kind::Path, "where the PNG goes ('.png' is added when missing)"), LABEL],
        platforms: Platforms::Both,
        guarded: false,
        action: true,
        surface: Surface::Both,
        result: "{path, width, height}",
        doc: "Saves the screen as a PNG at path and lists it in the report's screenshots.",
    },
    Command {
        name: "assert_screenshot",
        maestro: Some("assertScreenshot"),
        params: &[
            pos("path", Kind::Str, "the golden's name under docs/runbooks/goldens/<platform>/ ('.png' optional)"),
            opt("threshold_percentage", Kind::Num, Some("95"), "the share of pixels that must match"),
            opt("ignore", Kind::Rects, None, "rectangles in tap units left out of the comparison"),
            OPTIONAL,
            LABEL,
        ],
        platforms: Platforms::Both,
        guarded: false,
        action: true,
        surface: Surface::Both,
        result: "{path, verdict, match_percentage}",
        doc: "Compares the screen, scaled to 360 px, with its golden on every run, and fails below threshold_percentage or when no golden exists, with the diff as evidence. Under test --update-goldens it writes the golden instead.",
    },
    Command {
        name: "start_recording",
        maestro: Some("startRecording"),
        params: &[pos("path", Kind::Path, "where the video goes ('.mp4' is added when missing)")],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::CliOnly,
        result: "{path}",
        doc: "Starts a screen recording. CLI only: a test records itself from its first open_link to its end.",
    },
    Command {
        name: "stop_recording",
        maestro: Some("stopRecording"),
        params: &[],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::CliOnly,
        result: "{files}",
        doc: "Ends the recording and returns its files. CLI only, as start_recording.",
    },
    Command {
        name: "get_element",
        maestro: None,
        params: &[SEL],
        platforms: Platforms::Both,
        guarded: false,
        action: false,
        surface: Surface::Both,
        result: "{bounds: {left, top, right, bottom, cx, cy}, text} or null",
        doc: "One look, no wait (Cypress's cy.get): the element when it is there, null when it is not. text is a field's value (the code editor's buffer for {id='CodeEditorInput'}), else the visible text, else the accessibility label.",
    },
    Command {
        name: "get_device_state",
        maestro: None,
        params: &[Param { name: "part", kind: Kind::Enum(&["screen", "keyboard", "microphone"]), default: None, positional: true, required: false, doc: "only this part, returned as itself" }],
        platforms: Platforms::Both,
        guarded: false,
        action: false,
        surface: Surface::Both,
        result: "{screen: {width, height, dpr}, keyboard: {visible, bounds}, microphone: {input, consumed}}, or the one part asked for",
        doc: "What the device holds right now; a part reads only that part. screen is in tap units with dpr the screenshot pixels per unit; keyboard.bounds is null while it is down; microphone.consumed is whether a recorder took the clip set_microphone_input set.",
    },
    Command {
        name: "get_pixel",
        maestro: None,
        params: &[pos("point", Kind::Point, "'x,y' in tap units or 'x%,y%'")],
        platforms: Platforms::Both,
        guarded: false,
        action: false,
        surface: Surface::Both,
        result: "{r, g, b}",
        doc: "The colour of one screen point from a fresh screenshot: it sees what the accessibility tree cannot (Android's Reset dialog, the result toast).",
    },
    Command {
        name: "get_logs",
        maestro: None,
        params: &[opt("level", Kind::Enum(&["debug", "info", "warn", "error"]), Some("\"warn\""), "the lowest level returned")],
        platforms: Platforms::Both,
        guarded: false,
        action: false,
        surface: Surface::Both,
        result: "[\"<level>: <message> (at <frames>)\", ...]",
        doc: "The app's console lines since the session opened or the last clear_logs, at level or above.",
    },
    Command {
        name: "clear_logs",
        maestro: None,
        params: &[OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{cleared}",
        doc: "Discards the buffered log lines without reporting them (known noise, fresh timing marks).",
    },
    Command {
        name: "set_microphone_input",
        maestro: None,
        params: &[pos("path", Kind::Path, "an audio clip"), OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{path, bytes}",
        doc: "Sets the clip the app's microphone hears at its next recording (one pending clip; a new call replaces it). Wait for it with extended_wait_until{condition=function() return device:get_device_state('microphone').consumed end}.",
    },
    Command {
        name: "draw_strokes_on",
        maestro: None,
        params: &[SEL, Param { name: "source", kind: Kind::Enum(&["guide", "template"]), default: None, positional: false, required: true, doc: "which strokes the pad shows" }, opt("human", Kind::Bool, Some("true"), "human timing and path jitter; false draws each stroke straight and evenly"), OPTIONAL, LABEL],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{strokes, points}",
        doc: "Waits for the drawing pad like tap_on, reads its guide or template strokes and draws each by touch.",
    },
    Command {
        name: "run_shell",
        maestro: None,
        params: &[pos("command", Kind::Str, "a shell command line"), OPTIONAL, LABEL],
        platforms: Platforms::Android("a page has no OS shell"),
        guarded: true,
        action: true,
        surface: Surface::Both,
        result: "{stdout, exit_code} (Lua: stdout, exit_code)",
        doc: "Runs a shell command on the device; a non-zero exit is an answer, not a failure.",
    },
    Command {
        name: "add_network_stub",
        maestro: None,
        params: &[
            pos("method", Kind::Str, "GET, POST, ... or '*'"),
            pos("url_pattern", Kind::Str, "a URL glob ('*' any run of characters)"),
            opt("status", Kind::Int, Some("200"), "the answer's status"),
            opt("body", Kind::Str, None, "a text body"),
            opt("json_body", Kind::Json, None, "a table sent as JSON"),
            opt("body_file", Kind::Path, None, "a file sent as the body"),
            opt("headers", Kind::Json, None, "{K=V}"),
            opt("delay", Kind::Int, Some("0"), "ms before the answer"),
        ],
        platforms: Platforms::Both,
        guarded: true,
        action: true,
        surface: Surface::LuaOnly,
        result: "{pattern, method, status, bytes}",
        doc: "Answers matching requests with the canned response for the rest of the run; unmatched requests still reach the network. Lua only: stubs live for the run.",
    },
    Command {
        name: "get_network_stub_hits",
        maestro: None,
        params: &[],
        platforms: Platforms::Both,
        guarded: false,
        action: false,
        surface: Surface::LuaOnly,
        result: "[{method, url, status, pattern, at}, ...]",
        doc: "The stubbed requests answered so far. Lua only, as add_network_stub.",
    },
    Command {
        name: "get_debug_hits",
        maestro: None,
        params: &[],
        platforms: Platforms::Browser("a logpoint is armed in a browser page; an Android run loads the EAS Update, which has no inspector"),
        guarded: false,
        action: false,
        surface: Surface::LuaOnly,
        result: "[{file, line, function, frames, locals}, ...]",
        doc: "The logpoint hits so far (points are armed with test --logpoint file:line[:cond]), with frames at original lines and the locals at the stop. Lua only: hits live for the run.",
    },
];

/// The Lua globals beside `device`.
const GLOBALS: &[(&str, &str)] = &[
    ("run_flow(label, fn)", "runs fn as a named step: its own row in the report's timing (Maestro's runFlow)"),
    ("assert_true(value, label)", "fails the test at this line with label when value is nil or false (Maestro's assertTrue)"),
    ("log(...)", "prints a line"),
    ("env", "the --env K=V values, as strings (Maestro's env)"),
    ("json.encode(v)", "a value as JSON text"),
    ("json.array()", "an empty table that json.encode writes as [] rather than {}"),
    ("now_ms()", "epoch milliseconds"),
    ("sleep_ms(ms)", "a fixed pause, for real-time media only; wait with extended_wait_until instead"),
    ("add_timing_span(label, from_ms, to_ms)", "a step measured from marks elsewhere"),
    ("agent.run{prompt=, model=, timeout=, json=}", "a local agent on the same device for an open-ended step: {ok, text, exit, timed_out, ms, stderr, result}"),
    ("pcall(f, ...) / xpcall(f, h, ...)", "as Lua's, except that a caught error still honours a run stop"),
    ("device.platform", "'android' or 'browser' (Maestro's MAESTRO_PLATFORM)"),
    ("io, os", "Lua's standard libraries (wav files, result.json)"),
];

pub fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// The command a CLI verb names (`tap-on`), when it is one a CLI takes.
pub fn by_verb(verb: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.surface != Surface::LuaOnly && c.verb() == verb)
}

fn as_int(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
}

/// Checks `args` against the command's params and fills the defaults: an unknown key, a wrong kind or a missing required
/// param is a usage error.
pub fn check(cmd: &Command, args: &serde_json::Map<String, Value>) -> Result<serde_json::Map<String, Value>, String> {
    let mut out = serde_json::Map::new();
    for (k, v) in args {
        let p = cmd.param(k).ok_or_else(|| format!("{}: unknown parameter '{k}' (takes: {})", cmd.name, cmd.params.iter().map(|p| p.name).collect::<Vec<_>>().join(", ")))?;
        if v.is_null() {
            continue;
        }
        let bad = || format!("{}: {k} must be {}, got {v}", cmd.name, p.kind.name());
        let v = match p.kind {
            Kind::Int => json!(as_int(v).filter(|n| *n >= 0).ok_or_else(bad)?),
            Kind::Num => json!(v.as_f64().ok_or_else(bad)?),
            Kind::Bool => json!(v.as_bool().ok_or_else(bad)?),
            Kind::Str | Kind::Path | Kind::Point | Kind::Lua => json!(v.as_str().ok_or_else(bad)?),
            Kind::Enum(values) => {
                let s = v.as_str().ok_or_else(bad)?;
                json!(values.iter().find(|x| x.eq_ignore_ascii_case(s)).ok_or_else(|| format!("{}: {k} must be one of {}, got '{s}'", cmd.name, values.join(", ")))?)
            }
            Kind::Sel => {
                if !(v.is_string() || v.is_object()) {
                    return Err(bad());
                }
                v.clone()
            }
            Kind::Rects => {
                let ok = v.as_array().is_some_and(|a| a.iter().all(|r| r.as_array().is_some_and(|r| r.len() == 4 && r.iter().all(|n| as_int(n).is_some()))));
                if !ok {
                    return Err(format!("{}: {k} is a list of {{left, top, right, bottom}}, got {v}", cmd.name));
                }
                v.clone()
            }
            Kind::Json => v.clone(),
        };
        out.insert(k.clone(), v);
    }
    for p in cmd.params {
        if !out.contains_key(p.name) {
            if p.required {
                return Err(format!("{}: {} is required", cmd.name, p.name));
            }
            if let Some(d) = p.default {
                out.insert(p.name.into(), serde_json::from_str(d).unwrap_or(Value::Null));
            }
        }
    }
    Ok(out)
}

/// `wf <noun> <verb>`'s usage line.
pub fn usage(platform: &str, cmd: &Command) -> String {
    let mut parts = vec![format!("wf {platform} {}", cmd.verb())];
    for p in cmd.params {
        let flag = p.name.replace('_', "-");
        let s = match p.kind {
            Kind::Sel if p.name == "sel" => {
                let point = if cmd.name == "tap_on" { " | --point X,Y" } else { "" };
                format!("<text> | --id RE | --text RE | --css S{point}")
            }
            Kind::Sel if matches!(p.name, "visible" | "not_visible") => format!("--{flag} <selector flags>"),
            Kind::Sel => format!("--{flag}-id RE | --{flag}-text RE | --{flag}-css S"),
            Kind::Bool => format!("--{flag}"),
            Kind::Lua => "--condition '<Lua expression>'".to_string(),
            k if p.positional => format!("<{}>", if matches!(k, Kind::Enum(_)) { k.metavar() } else { p.name.to_string() }),
            k => format!("--{flag} {}", k.metavar()),
        };
        parts.push(if p.required || (p.positional && p.kind == Kind::Sel) { s } else { format!("[{s}]") });
    }
    if cmd.name == "extended_wait_until" {
        parts.retain(|p| !p.starts_with("[--visible") && !p.starts_with("[--not-visible") && !p.starts_with("[--condition"));
        parts.insert(1, "(--visible | --not-visible) <text> | --id RE | --text RE | --css S | --condition '<Lua expression>'".into());
    }
    parts.push("[--device D]".into());
    parts.join(" ")
}

fn platforms_line(cmd: &Command) -> String {
    match cmd.platforms {
        Platforms::Both => "browser and android".into(),
        Platforms::Android(why) => format!("android only; wf browser exits 2: {why}"),
        Platforms::Browser(why) => format!("browser only; wf android exits 2: {why}"),
    }
}

/// `wf <noun> <verb> --help`.
pub fn verb_help(platform: &str, cmd: &Command) -> String {
    let mut s = format!("Usage: {}\n\n{}\n\n", usage(platform, cmd), cmd.doc);
    s.push_str(&format!("Maestro: {}. Lua: {}. Platforms: {}.", cmd.maestro.unwrap_or("none (ours)"), if cmd.surface == Surface::CliOnly { "none (CLI only)".to_string() } else { cmd.lua_signature() }, platforms_line(cmd)));
    if cmd.guarded {
        s.push_str(" Android: lease-guarded.");
    }
    if cmd.action {
        s.push_str(" An action: a condition may not call it.");
    }
    s.push_str("\n\nOptions:\n");
    for p in cmd.params {
        let default = p.default.map(|d| format!(" (default {})", d.trim_matches('"'))).unwrap_or_default();
        let name = match p.kind {
            Kind::Sel if p.name == "sel" => "<text> --id --text --css".to_string() + if cmd.name == "tap_on" { " --point" } else { "" },
            Kind::Sel if matches!(p.name, "visible" | "not_visible") => format!("--{}", p.name.replace('_', "-")),
            Kind::Sel => format!("--{0}-id --{0}-text --{0}-css", p.name.replace('_', "-")),
            Kind::Lua => "--condition".into(),
            _ if p.positional => format!("<{}>", p.name),
            _ => format!("--{}", p.name.replace('_', "-")),
        };
        s.push_str(&format!("  {name:<30} {}{default}\n", p.doc));
    }
    let device = if platform == "android" { "the adb serial (default: --device, ANDROID_SERIAL, WF_ANDROID_SERIAL, else the only device)" } else { "the browser session (default: the only one)" };
    s.push_str(&format!("  {:<30} {device}\n  {:<30} accepted and ignored: the output is always one JSON line\n", "--device D", "--json"));
    s.push_str(&format!("\nResult: {}\n\nOutput: one JSON line. Success {{ok:true, command, platform, device, ms, result}}, exit 0; a failure {{ok:false, command, platform, device, ms, error, evidence, verdict, met_after_ms, next}}, exit 1; a usage error {{ok:false, error, usage}}, exit 2; an Android lease conflict, exit 3.\n\n{SELECTORS}\n", cmd.result));
    s
}

/// The device verbs, appended to `wf <noun> --help`.
pub fn noun_section(platform: &str) -> String {
    let mut s = format!("\nDevice commands (Maestro's names; the same verbs and JSON answers on wf browser and wf android; `wf {platform} <verb> --help` for one, `wf {platform} test --api` for all):\n");
    for c in COMMANDS.iter().filter(|c| c.surface != Surface::LuaOnly) {
        let only = c.absent_on(platform).map(|_| " (not on this platform: exits 2)").unwrap_or_default();
        s.push_str(&format!("  {:<22} {}{only}\n", c.verb(), c.summary()));
    }
    s
}

const SCRIPT: &str = include_str!("../help/wf__run__api.txt");

/// `wf <noun> test --api`: the script API, generated from the table.
pub fn text(platform: &str) -> String {
    let (logpoint, extra) = if platform == "browser" { ("[--logpoint file:line[:cond]]... ", " [--screen-size P] [--frame screen|bezel|document]") } else { ("", " [--cold-start]") };
    let mut s = format!(
        "wf {platform} test <script.lua> [--env K=V]... [--publish] [--test-output-dir D] [--no-record] [--update-goldens] [--goldens-dir D]\n\
         \x20   {logpoint}[--device D]{extra} [--speed N]\n\
         wf {platform} test --api [--json]\n\n\
         A script is plain Lua 5.4. It runs top to bottom; any failed call stops it with the script line.\n\n\
         device (Maestro's commands in snake_case; each is also `wf browser|android <kebab-case verb>`, which prints one JSON line):\n"
    );
    for c in COMMANDS.iter().filter(|c| c.surface != Surface::CliOnly) {
        let only = match c.platforms {
            Platforms::Both => String::new(),
            Platforms::Android(why) => format!(" Android only: {why}."),
            Platforms::Browser(why) => format!(" Browser only: {why}."),
        };
        s.push_str(&format!("  {}\n      -> {}. {}{only}\n", c.lua_signature(), c.result, c.doc));
        // what each option is and means is part of the call: without it a cheap model read `direction` as the finger's
        // way, and passed `from` a bounds string
        for p in c.params.iter().filter(|p| !matches!(p.name, "optional" | "label")) {
            let default = p.default.map(|d| format!(" (default {d})")).unwrap_or_default();
            s.push_str(&format!("      {} ({}): {}{default}\n", p.name, p.kind.name(), p.doc));
        }
    }
    s.push_str("  CLI only: ");
    s.push_str(&COMMANDS.iter().filter(|c| c.surface == Surface::CliOnly).map(|c| format!("{} ({})", c.verb(), c.summary())).collect::<Vec<_>>().join("; "));
    s.push_str(&format!(".\n  Every action and wait takes optional (a miss returns nil after WF_OPTIONAL_LOOKUP_MS, with no evidence) and label. Durations are ms.\n\n{SELECTORS}\n\nGlobals\n"));
    for (g, doc) in GLOBALS {
        s.push_str(&format!("  {g:<44} {doc}\n"));
    }
    s.push('\n');
    s.push_str(SCRIPT);
    s
}

/// `test --api --json`: the table itself.
pub fn json(platform: &str) -> Value {
    let commands: Vec<Value> = COMMANDS
        .iter()
        .map(|c| {
            let (platforms, why) = match c.platforms {
                Platforms::Both => ("both", None),
                Platforms::Android(w) => ("android", Some(w)),
                Platforms::Browser(w) => ("browser", Some(w)),
            };
            json!({
                "name": c.name,
                "verb": (c.surface != Surface::LuaOnly).then(|| c.verb()),
                "lua": (c.surface != Surface::CliOnly).then(|| c.lua_signature()),
                "maestro": c.maestro,
                "params": c.params.iter().map(|p| json!({ "name": p.name, "kind": p.kind.name(), "default": p.default.and_then(|d| serde_json::from_str::<Value>(d).ok()), "positional": p.positional, "required": p.required, "doc": p.doc })).collect::<Vec<_>>(),
                "platforms": platforms,
                "why": why,
                "guarded": c.guarded,
                "action": c.action,
                "surface": match c.surface { Surface::Both => "both", Surface::LuaOnly => "lua", Surface::CliOnly => "cli" },
                "result": c.result,
                "doc": c.doc,
            })
        })
        .collect();
    json!({
        "schema": 1,
        "platform": platform,
        "commands": commands,
        "globals": GLOBALS.iter().map(|(g, d)| json!({ "name": g, "doc": d })).collect::<Vec<_>>(),
        "selectors": SELECTORS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_table_when_read_then_every_command_is_unique_and_documented_and_the_two_clis_list_the_same_verbs() {
        let mut names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len());
        assert_eq!(COMMANDS.iter().filter(|c| c.surface != Surface::LuaOnly).count(), 22, "22 device verbs on each CLI");
        assert_eq!(COMMANDS.iter().filter(|c| c.surface != Surface::CliOnly).count(), 23, "23 device methods in Lua");
        for c in COMMANDS {
            assert!(!c.doc.is_empty() && !c.result.is_empty() && c.params.iter().all(|p| !p.doc.is_empty()), "{}", c.name);
            let first_option = c.params.iter().position(|q| !q.positional).unwrap_or(usize::MAX);
            assert!(c.params.iter().enumerate().all(|(i, p)| !p.positional || i < first_option), "{}: positionals come first", c.name);
            for p in c.params {
                if let Some(d) = p.default {
                    assert!(serde_json::from_str::<Value>(d).is_ok(), "{}.{}", c.name, p.name);
                }
            }
        }
        assert_eq!(by_verb("tap-on").map(|c| c.name), Some("tap_on"));
        assert!(by_verb("add-network-stub").is_none() && by_verb("tap").is_none());
        assert_eq!((find("back").unwrap().absent_on("browser").is_some(), find("back").unwrap().absent_on("android")), (true, None));
    }

    #[test]
    fn given_arguments_when_checked_then_defaults_fill_in_and_unknown_or_wrong_ones_are_usage_errors() {
        let args = |v: Value| v.as_object().unwrap().clone();
        let swipe = check(find("swipe").unwrap(), &args(json!({ "start": "1,2", "end": "3,4" }))).unwrap();
        // a swipe's duration stays unset rather than 400, so the run's speed paces only the default and never a passed 400
        assert_eq!((swipe.get("duration"), swipe["optional"].clone()), (None, json!(false)));
        assert!(check(find("swipe").unwrap(), &args(json!({ "start": "1,2" }))).unwrap_err().contains("end is required"));
        assert!(check(find("tap_on").unwrap(), &args(json!({ "sel": "Go", "timeout": 5 }))).unwrap_err().contains("unknown parameter 'timeout'"));
        assert!(check(find("swipe").unwrap(), &args(json!({ "start": "1,2", "end": "3,4", "duration": "fast" }))).unwrap_err().contains("duration must be int"));
        assert_eq!(check(find("press_key").unwrap(), &args(json!({ "key": "enter" }))).unwrap()["key"], "Enter");
        assert_eq!(check(find("extended_wait_until").unwrap(), &args(json!({ "visible": "Go", "timeout": 1500.0 }))).unwrap()["timeout"], 1500);
        assert!(check(find("assert_screenshot").unwrap(), &args(json!({ "path": "x", "ignore": [[1, 2, 3]] }))).is_err());
    }

    #[test]
    fn given_the_table_when_the_help_and_api_are_generated_then_each_verb_and_method_is_in_them() {
        let api = text("android");
        let help = noun_section("browser");
        for c in COMMANDS {
            if c.surface != Surface::CliOnly {
                assert!(api.contains(&format!("device:{}", c.name)), "{} missing from the api", c.name);
            }
            if c.surface != Surface::LuaOnly {
                assert!(help.contains(&format!("  {} ", c.verb())), "{} missing from the noun help", c.verb());
                assert!(verb_help("android", c).starts_with(&format!("Usage: wf android {}", c.verb())));
            }
        }
        assert!(help.contains("back                   ") && help.contains("(not on this platform: exits 2)"));
        assert_eq!(find("tap_on").unwrap().lua_signature(), "device:tap_on(sel [, {optional, label}])");
        assert_eq!(find("extended_wait_until").unwrap().lua_signature(), "device:extended_wait_until{visible, not_visible, condition, timeout, interval, optional, label}");
        assert_eq!(find("get_device_state").unwrap().lua_signature(), "device:get_device_state([part])");
        assert_eq!(find("get_network_stub_hits").unwrap().lua_signature(), "device:get_network_stub_hits()");
        assert_eq!(find("back").unwrap().lua_signature(), "device:back([{optional, label}])");
        assert_eq!(find("draw_strokes_on").unwrap().lua_signature(), "device:draw_strokes_on(sel, {source, human, optional, label})", "source is required");
        assert!(find("add_network_stub").unwrap().guarded, "a stub rewrites Android's iptables");
        assert!(usage("browser", find("extended_wait_until").unwrap()).starts_with("wf browser extended-wait-until (--visible | --not-visible)"));
        assert_eq!(json("browser")["commands"].as_array().map(Vec::len), Some(COMMANDS.len()));
    }
}
