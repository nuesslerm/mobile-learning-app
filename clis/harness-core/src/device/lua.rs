//! The Lua side of the harness runs: a global `device` over a `Device`, plus `log`, `json`, `run_flow`, `assert_true`, `env`,
//! `sleep_ms`, `add_timing_span`, and `agent.run{...}` for the open-ended steps of a test (an agent works on the same device
//! while the script keeps the gates, the recording and the result).
//!
//! Every command of the table (`api.rs`) is a `device` method that turns its Lua arguments into `Args` and calls
//! `commands::call`, and nothing else is.
use super::api::{self, Command, Kind};
use super::commands::{self, Args, Ctx, Failure};
use super::golden::Goldens;
use super::{Device, Sel};
use crate::run_report::Trail;
use mlua::serde::{DeserializeOptions, SerializeOptions};
use mlua::{Error as LErr, Lua, LuaSerdeExt, MultiValue, Result as LRes, UserData, UserDataFields, UserDataMethods, Value, Variadic};
use serde_json::json;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command as Process, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

struct LuaDevice {
    dev: Rc<dyn Device>,
    goldens: Rc<Goldens>,
    trail: Rc<Trail>,
    out_dir: Option<PathBuf>,
    /// Set while a condition runs, where an action raises.
    in_condition: Rc<Cell<bool>>,
    /// The CLI's `--condition` state: every call is a condition's.
    read_only: bool,
}

/// The commands that count as a step of input for a golden's sidecar (its step number and last action).
const INPUTS: &[&str] = &["open_link", "tap_on", "swipe", "scroll_until_visible", "input_text", "press_key", "back", "draw_strokes_on", "set_microphone_input"];

fn to_lua(lua: &Lua, v: &serde_json::Value) -> LRes<Value> {
    lua.to_value_with(v, SerializeOptions::new().serialize_none_to_null(false).serialize_unit_to_null(false))
}

fn to_json(lua: &Lua, v: Value) -> LRes<serde_json::Value> {
    lua.from_value(v)
}

/// A condition's return as JSON, whatever it is (a function reads as its type name).
fn lenient(lua: &Lua, v: &Value) -> serde_json::Value {
    lua.from_value_with(v.clone(), DeserializeOptions::new().deny_unsupported_types(false)).unwrap_or_else(|_| json!(format!("<{}>", v.type_name())))
}

/// A Lua value as a script writes it, for an error line.
fn lua_literal(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => format!("'{s}'"),
        serde_json::Value::Array(a) => format!("{{{}}}", a.iter().map(lua_literal).collect::<Vec<_>>().join(", ")),
        serde_json::Value::Object(m) => format!("{{{}}}", m.iter().map(|(k, v)| format!("{k}={}", lua_literal(v))).collect::<Vec<_>>().join(", ")),
        other => other.to_string(),
    }
}

/// `tap_on{id='Go'}` or `extended_wait_until{visible={id='Go'}, timeout=30000}`: the call as the error names it.
fn show_call(cmd: &Command, args: &Args) -> String {
    let shown: Vec<(&str, &serde_json::Value)> = cmd
        .params
        .iter()
        .filter(|p| !matches!(p.name, "optional" | "label"))
        .filter_map(|p| args.get(p.name).filter(|v| !v.is_null() && p.default.and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok()).as_ref() != Some(*v)).map(|v| (p.name, v)))
        .collect();
    let sel = |v: &serde_json::Value| Sel::from_json(v).map(|s| s.show()).unwrap_or_else(|_| lua_literal(v));
    match shown.iter().find(|(name, _)| cmd.param(name).is_some_and(|p| p.positional && p.kind == Kind::Sel)) {
        Some((_, v)) => format!("{}{}", cmd.name, sel(v)),
        None => {
            let fields: Vec<String> = shown.iter().map(|(k, v)| format!("{k}={}", if cmd.param(k).is_some_and(|p| p.kind == Kind::Sel) { sel(v) } else if v.as_str() == Some("<function>") { "<function>".into() } else { lua_literal(v) })).collect();
            format!("{}{{{}}}", cmd.name, fields.join(", "))
        }
    }
}

/// `<file>:<line>: <command>{...} '<label>' <error> -- <verdict>; screenshot: <png>; evidence: <json>; next: <step>`.
fn failure_text(line: Option<String>, cmd: &Command, args: &Args, f: &Failure) -> String {
    let mut s = line.map(|l| format!("{l}: ")).unwrap_or_default();
    s.push_str(&show_call(cmd, args));
    if let Some(label) = args.get("label").and_then(serde_json::Value::as_str) {
        s.push_str(&format!(" '{label}'"));
    }
    s.push_str(&format!(" {}", f.error));
    if let Some(v) = f.verdict {
        s.push_str(&match f.met_after_ms {
            Some(ms) => format!(" -- {v}: met {ms} ms after the call"),
            None => format!(" -- {v}: never met"),
        });
    }
    if let Some(e) = &f.evidence {
        for (key, name) in [("screenshot", "screenshot"), ("path", "evidence")] {
            if let Some(p) = e[key].as_str() {
                s.push_str(&format!("; {name}: {p}"));
            }
        }
    }
    if let Some(n) = &f.next {
        s.push_str(&format!("; next: {n}"));
    }
    s
}

/// A command's Lua arguments as `Args`: the positional params in order, then one options table; a condition function
/// comes back on its own.
fn lua_args(lua: &Lua, cmd: &Command, args: MultiValue) -> Result<(Args, Option<mlua::Function>), String> {
    let mut vals: std::collections::VecDeque<Value> = args.into_iter().collect();
    let mut out = Args::new();
    let mut cond = None;
    for p in cmd.params.iter().filter(|p| p.positional) {
        match vals.front() {
            None => break,
            Some(Value::Table(_)) if !matches!(p.kind, Kind::Sel | Kind::Json | Kind::Rects) => break,
            Some(Value::Nil) => {
                vals.pop_front();
            }
            Some(_) => {
                let v = vals.pop_front().unwrap_or(Value::Nil);
                out.insert(p.name.into(), to_json(lua, v).map_err(|e| format!("{}: {e}", p.name))?);
            }
        }
    }
    match vals.pop_front() {
        None | Some(Value::Nil) => {}
        Some(Value::Table(t)) => {
            for pair in t.pairs::<Value, Value>() {
                let (k, v) = pair.map_err(|e| e.to_string())?;
                let Value::String(k) = k else { return Err(format!("the options are named, {{name=value}}: {}", cmd.lua_signature())) };
                let k = k.to_string_lossy().to_string();
                match (cmd.param(&k).map(|p| p.kind), v) {
                    (Some(Kind::Lua), Value::Function(f)) => {
                        cond = Some(f);
                        out.insert(k, json!("<function>"));
                    }
                    (Some(Kind::Lua), other) => return Err(format!("{k} must be a function, got {}", other.type_name())),
                    (_, v) => {
                        out.insert(k.clone(), to_json(lua, v).map_err(|e| format!("{k}: {e}"))?);
                    }
                }
            }
        }
        Some(other) => return Err(format!("expected an options table, got a {}: {}", other.type_name(), cmd.lua_signature())),
    }
    if !vals.is_empty() {
        return Err(format!("too many arguments: {}", cmd.lua_signature()));
    }
    Ok((out, cond))
}

impl LuaDevice {
    fn ctx(&self) -> Ctx<'_> {
        Ctx { out_dir: self.out_dir.clone(), in_condition: self.read_only || self.in_condition.get(), goldens: Some(&self.goldens), ..Ctx::default() }
    }

    /// One command from the table, as a `device` method.
    fn invoke(&self, lua: &Lua, cmd: &'static Command, args: MultiValue) -> LRes<MultiValue> {
        let line = caller_line(lua, 3);
        let at = line.as_deref().map(|l| format!("{l}: ")).unwrap_or_default();
        let (args, func) = lua_args(lua, cmd, args).map_err(|e| LErr::runtime(format!("{at}device:{}: {e}", cmd.name)))?;
        if INPUTS.contains(&cmd.name) && !self.in_condition.get() {
            self.goldens.note(Some(show_call(cmd, &args)));
        }
        let flag = self.in_condition.clone();
        let cond = func.map(|f| {
            move || -> Result<Option<serde_json::Value>, String> {
                let was = flag.replace(true);
                let r = f.call::<Value>(());
                flag.set(was);
                match r.map_err(|e| clean(&e))? {
                    Value::Nil => Ok(None),
                    v => Ok(Some(lenient(lua, &v))),
                }
            }
        });
        let ctx = self.ctx();
        let out = commands::call(&*self.dev, &ctx, cmd.name, &args, cond.as_ref().map(|c| c as &dyn Fn() -> Result<Option<serde_json::Value>, String>));
        let v = out.map_err(|f| LErr::runtime(failure_text(line, cmd, &args, &f)))?;
        match cmd.name {
            "take_screenshot" => {
                if let Some(p) = v["path"].as_str() {
                    self.trail.screenshots.borrow_mut().push(p.to_string());
                }
            }
            "get_logs" => {
                let lines: Vec<String> = v.as_array().into_iter().flatten().filter_map(|l| l.as_str()).filter(|l| l.starts_with("warn: ") || l.starts_with("error: ")).map(str::to_string).collect();
                self.trail.saw_warnings(&lines);
            }
            "add_network_stub" => self.trail.stubs.borrow_mut().push(commands::stub_spec(&args)),
            "run_shell" => return Ok(MultiValue::from_iter([to_lua(lua, &v["stdout"])?, to_lua(lua, &v["exit_code"])?])),
            _ => {}
        }
        Ok(MultiValue::from_iter([to_lua(lua, &v)?]))
    }

}

impl UserData for LuaDevice {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("platform", |_, t| Ok(t.dev.platform()));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        for cmd in api::COMMANDS.iter().filter(|c| c.surface != api::Surface::CliOnly) {
            m.add_method(cmd.name, move |lua, t, args: MultiValue| t.invoke(lua, cmd, args));
        }
    }
}

fn show(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::String(s) => s.to_string_lossy().to_string(),
        other => other.to_string().unwrap_or_else(|_| format!("{other:?}")),
    }
}

/// `agent.run{ prompt=..., model=..., timeout=..., json=bool }`: run a local agent (opencode by default, or the command
/// in `WF_AGENT_CMD`, which gets the prompt as its last argument) with this `wf` first on its PATH and the same device.
fn agent_run(lua: &Lua, serial: Option<&str>, opts: mlua::Table) -> LRes<mlua::Table> {
    let prompt: String = opts.get("prompt").map_err(|_| LErr::runtime("agent.run needs prompt="))?;
    let model: String = opts.get::<Option<String>>("model")?.unwrap_or_else(|| "minimax/MiniMax-M3".into());
    let timeout: u64 = opts.get::<Option<u64>>("timeout")?.unwrap_or(300);
    let want_json: bool = opts.get::<Option<bool>>("json")?.unwrap_or(false);
    let mut argv: Vec<String> = match std::env::var("WF_AGENT_CMD") {
        Ok(c) if !c.trim().is_empty() => c.split_whitespace().map(str::to_string).collect(),
        _ => vec!["opencode".into(), "run".into(), "--model".into(), model.clone()],
    };
    argv.push(prompt);
    let exe = std::env::current_exe().map_err(|e| LErr::runtime(e.to_string()))?;
    let dir = exe.parent().map(|p| p.display().to_string().trim_end_matches(" (deleted)").to_string()).unwrap_or_default();
    let path = format!("{dir}:{}", std::env::var("PATH").unwrap_or_default());
    let mut c = Process::new(&argv[0]);
    // its own process group, so a timeout can take down the agent and every command it started
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    c.args(&argv[1..]).env("PATH", path).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(s) = serial {
        c.env("ANDROID_SERIAL", s);
    }
    let started = Instant::now();
    let mut child = c.spawn().map_err(|e| LErr::runtime(format!("agent.run: cannot start {}: {e}", argv[0])))?;
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let (mut o, mut e) = (String::new(), String::new());
        if let Some(s) = out.as_mut() {
            let _ = s.read_to_string(&mut o);
        }
        if let Some(s) = err.as_mut() {
            let _ = s.read_to_string(&mut e);
        }
        (o, e)
    });
    let mut timed_out = false;
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(1),
            Ok(None) if started.elapsed() > Duration::from_secs(timeout) => {
                let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(child.id() as i32), nix::sys::signal::Signal::SIGKILL);
                let _ = child.wait();
                timed_out = true;
                break 124;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(LErr::runtime(format!("agent.run: {e}"))),
        }
    };
    let (text, stderr) = reader.join().unwrap_or_default();
    let res = lua.create_table()?;
    res.set("ok", code == 0 && !timed_out)?;
    res.set("exit", code)?;
    res.set("timed_out", timed_out)?;
    res.set("ms", started.elapsed().as_millis() as i64)?;
    res.set("text", text.clone())?;
    res.set("stderr", stderr)?;
    if want_json {
        // the last line of the agent's answer that parses as a JSON object or array
        if let Some(v) = text.lines().rev().map(str::trim).filter(|l| l.starts_with('{') || l.starts_with('[')).find_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()) {
            res.set("result", lua.to_value(&v)?)?;
        }
    }
    Ok(res)
}

pub struct Script<'a> {
    pub path: &'a str,
    pub source: &'a str,
    /// The `--env K=V` values.
    pub env: BTreeMap<String, String>,
    pub serial: Option<String>,
    pub goldens: Rc<Goldens>,
    pub trail: Rc<Trail>,
    /// The test output dir: a failure's screenshot and evidence go here.
    pub out_dir: PathBuf,
}

/// Why a script did not finish: `error` when it could not start (syntax, setup), `fail` when it raised while running.
pub struct ScriptError {
    pub verdict: &'static str,
    pub text: String,
}

/// A sleep in slices, so a stop (SIGTERM) ends it at its next slice.
fn sleep_for(lua: &Lua, ms: u64) -> LRes<()> {
    let _span = crate::spans::begin("sleep", "sleep", caller_line(lua, 1));
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        crate::run_stop::check().map_err(LErr::runtime)?;
        std::thread::sleep(end.saturating_duration_since(Instant::now()).min(Duration::from_millis(100)));
    }
    Ok(())
}

/// `device` as a script sees it: the table's methods, each a timed span that first honours a stop. Any other name is nil,
/// so calling one fails with Lua's own error at the script line.
fn bind_device(lua: &Lua, device: LuaDevice) -> LRes<()> {
    let g = lua.globals();
    g.set("device", device)?;
    let check = lua.create_function(|_, ()| crate::run_stop::check().map_err(LErr::runtime))?;
    let methods = lua.create_table()?;
    for cmd in api::COMMANDS.iter().filter(|c| c.surface != api::Surface::CliOnly) {
        methods.set(cmd.name, true)?;
    }
    let bind: mlua::Function = lua
        .load(
            "local raw, methods, check, timed = ...\n\
             return setmetatable({ platform = raw.platform }, { __index = function(_, k)\n\
               if not methods[k] then return nil end\n\
               local v = raw[k]\n\
               return function(_, ...) check() return timed(k, v, raw, ...) end\n\
             end })",
        )
        .into_function()?;
    let raw: Value = g.get("device")?;
    let bound: Value = bind.call((raw, methods, check, timed_call(lua)?))?;
    g.set("device", bound)
}

pub fn run(device: Rc<dyn Device>, s: Script) -> Result<(), ScriptError> {
    let lua = Lua::new();
    // a stop asked for while Lua itself runs (a loop with no device call) ends the script at the next instructions
    lua.set_global_hook(mlua::HookTriggers::new().every_nth_instruction(1000), |_, _| crate::run_stop::check().map(|_| mlua::VmState::Continue).map_err(LErr::runtime))
        .map_err(|e| ScriptError { verdict: "error", text: format!("cannot arm the stop hook: {e}") })?;
    let g = lua.globals();
    let setup = || -> LRes<()> {
        // a stopped run must not go on driving the device: every verb checks first, and a pcall in the script does not
        // swallow the stop (G2 and G3 jobs kept tapping through their grace period and lost their reports)
        let check = lua.create_function(|_, ()| crate::run_stop::check().map_err(LErr::runtime))?;
        lua.load(
            "local check = ...\n\
             local raw_pcall, raw_xpcall = pcall, xpcall\n\
             local function pass(ok, ...) if not ok then check() end return ok, ... end\n\
             pcall = function(f, ...) return pass(raw_pcall(f, ...)) end\n\
             xpcall = function(f, h, ...) return pass(raw_xpcall(f, h, ...)) end",
        )
        .call::<()>(check)?;
        bind_device(&lua, LuaDevice { dev: device.clone(), goldens: s.goldens.clone(), trail: s.trail.clone(), out_dir: Some(s.out_dir.clone()), in_condition: Rc::new(Cell::new(false)), read_only: false })?;
        g.set("log", lua.create_function(|_, vals: Variadic<Value>| {
            println!("{}", vals.iter().map(show).collect::<Vec<_>>().join("\t"));
            Ok(())
        })?)?;
        g.set("sleep_ms", lua.create_function(|lua, ms: f64| sleep_for(lua, ms.max(0.0) as u64))?)?;
        g.set("now_ms", lua.create_function(|_, ()| Ok(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)))?)?;
        let assert = lua.create_function(|_, (cond, msg): (Value, Option<String>)| match cond {
            Value::Nil | Value::Boolean(false) => Err(LErr::runtime(msg.unwrap_or_else(|| "assertion failed".into()))),
            _ => Ok(()),
        })?;
        g.set("assert_true", assert)?;
        g.set("run_flow", flow_fn(&lua)?)?;
        g.set("add_timing_span", lua.create_function(|_, (name, from, to): (String, f64, f64)| {
            crate::spans::record_between("step", &name, from.max(0.0) as u64, to.max(0.0) as u64);
            Ok(())
        })?)?;
        let json = lua.create_table()?;
        json.set("encode", lua.create_function(|lua, v: Value| {
            let j: serde_json::Value = lua.from_value(v)?;
            Ok(j.to_string())
        })?)?;
        // an empty Lua table encodes as {}; one marked as an array encodes as []
        json.set("array", lua.create_function(|lua, ()| {
            let t = lua.create_table()?;
            t.set_metatable(Some(lua.array_metatable()))?;
            Ok(t)
        })?)?;
        g.set("json", json)?;
        let env = lua.create_table()?;
        for (k, v) in &s.env {
            env.set(k.as_str(), v.as_str())?;
        }
        g.set("env", env)?;
        let agent = lua.create_table()?;
        let serial = s.serial.clone();
        agent.set("run", lua.create_function(move |lua, opts: mlua::Table| agent_run(lua, serial.as_deref(), opts))?)?;
        g.set("agent", agent)?;
        Ok(())
    };
    setup().map_err(|e| ScriptError { verdict: "error", text: e.to_string() })?;
    lua.load(s.source).set_name(format!("@{}", s.path)).exec().map_err(|e| ScriptError { verdict: if matches!(e, LErr::SyntaxError { .. }) { "error" } else { "fail" }, text: clean(&e) })
}

/// The CLI's `--condition '<expr>'`: the expression in a fresh Lua state whose `device` may only read, evaluated on each call.
pub fn condition(dev: Rc<dyn Device>, expr: &str) -> Result<impl Fn() -> Result<Option<serde_json::Value>, String>, String> {
    let lua = Lua::new();
    let tmp = std::env::temp_dir();
    let goldens = Rc::new(Goldens::new(tmp.clone(), crate::golden::Mode::Save, tmp));
    bind_device(&lua, LuaDevice { dev, goldens, trail: Rc::new(Trail::default()), out_dir: None, in_condition: Rc::new(Cell::new(true)), read_only: true }).map_err(|e| e.to_string())?;
    let f = lua.load(format!("return ({expr})")).set_name("=--condition").into_function().map_err(|e| format!("--condition: {}", clean(&e)))?;
    Ok(move || match f.call::<Value>(()).map_err(|e| clean(&e))? {
        Value::Nil => Ok(None),
        v => Ok(Some(lenient(&lua, &v))),
    })
}

/// `file:line` of the Lua code `level` frames up (1: the caller of the Rust function that asks).
fn caller_line(lua: &Lua, level: usize) -> Option<String> {
    lua.inspect_stack(level, |d| {
        let src = d.source();
        let file = src.short_src.as_deref().unwrap_or("?").to_string();
        d.current_line().map(|l| format!("{file}:{l}"))
    })
    .flatten()
}

/// The span name of a call and whether it is a lookup (its nil is a miss): a wait's name carries its mode, and `:optional`.
fn call_kind(name: &str, args: &MultiValue) -> (String, bool) {
    let table = |i: usize| match args.get(i + 1) {
        Some(Value::Table(t)) => Some(t.clone()),
        _ => None,
    };
    let optional = |t: Option<mlua::Table>| t.and_then(|t| t.get::<Option<bool>>("optional").ok().flatten()).unwrap_or(false);
    let suffix = |o: bool| if o { ":optional" } else { "" };
    match name {
        "extended_wait_until" => {
            let t = table(0);
            let mode = ["visible", "not_visible", "condition"].into_iter().find(|k| t.as_ref().is_some_and(|t| t.contains_key(*k).unwrap_or(false))).unwrap_or("visible");
            let o = optional(t);
            (format!("extended_wait_until:{mode}{}", suffix(o)), o)
        }
        "get_element" => (name.into(), true),
        _ => {
            // the options table follows the positional arguments
            let o = api::find(name).is_some_and(|c| optional(table(c.params.iter().filter(|p| p.positional).count())));
            (format!("{name}{}", if matches!(name, "assert_visible" | "assert_not_visible") { suffix(o) } else { "" }), o)
        }
    }
}

/// What a finished device call is worth to the timing table: a lookup that found nothing is a miss, a wait that ran out a
/// timeout, so slow misses and slow hits can be told apart.
fn call_outcome(lookup: bool, out: &LRes<MultiValue>) -> &'static str {
    match out {
        Err(e) => {
            let t = e.to_string();
            if t.contains("not visible after") || t.contains("still visible after") || t.contains("timed out") { "timeout" } else { "error" }
        }
        Ok(v) if lookup => match v.iter().next() {
            None | Some(Value::Nil) | Some(Value::Boolean(false)) => "miss",
            _ => "hit",
        },
        Ok(_) => "ok",
    }
}

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Every `device:` call as a span: its verb, the script line it came from, and its outcome (`crate::spans`). A call made by
/// a condition is a part of its wait, so its time is not counted twice.
fn timed_call(lua: &Lua) -> LRes<mlua::Function> {
    lua.create_function(|lua, mut args: MultiValue| {
        let name = match args.pop_front() {
            Some(Value::String(s)) => s.to_str()?.to_string(),
            _ => "call".to_string(),
        };
        let Some(Value::Function(f)) = args.pop_front() else {
            return Err(LErr::runtime("timed: no function"));
        };
        let (span_name, lookup) = call_kind(&name, &args);
        // 0 is this function, 1 the binding's wrapper, 2 the script line that called device:<verb>
        let depth = DEPTH.with(|d| d.replace(d.get() + 1));
        let mut span = crate::spans::begin(if depth > 0 { "inner" } else { "call" }, &span_name, caller_line(lua, 2));
        let out = f.call::<MultiValue>(args);
        DEPTH.with(|d| d.set(depth));
        span.outcome = call_outcome(lookup, &out);
        out
    })
}

/// `run_flow(name, fn)`: a named part of the script (open the route, wait for the transcript) as its own span, so a report
/// shows the time a backend took apart from the taps around it.
fn flow_fn(lua: &Lua) -> LRes<mlua::Function> {
    lua.create_function(move |lua, mut args: MultiValue| {
        let name: String = lua.unpack(args.pop_front().unwrap_or(Value::Nil))?;
        let f: mlua::Function = lua.unpack(args.pop_front().unwrap_or(Value::Nil))?;
        let mut span = crate::spans::begin("step", &name, caller_line(lua, 1));
        let out = f.call::<MultiValue>(());
        if out.is_err() {
            span.outcome = "error";
        }
        out
    })
}

/// A Lua error as one readable block: the message with its `script:line:` prefix, without the Rust-side wrapper text.
fn clean(e: &LErr) -> String {
    let t = e.to_string();
    t.replace("runtime error: ", "")
}
