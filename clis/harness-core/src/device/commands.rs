//! The dispatcher both front ends call (`cli.rs`, `lua.rs`): the arguments checked against the table, then the generic logic
//! of every command, once for both platforms: implicit waits, the one wait, scrolling, pixels, goldens, optional and evidence.
#![allow(clippy::result_large_err)]
use super::api::{self, Command};
use super::golden::{Goldens, Opts as GoldenOpts};
use super::sel::{Point, Sel};
use super::{Device, Element, Key, Level, Part, Rect};
use serde_json::{json, Map, Value};
use std::cell::Cell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub type Args = Map<String, Value>;

/// A condition's check: `Ok(None)` or a null or false value is not met yet; an `Err` fails the wait at once.
pub type Cond<'a> = &'a dyn Fn() -> Result<Option<Value>, String>;

pub struct Ctx<'a> {
    pub optional: bool,
    pub label: Option<String>,
    /// Where a failure's screenshot, hierarchy and evidence JSON go (the test output dir).
    pub out_dir: Option<PathBuf>,
    /// The call runs inside a condition, where an action raises.
    pub in_condition: bool,
    pub goldens: Option<&'a Goldens>,
    /// How long a timed-out wait keeps looking to tell slow from broken; None is `WF_WAIT_DIAGNOSE_MS`.
    pub diagnose_ms: Option<u64>,
    /// How many looks the last wait took.
    pub checks: Cell<u64>,
}

impl Default for Ctx<'_> {
    fn default() -> Self {
        Ctx { optional: false, label: None, out_dir: None, in_condition: false, goldens: None, diagnose_ms: None, checks: Cell::new(0) }
    }
}

#[derive(Debug, Default)]
pub struct Failure {
    pub error: String,
    pub usage: bool,
    pub evidence: Option<Value>,
    pub verdict: Option<&'static str>,
    pub met_after_ms: Option<u64>,
    pub next: Option<String>,
    /// The CLI's exit code when it is not 1: 3 when another run holds the Android lease.
    pub code: Option<i32>,
}

impl Failure {
    pub fn usage(error: impl Into<String>) -> Failure {
        Failure { error: error.into(), usage: true, ..Failure::default() }
    }
}

impl From<String> for Failure {
    fn from(error: String) -> Failure {
        Failure { error, ..Failure::default() }
    }
}

type Res<T> = Result<T, Failure>;

/// Runs one command. `cond` is `extended_wait_until`'s condition.
pub fn call(dev: &dyn Device, ctx: &Ctx, name: &str, args: &Args, cond: Option<Cond>) -> Res<Value> {
    let cmd = api::find(name).ok_or_else(|| Failure::usage(format!("unknown command '{name}'")))?;
    let given = args;
    let args = api::check(cmd, given).map_err(Failure::usage)?;
    if let Some(why) = cmd.absent_on(dev.platform()) {
        return Err(Failure::usage(format!("not on {}: {why}", dev.platform())));
    }
    if ctx.in_condition && cmd.action {
        return Err(Failure::usage(format!("a condition must not act: {name}")));
    }
    crate::run_stop::check()?;
    if cmd.guarded {
        dev.guard().map_err(|error| Failure { error, code: Some(3), next: Some("wf android lease status; wait for your turn, or take it with wf android lease acquire".into()), ..Failure::default() })?;
    }
    let run = Run { dev, ctx, cmd, args: &args, given, optional: ctx.optional || args.get("optional") == Some(&Value::Bool(true)) };
    // a wait's optional is its own miss; a condition that raises is a broken check, not a miss
    match run.go(cond) {
        Err(f) if run.optional && !f.usage && !cmd.is_wait() && !crate::run_stop::requested() => Ok(Value::Null),
        other => other,
    }
}

struct Run<'a> {
    dev: &'a dyn Device,
    ctx: &'a Ctx<'a>,
    cmd: &'static Command,
    args: &'a Args,
    /// The arguments as the caller gave them, before the defaults.
    given: &'a Args,
    optional: bool,
}

enum Mode {
    Visible(Sel),
    NotVisible(Sel),
    Condition,
}

impl Mode {
    fn name(&self) -> &'static str {
        match self {
            Mode::Visible(_) => "visible",
            Mode::NotVisible(_) => "not_visible",
            Mode::Condition => "condition",
        }
    }

    fn sel(&self) -> Option<&Sel> {
        match self {
            Mode::Visible(s) | Mode::NotVisible(s) => Some(s),
            Mode::Condition => None,
        }
    }
}

fn truthy(v: &Option<Value>) -> bool {
    !matches!(v, None | Some(Value::Null) | Some(Value::Bool(false)))
}

fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

/// Waits `d`, in slices, so a stop ends it at once.
fn pause(d: Duration) -> Res<()> {
    let end = Instant::now() + d;
    while Instant::now() < end {
        crate::run_stop::check()?;
        std::thread::sleep(end.saturating_duration_since(Instant::now()).min(Duration::from_millis(100)));
    }
    Ok(())
}

impl Run<'_> {
    fn str(&self, k: &str) -> Option<&str> {
        self.args.get(k).and_then(Value::as_str)
    }

    fn int(&self, k: &str) -> Option<u64> {
        self.args.get(k).and_then(Value::as_u64)
    }

    fn sel(&self, k: &str) -> Res<Sel> {
        let v = self.args.get(k).ok_or_else(|| Failure::usage(format!("{}: {k} is required", self.cmd.name)))?;
        let sel = Sel::from_json(v).map_err(|e| Failure::usage(format!("{}: {k}: {e}", self.cmd.name)))?;
        if sel.point.is_some() && self.cmd.name != "tap_on" {
            return Err(Failure::usage(format!("{}: a point is tap_on's only; look the element up by id, text or css", self.cmd.name)));
        }
        if sel.css.is_some() && self.dev.platform() == "android" {
            return Err(Failure::usage("not on android: a css selector needs a DOM"));
        }
        Ok(sel)
    }

    fn point(&self, k: &str) -> Res<Point> {
        Point::parse(self.str(k).unwrap_or_default()).map_err(|e| Failure::usage(format!("{}: {e}", self.cmd.name)))
    }

    fn label(&self) -> Option<String> {
        self.str("label").map(str::to_string).or_else(|| self.ctx.label.clone())
    }

    fn screen(&self) -> Res<(f64, f64)> {
        let s = self.dev.get_device_state(&[Part::Screen])?;
        match (s["screen"]["width"].as_f64(), s["screen"]["height"].as_f64()) {
            (Some(w), Some(h)) => Ok((w, h)),
            _ => Err(format!("the screen size is unreadable: {s}").into()),
        }
    }

    /// Screenshot pixels per tap unit.
    fn dpr(&self) -> Res<f64> {
        if self.dev.platform() == "android" {
            return Ok(1.0);
        }
        Ok(self.dev.get_device_state(&[Part::Screen])?["screen"]["dpr"].as_f64().unwrap_or(1.0))
    }

    /// A point in tap units; a percentage is of the screen.
    fn tap_units(&self, p: &Point) -> Res<(f64, f64)> {
        if p.is_pct() {
            let (w, h) = self.screen()?;
            Ok(p.resolve(w, h))
        } else {
            Ok(p.resolve(0.0, 0.0))
        }
    }

    fn go(&self, cond: Option<Cond>) -> Res<Value> {
        let dev = self.dev;
        match self.cmd.name {
            "open_link" => {
                let link = self.str("link").unwrap_or_default();
                dev.open_link(link)?;
                Ok(json!({ "link": link }))
            }
            "tap_on" => {
                let sel = self.sel("sel")?;
                if let Some(p) = &sel.point {
                    let (x, y) = self.tap_units(p)?;
                    dev.tap_at(x, y)?;
                    return Ok(json!({ "x": x, "y": y, "bounds": null }));
                }
                let el = self.implicit(&sel)?;
                let (x, y) = dev.tap_element(&el, &sel)?;
                Ok(json!({ "x": x, "y": y, "bounds": el.bounds.json() }))
            }
            "swipe" => {
                let (start, end) = (self.point("start")?, self.point("end")?);
                let duration = self.int("duration").unwrap_or_else(|| super::paced(400));
                dev.swipe(self.tap_units(&start)?, self.tap_units(&end)?, duration)?;
                Ok(json!({ "start": start.show(), "end": end.show(), "duration": duration }))
            }
            "scroll_until_visible" => self.scroll(),
            "input_text" => {
                let text = self.str("text").unwrap_or_default();
                self.keyboard_must_be(true)?;
                let t0 = Instant::now();
                dev.input_text(text)?;
                Ok(json!({ "typed": text, "ms": ms(t0.elapsed()) }))
            }
            "press_key" => {
                let key = Key::parse(self.str("key").unwrap_or_default()).map_err(Failure::usage)?;
                self.keyboard_must_be(true)?;
                dev.press_key(key)?;
                Ok(json!({ "key": key.name() }))
            }
            "back" => {
                self.keyboard_must_be(false)?;
                dev.back()?;
                Ok(json!({}))
            }
            "extended_wait_until" => {
                let modes = [self.args.contains_key("visible"), self.args.contains_key("not_visible"), self.args.contains_key("condition") || cond.is_some()];
                let mode = match modes {
                    [true, false, false] => Mode::Visible(self.sel("visible")?),
                    [false, true, false] => Mode::NotVisible(self.sel("not_visible")?),
                    [false, false, true] => Mode::Condition,
                    _ => return Err(Failure::usage("extended_wait_until takes exactly one of visible, not_visible and condition")),
                };
                if let Mode::Condition = mode {
                    if cond.is_none() {
                        return Err(Failure::usage("extended_wait_until: condition must be a function"));
                    }
                } else if self.given.contains_key("interval") {
                    return Err(Failure::usage("extended_wait_until: interval is for a condition; selector waits are event-driven"));
                }
                self.wait(mode, cond)
            }
            "assert_visible" => self.wait(Mode::Visible(self.sel("sel")?), None),
            "assert_not_visible" => self.wait(Mode::NotVisible(self.sel("sel")?), None),
            "take_screenshot" => {
                let path = with_ext(self.str("path").unwrap_or_default(), ".png");
                if let Some(dir) = std::path::Path::new(&path).parent().filter(|d| !d.as_os_str().is_empty()) {
                    let _ = std::fs::create_dir_all(dir);
                }
                let (w, h) = dev.take_screenshot(&path)?;
                Ok(json!({ "path": path, "width": w, "height": h }))
            }
            "assert_screenshot" => self.assert_screenshot(),
            "start_recording" => {
                let path = with_ext(self.str("path").unwrap_or_default(), ".mp4");
                dev.start_recording(&path)?;
                Ok(json!({ "path": path }))
            }
            "stop_recording" => Ok(json!({ "files": dev.stop_recording()? })),
            "get_element" => Ok(dev.get_element(&self.sel("sel")?)?.map_or(Value::Null, |e| e.json())),
            "get_device_state" => match self.str("part") {
                Some(p) => {
                    let part = Part::parse(p).ok_or_else(|| Failure::usage(format!("get_device_state: unknown part '{p}'")))?;
                    Ok(dev.get_device_state(&[part])?[part.name()].clone())
                }
                None => Ok(dev.get_device_state(&Part::ALL)?),
            },
            "get_pixel" => self.pixel(&self.point("point")?),
            "get_logs" => {
                let level = Level::parse(self.str("level").unwrap_or("warn")).unwrap_or(Level::Warn);
                Ok(json!(dev.get_logs(level)?))
            }
            "clear_logs" => Ok(json!({ "cleared": dev.clear_logs()? })),
            "set_microphone_input" => Ok(dev.set_microphone_input(self.str("path").unwrap_or_default())?),
            "draw_strokes_on" => {
                let sel = self.sel("sel")?;
                let el = self.implicit(&sel)?;
                Ok(dev.draw_strokes_on(&el, &sel, self.str("source").unwrap_or("guide"), self.args.get("human") == Some(&Value::Bool(true)))?)
            }
            "run_shell" => {
                let (stdout, code) = dev.run_shell(self.str("command").unwrap_or_default())?;
                Ok(json!({ "stdout": stdout, "exit_code": code }))
            }
            "add_network_stub" => Ok(dev.add_network_stub(&stub_spec(self.args))?),
            "get_network_stub_hits" => Ok(json!(dev.get_network_stub_hits()?)),
            "get_debug_hits" => Ok(json!(dev.get_debug_hits()?)),
            other => Err(Failure::usage(format!("{other} has no implementation"))),
        }
    }

    /// The element of tap_on and draw_strokes_on, waited for as Maestro's implicit lookup, then settled: its box holds still
    /// across two looks, so a tap does not land on a dialog still animating in (Android ignored a Reset tap that came first).
    fn implicit(&self, sel: &Sel) -> Res<Element> {
        let gone = || Failure::from(format!("{}: {} is not on screen", self.cmd.name, sel.show()));
        if let Some(el) = self.dev.settled(sel, None, super::settle_ms())? {
            return Ok(el);
        }
        let timeout = if self.optional { super::optional_lookup_ms() } else { super::wait_budget_ms() };
        let found = self.wait_with(Mode::Visible(sel.clone()), None, timeout, false)?;
        let seen = match element_of(&found) {
            Some(el) => el,
            None => self.dev.get_element(sel)?.ok_or_else(gone)?,
        };
        Ok(self.dev.settled(sel, Some(seen.clone()), super::settle_ms())?.unwrap_or(seen))
    }

    fn span_name(&self, mode: &Mode) -> String {
        let base = if self.cmd.name == "extended_wait_until" { format!("extended_wait_until:{}", mode.name()) } else { self.cmd.name.to_string() };
        if self.optional {
            format!("{base}:optional")
        } else {
            base
        }
    }

    fn wait(&self, mode: Mode, cond: Option<Cond>) -> Res<Value> {
        let timeout = self.int("timeout").unwrap_or(if self.optional { super::optional_lookup_ms() } else { super::wait_budget_ms() });
        self.wait_with(mode, cond, timeout, true)
    }

    /// input_text and press_key need the keyboard up and back needs it down, as a user moves it only by a tap: a refusal
    /// names that tap, with the screen as it stood.
    fn keyboard_must_be(&self, up: bool) -> Res<()> {
        if self.dev.keyboard_visible()? == up {
            return Ok(());
        }
        let (error, next) = if up {
            (format!("{} refused: the keyboard is down -- tap the field first", self.cmd.name), "tap the field first (tap_on), then type")
        } else {
            ("back refused: the keyboard is up, and Back would only lower it -- tap Done".to_string(), "tap Done to lower the keyboard, then go back")
        };
        Err(Failure { error, evidence: (!self.optional).then(|| self.evidence(None, None)), next: Some(next.into()), ..Failure::default() })
    }

    /// One wait: met returns its value; a miss is null when optional, else a failure with evidence and a slow or broken verdict.
    fn wait_with(&self, mode: Mode, cond: Option<Cond>, timeout: u64, log: bool) -> Res<Value> {
        let started = Instant::now();
        self.ctx.checks.set(0);
        let interval = self.int("interval").unwrap_or(200);
        if matches!(mode, Mode::Condition) && interval < 50 {
            return Err(Failure::usage(format!("{}: interval must be at least 50 ms, got {interval}", self.cmd.name)));
        }
        let mut last: Option<Value> = None;
        let out = match self.look(&mode, cond, timeout, interval, &mut last) {
            Ok(Some(v)) => Ok(v),
            Ok(None) if self.optional => Ok(Value::Null),
            Ok(None) => Err(self.timed_out(&mode, cond, timeout, interval, started, last)),
            Err(e) => Err(e),
        };
        if log {
            self.log_wait(&mode, started.elapsed(), matches!(&out, Ok(v) if !v.is_null()), timeout);
        }
        out
    }

    /// Looks until met or `timeout` ms pass: the met value, or None.
    fn look(&self, mode: &Mode, cond: Option<Cond>, timeout: u64, interval: u64, last: &mut Option<Value>) -> Res<Option<Value>> {
        let checks = &self.ctx.checks;
        match mode {
            Mode::Visible(sel) | Mode::NotVisible(sel) => {
                let want = matches!(mode, Mode::Visible(_));
                checks.set(checks.get() + 1);
                // a met wait answers the element its own look saw: no lookup after it
                let seen = match (timeout, want) {
                    (0, _) => Some(self.dev.get_element(sel)?).filter(|el| el.is_some() == want),
                    (_, true) => self.dev.wait_shown(sel, timeout)?,
                    (_, false) => self.dev.wait_visibility(sel, false, timeout)?.then_some(None),
                };
                Ok(seen.map(|el| if want { el.map_or_else(|| json!({}), |e| e.json()) } else { Value::Bool(true) }))
            }
            Mode::Condition => {
                let check = cond.ok_or_else(|| Failure::usage("a condition wait needs its function"))?;
                let end = Instant::now() + Duration::from_millis(timeout);
                loop {
                    checks.set(checks.get() + 1);
                    let looked = Instant::now();
                    let v = check().map_err(|e| Failure { error: format!("the condition raised: {e}"), ..Failure::default() })?;
                    if truthy(&v) {
                        return Ok(v);
                    }
                    *last = v;
                    let now = Instant::now();
                    if now >= end {
                        return Ok(None);
                    }
                    let gap = Duration::from_millis(interval).min(end - now);
                    if !self.dev.wait_for_change(gap.as_millis() as u64, looked.elapsed().as_millis() as u64)? {
                        pause(gap)?;
                    }
                }
            }
        }
    }

    /// A wait that ran out: evidence of the screen as it stood, then a further look to tell slow from broken.
    fn timed_out(&self, mode: &Mode, cond: Option<Cond>, timeout: u64, interval: u64, started: Instant, last: Option<Value>) -> Failure {
        let evidence = self.evidence(Some(mode), last.as_ref());
        let mut f = Failure { error: format!("timed out after {timeout} ms"), evidence: Some(evidence), ..Failure::default() };
        if timeout > 0 && !crate::run_stop::requested() {
            let checks = self.ctx.checks.get();
            let mut late = None;
            let met = matches!(self.look(mode, cond, self.ctx.diagnose_ms.unwrap_or_else(super::wait_diagnose_ms), interval, &mut late), Ok(Some(_)));
            self.ctx.checks.set(checks);
            f.verdict = Some(if met { "slow" } else { "broken" });
            f.met_after_ms = met.then(|| ms(started.elapsed()));
        }
        f.next = Some(next_step(mode, &f, self.dev.platform()));
        f
    }

    /// The screen as it stood: a screenshot, the hierarchy, the keyboard and the last warnings, plus the element of a
    /// selector wait or the last value of a condition.
    fn evidence(&self, mode: Option<&Mode>, last: Option<&Value>) -> Value {
        let dev = self.dev;
        let dir = self.ctx.out_dir.clone().unwrap_or_else(std::env::temp_dir);
        let _ = std::fs::create_dir_all(&dir);
        let stem = dir.join(format!("wait-timeout-{}", wf_kit::time::now_ms()));
        let shot = format!("{}.png", stem.display());
        let screenshot = dev.take_screenshot(&shot).map(|_| shot);
        let hierarchy = dev.hierarchy().ok().and_then(|h| {
            let path = format!("{}.hierarchy.json", stem.display());
            std::fs::write(&path, h.to_string()).ok().map(|()| path)
        });
        let mut e = json!({
            "screenshot": screenshot.as_ref().ok(),
            "hierarchy": hierarchy,
            "keyboard_visible": dev.get_device_state(&[Part::Keyboard]).ok().map(|k| k["keyboard"]["visible"].clone()),
            "logs": dev.get_logs(Level::Warn).map(|l| l.iter().rev().take(3).rev().cloned().collect::<Vec<_>>()).unwrap_or_default(),
        });
        if let Err(err) = &screenshot {
            e["screenshot_error"] = json!(err);
        }
        if let Some((mode, sel)) = mode.and_then(|m| m.sel().map(|s| (m, s))) {
            let size = |r: &Rect| format!("{},{} {}x{}", r.left, r.top, r.right - r.left, r.bottom - r.top);
            let matches = dev.count_matches(sel).ok();
            e["element"] = json!(match (mode, dev.get_element(sel), matches) {
                (_, Err(err), _) => format!("unreadable: {err}"),
                (_, Ok(None), Some(n)) if n > 0 => format!("present, not visible: {n} match{} off screen or hidden", if n == 1 { "" } else { "es" }),
                (_, Ok(None), _) => "absent".to_string(),
                (Mode::NotVisible(_), Ok(Some(el)), _) => format!("still visible at {}", size(&el.bounds)),
                (_, Ok(Some(el)), _) => format!("visible at {}", size(&el.bounds)),
            });
            e["matches"] = json!(matches);
        } else if let Some(Mode::Condition) = mode {
            let mut v = last.map(Value::to_string).unwrap_or_else(|| "nil".into());
            if v.len() > 2048 {
                v.truncate(v.char_indices().nth(2048).map_or(v.len(), |(i, _)| i));
                v.push('…');
            }
            e["last_value"] = json!(v);
            e["checks"] = json!(self.ctx.checks.get());
        }
        let path = format!("{}.json", stem.display());
        if std::fs::write(&path, format!("{e}\n")).is_ok() {
            e["path"] = json!(path);
        }
        e
    }

    /// With `WF_WAIT_LOG=<file>` every wait appends `{call, sel | label, ms, ok, timeoutMs, budgetMs, platform}`: the
    /// evidence the wait budget is set from.
    fn log_wait(&self, mode: &Mode, took: Duration, ok: bool, timeout: u64) {
        let Some(path) = std::env::var_os("WF_WAIT_LOG") else { return };
        let (call, target) = (self.span_name(mode), mode.sel().map_or(("label", json!(self.label())), |s| ("sel", json!(s.show()))));
        let mut line = json!({ "call": call, "ms": ms(took), "ok": ok, "timeoutMs": timeout, "budgetMs": super::wait_budget_ms(), "platform": self.dev.platform() });
        line[target.0] = target.1;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = std::io::Write::write_all(&mut f, format!("{line}\n").as_bytes());
        }
    }

    fn scroll(&self) -> Res<Value> {
        let element = self.sel("element")?;
        let from = if self.args.contains_key("from") { Some(self.sel("from")?) } else { None };
        let direction = self.str("direction").unwrap_or("DOWN");
        let timeout = self.int("timeout").unwrap_or(20_000);
        let want = self.int("visibility_percentage").unwrap_or(100).min(100) as f64;
        let (sw, sh) = self.screen()?;
        let started = Instant::now();
        let mut swipes = 0u64;
        loop {
            self.ctx.checks.set(swipes + 1);
            let (found, region_seen) = match &from {
                Some(sel) => self.dev.get_element_with(&element, sel, want)?,
                None => (self.dev.get_element(&element)?, None),
            };
            if let Some(el) = found {
                if visible_pct(&el) >= want {
                    let mut v = el.json();
                    v["swipes"] = json!(swipes);
                    return Ok(v);
                }
            }
            if ms(started.elapsed()) >= timeout {
                let error = format!("{} not {}% on screen after {swipes} swipes in {timeout} ms", element.show(), want);
                // an optional miss is nil, with no evidence to take
                if self.optional {
                    return Err(Failure::from(error));
                }
                let mut f = Failure { error, evidence: Some(self.evidence(Some(&Mode::Visible(element.clone())), None)), ..Failure::default() };
                if let Some(e) = f.evidence.as_mut() {
                    e["swipes"] = json!(swipes);
                }
                f.next = Some(format!("check the direction ({direction}) and the region it scrolls (from), against the hierarchy"));
                return Err(f);
            }
            let region = match &from {
                Some(sel) => match region_seen {
                    Some(seen) => seen,
                    None => self.dev.get_element(sel)?,
                }
                .map(|e| e.bounds)
                .ok_or_else(|| Failure::from(format!("scroll_until_visible: from {} is not on screen", sel.show())))?,
                None => Rect { left: 0, top: 0, right: sw as i64, bottom: sh as i64 },
            };
            let (l, t, r, b) = (region.left as f64, region.top as f64, region.right as f64, region.bottom as f64);
            let (cx, cy, dx, dy) = ((l + r) / 2.0, (t + b) / 2.0, (r - l) * 0.1, (b - t) * 0.1);
            // sideways a thumb crosses the row edge to edge; up and down start at the centre, since a swipe from a screen
            // edge can land on Android's navigation bar or the browser keyboard
            let (start, to) = match direction {
                "UP" => ((cx, cy), (cx, b - dy)),
                "LEFT" => ((l + dx, cy), (r - dx, cy)),
                "RIGHT" => ((r - dx, cy), (l + dx, cy)),
                _ => ((cx, cy), (cx, t + dy)),
            };
            self.dev.swipe(start, to, super::paced(600))?;
            swipes += 1;
            crate::run_stop::check()?;
        }
    }

    fn pixel(&self, point: &Point) -> Res<Value> {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tmp = std::env::temp_dir().join(format!("wf-pixel-{}-{}.png", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let path = tmp.display().to_string();
        self.dev.take_screenshot(&path)?;
        let bytes = std::fs::read(&tmp).map_err(|e| format!("get_pixel: cannot read the screenshot: {e}"));
        let _ = std::fs::remove_file(&tmp);
        let img = crate::golden::decode(&bytes?)?;
        let (x, y) = if point.is_pct() {
            point.resolve(img.width as f64, img.height as f64)
        } else {
            let d = self.dpr()?;
            let (x, y) = point.resolve(0.0, 0.0);
            (x * d, y * d)
        };
        let (x, y) = (x.floor().max(0.0) as usize, y.floor().max(0.0) as usize);
        if x >= img.width || y >= img.height {
            return Err(format!("({x},{y}) is outside the {}x{} screen", img.width, img.height).into());
        }
        let i = (y * img.width + x) * 4;
        Ok(json!({ "r": img.rgba[i], "g": img.rgba[i + 1], "b": img.rgba[i + 2] }))
    }

    fn assert_screenshot(&self) -> Res<Value> {
        let goldens = self.ctx.goldens.ok_or_else(|| Failure::usage("assert_screenshot needs the run's goldens"))?;
        let path = self.str("path").unwrap_or_default();
        let name = path.strip_suffix(".png").unwrap_or(path);
        let pct = self.args.get("threshold_percentage").and_then(Value::as_f64).unwrap_or(95.0);
        if !(0.0..=100.0).contains(&pct) {
            return Err(Failure::usage(format!("assert_screenshot: threshold_percentage is 0 to 100, got {pct}")));
        }
        let d = self.dpr()?;
        let ignore = self.args.get("ignore").and_then(Value::as_array).map(|rects| rects.iter().filter_map(|r| r.as_array()).map(|r| {
            let n = |i: usize| (r[i].as_f64().unwrap_or(0.0) * d).round() as i64;
            (n(0), n(1), n(2), n(3))
        }).collect()).unwrap_or_default();
        let opts = GoldenOpts { tolerance: Some(1.0 - pct / 100.0), threshold: None, max_width: None, ignore };
        let mode = if goldens.mode == crate::golden::Mode::Update { crate::golden::Mode::Update } else { crate::golden::Mode::Compare };
        let o = goldens.capture_as(self.dev, name, &opts, mode)?;
        let matched = ((1.0 - o.ratio) * 10_000.0).round() / 100.0;
        if !o.ok() {
            return Err(Failure {
                error: o.message.clone(),
                evidence: Some(json!({ "actual": o.actual, "expected": o.expected, "diff": o.diff, "match_percentage": matched, "golden": o.golden })),
                next: Some(if o.verdict == "missing" { "record it: run the test with --update-goldens and review the image".into() } else { "compare the diff; if the change is meant, run with --update-goldens and review it".into() }),
                ..Failure::default()
            });
        }
        Ok(json!({ "path": o.golden, "verdict": o.verdict, "match_percentage": matched }))
    }
}

/// The element a wait returned, to tap: its JSON carries no whole box, which a tap does not need.
fn element_of(v: &Value) -> Option<Element> {
    let b = &v["bounds"];
    let n = |k: &str| b[k].as_i64();
    let bounds = Rect { left: n("left")?, top: n("top")?, right: n("right")?, bottom: n("bottom")? };
    Some(Element { bounds, full: bounds, text: v["text"].as_str().map(str::to_string), frame: "app" })
}

/// How much of the element shows: its on-screen part over its whole box.
fn visible_pct(el: &Element) -> f64 {
    let area = |r: &Rect| ((r.right - r.left).max(0) * (r.bottom - r.top).max(0)) as f64;
    let full = area(&el.full);
    if full == 0.0 {
        0.0
    } else {
        (100.0 * area(&el.bounds) / full).min(100.0)
    }
}

fn with_ext(path: &str, ext: &str) -> String {
    if path.ends_with(ext) {
        path.to_string()
    } else {
        format!("{path}{ext}")
    }
}

/// The step a failed wait points to, chosen from its evidence.
fn next_step(mode: &Mode, f: &Failure, platform: &str) -> String {
    let e = f.evidence.clone().unwrap_or(Value::Null);
    if f.verdict == Some("slow") {
        let up = f.met_after_ms.unwrap_or(0).div_ceil(1000) * 1000;
        return format!("raise timeout to {up} if this is slow by design, else find what slowed it");
    }
    match mode {
        Mode::Condition => format!("the last value was {}: check the condition", e["last_value"].as_str().unwrap_or("nil")),
        Mode::NotVisible(_) => "it stayed on screen: check what should have removed it".into(),
        Mode::Visible(sel) if e["element"].as_str().is_some_and(|s| s.starts_with("present")) => format!("scroll_until_visible{{element={}}} first", sel.show()),
        Mode::Visible(_) => match e["hierarchy"].as_str() {
            Some(h) => format!("the selector matched nothing: compare it with {h}"),
            None if platform == "browser" => "the selector matched nothing: compare it with the screenshot and `wf browser hierarchy`".into(),
            None => "the selector matched nothing: compare it with the screenshot".into(),
        },
    }
}

/// A stub as the drivers take it: `**` globs read as `*`, the method upper-cased (none for '*').
pub fn stub_spec(args: &Args) -> Value {
    let mut spec = Map::new();
    for (k, v) in args {
        if !matches!(k.as_str(), "method" | "url_pattern" | "optional" | "label") && !v.is_null() {
            spec.insert(k.clone(), v.clone());
        }
    }
    spec.insert("pattern".into(), json!(args.get("url_pattern").and_then(Value::as_str).unwrap_or("*").replace("**", "*")));
    let method = args.get("method").and_then(Value::as_str).unwrap_or("*");
    if method != "*" && !method.eq_ignore_ascii_case("any") {
        spec.insert("method".into(), json!(method.to_ascii_uppercase()));
    }
    Value::Object(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A screen of named elements that appear after a number of looks, recording every call that reaches it.
    #[derive(Default)]
    struct Fake {
        /// (id, looks before it shows)
        shows_after: RefCell<Vec<(String, u64)>>,
        looks: Cell<u64>,
        calls: RefCell<Vec<String>>,
        keyboard: Cell<bool>,
    }

    impl Fake {
        fn with(id: &str, after: u64) -> Fake {
            let f = Fake::default();
            f.shows_after.borrow_mut().push((id.into(), after));
            f
        }

        fn id_of(sel: &Sel) -> String {
            sel.id.as_ref().or(sel.text.as_ref()).map_or_else(|| sel.css.clone().unwrap_or_default(), |p| p.source().to_string())
        }

        fn note(&self, c: &str) {
            self.calls.borrow_mut().push(c.into());
        }
    }

    impl Device for Fake {
        fn platform(&self) -> &'static str {
            "android"
        }
        fn open_link(&self, link: &str) -> Result<(), String> {
            self.note(&format!("open {link}"));
            Ok(())
        }
        fn tap_at(&self, x: f64, y: f64) -> Result<(), String> {
            self.note(&format!("tap {x},{y}"));
            Ok(())
        }
        fn tap_element(&self, el: &Element, sel: &Sel) -> Result<(f64, f64), String> {
            self.note(&format!("tap {}", Fake::id_of(sel)));
            let b = el.bounds;
            Ok(((b.left + b.right) as f64 / 2.0, (b.top + b.bottom) as f64 / 2.0))
        }
        fn swipe(&self, from: (f64, f64), to: (f64, f64), ms: u64) -> Result<(), String> {
            self.note(&format!("swipe {},{} {},{} {ms}", from.0, from.1, to.0, to.1));
            Ok(())
        }
        fn input_text(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn press_key(&self, key: Key) -> Result<(), String> {
            self.note(&format!("press {}", key.short()));
            Ok(())
        }
        fn back(&self) -> Result<(), String> {
            self.note("back");
            Ok(())
        }
        fn draw_strokes_on(&self, _: &Element, _: &Sel, _: &str, _: bool) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn set_microphone_input(&self, path: &str) -> Result<Value, String> {
            Ok(json!({ "path": path }))
        }
        fn run_shell(&self, _: &str) -> Result<(String, i32), String> {
            Ok((String::new(), 0))
        }
        fn take_screenshot(&self, path: &str) -> Result<(u32, u32), String> {
            self.note("screenshot");
            let mut bytes = Vec::new();
            let mut e = png::Encoder::new(&mut bytes, 2, 2);
            e.set_color(png::ColorType::Rgb);
            e.write_header().and_then(|mut w| w.write_image_data(&[0, 0, 255, 255, 255, 255, 255, 255, 255, 1, 2, 3])).map_err(|e| e.to_string())?;
            std::fs::write(path, bytes).map_err(|e| e.to_string())?;
            Ok((2, 2))
        }
        fn start_recording(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn stop_recording(&self) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        fn get_element(&self, sel: &Sel) -> Result<Option<Element>, String> {
            self.looks.set(self.looks.get() + 1);
            let id = Fake::id_of(sel);
            let shown = self.shows_after.borrow().iter().any(|(i, after)| *i == id && self.looks.get() >= *after);
            let bounds = Rect { left: 10, top: 20, right: 110, bottom: 60 };
            Ok(shown.then_some(Element { bounds, full: bounds, text: Some(id), frame: "native" }))
        }
        fn count_matches(&self, sel: &Sel) -> Result<usize, String> {
            self.looks.set(self.looks.get() + 1);
            Ok(self.shows_after.borrow().iter().filter(|(i, _)| *i == Fake::id_of(sel)).count())
        }
        fn get_device_state(&self, parts: &[Part]) -> Result<Value, String> {
            let mut out = Map::new();
            for p in parts {
                let v = match p {
                    Part::Screen => json!({ "width": 400, "height": 800, "dpr": 1 }),
                    Part::Keyboard => json!({ "visible": self.keyboard.get(), "bounds": null }),
                    Part::Microphone => json!({ "input": null, "consumed": false }),
                };
                out.insert(p.name().into(), v);
            }
            Ok(Value::Object(out))
        }
        fn get_logs(&self, _: Level) -> Result<Vec<String>, String> {
            Ok(vec!["warn: a".into(), "warn: b".into(), "warn: c".into(), "warn: d".into()])
        }
        fn clear_logs(&self) -> Result<usize, String> {
            Ok(4)
        }
        fn hierarchy(&self) -> Result<Value, String> {
            Err("no hierarchy".into())
        }
        fn wait_visibility(&self, sel: &Sel, want: bool, ms: u64) -> Result<bool, String> {
            let end = Instant::now() + Duration::from_millis(ms);
            loop {
                if self.get_element(sel)?.is_some() == want {
                    return Ok(true);
                }
                if Instant::now() >= end {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        fn add_network_stub(&self, spec: &Value) -> Result<Value, String> {
            Ok(spec.clone())
        }
        fn get_network_stub_hits(&self) -> Result<Vec<Value>, String> {
            Ok(vec![])
        }
        fn get_debug_hits(&self) -> Result<Vec<Value>, String> {
            Ok(vec![])
        }
        fn arm_logpoints(&self, _: &[super::super::Logpoint]) -> Result<(), String> {
            Ok(())
        }
    }

    fn args(v: Value) -> Args {
        v.as_object().cloned().unwrap_or_default()
    }

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn given_a_condition_when_it_turns_truthy_then_the_wait_returns_its_value_and_a_falsy_one_times_out_broken() {
        let (dev, out) = (Fake::default(), dir());
        let ctx = Ctx { out_dir: Some(out.path().into()), diagnose_ms: Some(100), ..Ctx::default() };
        let n = Cell::new(0);
        let cond = || -> Result<Option<Value>, String> {
            n.set(n.get() + 1);
            Ok(Some(if n.get() >= 3 { json!({ "buffer": "abc" }) } else { json!(false) }))
        };
        let met = call(&dev, &ctx, "extended_wait_until", &args(json!({ "condition": "<function>", "interval": 50, "timeout": 2000 })), Some(&cond)).unwrap();
        assert_eq!((met, ctx.checks.get()), (json!({ "buffer": "abc" }), 3));

        let never = || -> Result<Option<Value>, String> { Ok(Some(json!(0))) };
        let falsy = || -> Result<Option<Value>, String> { Ok(Some(json!(false))) };
        let f = call(&dev, &ctx, "extended_wait_until", &args(json!({ "condition": "<function>", "interval": 50, "timeout": 120 })), Some(&falsy)).unwrap_err();
        assert_eq!((f.error.as_str(), f.verdict, f.usage), ("timed out after 120 ms", Some("broken"), false));
        let e = f.evidence.unwrap();
        assert_eq!(e["last_value"], "false");
        assert!(e["checks"].as_u64() >= Some(2) && std::path::Path::new(e["path"].as_str().unwrap()).is_file() && e["screenshot"].as_str().is_some_and(|s| s.ends_with(".png")), "{e}");
        assert_eq!(e["logs"].as_array().map(Vec::len), Some(3));
        assert!(f.next.unwrap().contains("the last value was false"));
        assert_eq!(call(&dev, &ctx, "extended_wait_until", &args(json!({ "condition": "<function>", "timeout": 0 })), Some(&never)).unwrap(), json!(0), "0 is truthy, as in Lua");
    }

    #[test]
    fn given_a_condition_that_raises_or_acts_when_waited_on_then_the_wait_fails_at_once() {
        let dev = Fake::default();
        let ctx = Ctx::default();
        let raises = || -> Result<Option<Value>, String> { Err("t.lua:3: attempt to index a nil value".into()) };
        let t0 = Instant::now();
        let f = call(&dev, &ctx, "extended_wait_until", &args(json!({ "condition": "<function>", "timeout": 5000 })), Some(&raises)).unwrap_err();
        assert!(f.error.contains("the condition raised: t.lua:3") && f.verdict.is_none() && t0.elapsed() < Duration::from_secs(1), "{f:?}");
        let optional = call(&dev, &ctx, "extended_wait_until", &args(json!({ "condition": "<function>", "timeout": 5000, "optional": true })), Some(&raises));
        assert!(optional.is_err_and(|f| f.error.contains("the condition raised")), "optional covers a miss, not a broken check");

        let inside = Ctx { in_condition: true, ..Ctx::default() };
        let acted = call(&dev, &inside, "tap_on", &args(json!({ "sel": { "id": "Go" } })), None).unwrap_err();
        assert_eq!((acted.error.as_str(), acted.usage), ("a condition must not act: tap_on", true));
        assert!(dev.calls.borrow().is_empty(), "nothing reached the device: {:?}", dev.calls.borrow());
        assert!(call(&dev, &inside, "get_element", &args(json!({ "sel": { "id": "Go" } })), None).is_ok(), "a read is fine inside a condition");
    }

    #[test]
    fn given_an_optional_wait_when_it_misses_then_it_is_null_without_evidence_and_timeout_0_looks_once() {
        let (dev, out) = (Fake::default(), dir());
        let ctx = Ctx { out_dir: Some(out.path().into()), ..Ctx::default() };
        let miss = call(&dev, &ctx, "assert_visible", &args(json!({ "sel": { "id": "Nope" }, "optional": true, "timeout": 30 })), None).unwrap();
        assert_eq!(miss, Value::Null);
        assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0, "no evidence for an optional miss");
        assert!(!dev.calls.borrow().iter().any(|c| c == "screenshot"));

        let before = dev.looks.get();
        let once = call(&dev, &ctx, "extended_wait_until", &args(json!({ "visible": { "id": "Nope" }, "timeout": 0 })), None).unwrap_err();
        assert_eq!((dev.looks.get() - before, once.verdict, once.error.as_str()), (3, None, "timed out after 0 ms"), "one look, then the evidence's element and match count");
    }

    #[test]
    fn given_a_visible_wait_when_the_element_shows_then_the_element_comes_back_and_a_late_one_is_slow() {
        let (dev, out) = (Fake::with("Go", 4), dir());
        let ctx = Ctx { out_dir: Some(out.path().into()), ..Ctx::default() };
        let el = call(&dev, &ctx, "assert_visible", &args(json!({ "sel": { "id": "Go" }, "timeout": 2000 })), None).unwrap();
        assert_eq!(el, json!({ "bounds": { "left": 10, "top": 20, "right": 110, "bottom": 60, "cx": 60, "cy": 40 }, "text": "Go" }));
        assert_eq!(call(&dev, &ctx, "assert_not_visible", &args(json!({ "sel": { "id": "Gone" } })), None).unwrap(), json!(true));

        let ctx = Ctx { out_dir: Some(out.path().into()), diagnose_ms: Some(2000), ..Ctx::default() };
        let late = Fake::with("Go", 40);
        let f = call(&late, &ctx, "extended_wait_until", &args(json!({ "visible": { "id": "Go" }, "timeout": 20 })), None).unwrap_err();
        assert_eq!(f.verdict, Some("slow"), "{f:?}");
        assert!(f.met_after_ms.is_some() && f.next.unwrap().starts_with("raise timeout to"));
        let e = f.evidence.unwrap();
        assert_eq!((e["element"].clone(), e["matches"].clone()), (json!("present, not visible: 1 match off screen or hidden"), json!(1)), "in the tree, not yet shown");
        let never = call(&Fake::default(), &Ctx { out_dir: Some(out.path().into()), diagnose_ms: Some(0), ..Ctx::default() }, "assert_visible", &args(json!({ "sel": { "id": "Go" }, "timeout": 0 })), None).unwrap_err();
        assert_eq!(never.evidence.unwrap()["element"], "absent");
    }

    #[test]
    fn given_wait_arguments_when_they_contradict_then_each_is_a_usage_error() {
        let (dev, ctx) = (Fake::default(), Ctx::default());
        let cond = || -> Result<Option<Value>, String> { Ok(Some(json!(true))) };
        let usage = |a: Value, c: Option<Cond>| call(&dev, &ctx, "extended_wait_until", &args(a), c).unwrap_err();
        assert!(usage(json!({ "visible": "Go", "not_visible": "Go" }), None).error.contains("exactly one"));
        assert!(usage(json!({}), None).error.contains("exactly one"));
        assert!(usage(json!({ "visible": "Go", "interval": 300 }), None).error.contains("interval is for a condition"));
        assert!(usage(json!({ "not_visible": "Go", "interval": 200 }), None).error.contains("interval is for a condition"), "even at its default");
        let low = usage(json!({ "condition": "<function>", "interval": 10 }), Some(&cond));
        assert!(low.usage && low.error.contains("at least 50"), "{low:?}");
        assert!(usage(json!({ "visible": "id:Go" }), None).error.contains("old selector form"));
        assert!(call(&dev, &ctx, "get_element", &args(json!({ "sel": { "point": "1,2" } })), None).unwrap_err().error.contains("tap_on's only"));
        assert!(call(&dev, &ctx, "get_element", &args(json!({ "sel": { "css": ".x" } })), None).unwrap_err().error.contains("not on android"));
        assert!(call(&dev, &ctx, "tap_on", &args(json!({ "sel": "Go", "bogus": 1 })), None).unwrap_err().usage);
    }

    #[test]
    fn given_actions_when_called_then_implicit_waits_points_pixels_and_keys_reach_the_device() {
        let (dev, ctx) = (Fake::with("Go", 2), Ctx::default());
        let tapped = call(&dev, &ctx, "tap_on", &args(json!({ "sel": { "id": "Go" } })), None).unwrap();
        assert_eq!((tapped["x"].clone(), tapped["y"].clone()), (json!(60.0), json!(40.0)));
        call(&dev, &ctx, "tap_on", &args(json!({ "sel": { "point": "50%,25%" } })), None).unwrap();
        call(&dev, &ctx, "swipe", &args(json!({ "start": "10,20", "end": "10%,10%" })), None).unwrap();
        let refused = call(&dev, &ctx, "press_key", &args(json!({ "key": "Enter" })), None).unwrap_err();
        assert!(refused.error.contains("tap the field first") && refused.evidence.is_some(), "the keyboard is down: {refused:?}");
        dev.keyboard.set(true);
        call(&dev, &ctx, "press_key", &args(json!({ "key": "Enter" })), None).unwrap();
        call(&dev, &ctx, "press_key", &args(json!({ "key": "remote dpad up" })), None).unwrap();
        // the swipe's default 400 ms at this process's WF_TEST_SPEED (pace itself is tested in device/mod.rs)
        let swiped = format!("swipe 10,20 40,80 {}", crate::device::paced(400));
        assert_eq!(*dev.calls.borrow(), ["tap Go", "tap 200,200", swiped.as_str(), "screenshot", "press enter", "press up"]);
        assert_eq!(call(&dev, &ctx, "tap_on", &args(json!({ "sel": { "id": "Nope" }, "optional": true })), None).unwrap(), Value::Null);

        let out = dir();
        let shot = call(&dev, &ctx, "take_screenshot", &args(json!({ "path": out.path().join("first").display().to_string() })), None).unwrap();
        assert_eq!((shot["path"].as_str().map(|p| p.ends_with("first.png")), shot["width"].clone(), shot["height"].clone()), (Some(true), json!(2), json!(2)));
        let blue = call(&dev, &ctx, "get_pixel", &args(json!({ "point": "0,0" })), None).unwrap();
        assert_eq!(blue, json!({ "r": 0, "g": 0, "b": 255 }));
        assert!(call(&dev, &ctx, "get_pixel", &args(json!({ "point": "9,9" })), None).unwrap_err().error.contains("outside the 2x2 screen"));
        assert_eq!(call(&dev, &ctx, "get_device_state", &args(json!({ "part": "screen" })), None).unwrap(), json!({ "width": 400, "height": 800, "dpr": 1 }));
        let up = call(&dev, &ctx, "back", &args(json!({})), None).unwrap_err();
        assert!(up.error.contains("keyboard is up") && up.next.as_deref().is_some_and(|n| n.starts_with("tap Done")) && up.evidence.is_some(), "{up:?}");
        assert!(call(&dev, &ctx, "press_key", &args(json!({ "key": "Escape" })), None).unwrap_err().usage);
    }

    #[test]
    fn given_the_keyboard_down_when_text_or_a_key_is_sent_then_each_is_refused_with_the_tap_that_raises_it() {
        let (dev, out) = (Fake::default(), dir());
        let ctx = Ctx { out_dir: Some(out.path().into()), ..Ctx::default() };
        for (name, a) in [("input_text", json!({ "text": "x" })), ("press_key", json!({ "key": "Backspace" }))] {
            let f = call(&dev, &ctx, name, &args(a), None).unwrap_err();
            assert!(f.error == format!("{name} refused: the keyboard is down -- tap the field first") && f.next.as_deref() == Some("tap the field first (tap_on), then type"), "{f:?}");
            let e = f.evidence.unwrap();
            assert!(e["keyboard_visible"] == false && e["screenshot"].as_str().is_some() && std::path::Path::new(e["path"].as_str().unwrap()).is_file(), "{e}");
        }
        assert!(!dev.calls.borrow().iter().any(|c| c.starts_with("press")), "nothing was sent: {:?}", dev.calls.borrow());
        assert_eq!(call(&dev, &ctx, "back", &args(json!({})), None).unwrap(), json!({}));
        let shots = dev.calls.borrow().iter().filter(|c| *c == "screenshot").count();
        assert_eq!(call(&dev, &ctx, "input_text", &args(json!({ "text": "x", "optional": true })), None).unwrap(), Value::Null);
        assert_eq!(dev.calls.borrow().iter().filter(|c| *c == "screenshot").count(), shots, "an optional refusal takes no evidence");
    }

    #[test]
    fn given_an_optional_scroll_when_it_misses_then_it_is_nil_without_evidence() {
        let (dev, out) = (Fake::default(), dir());
        let ctx = Ctx { out_dir: Some(out.path().into()), ..Ctx::default() };
        let miss = call(&dev, &ctx, "scroll_until_visible", &args(json!({ "element": { "id": "Nope" }, "timeout": 0, "optional": true })), None).unwrap();
        assert_eq!((miss, std::fs::read_dir(out.path()).unwrap().count()), (Value::Null, 0));
        assert!(!dev.calls.borrow().iter().any(|c| c == "screenshot"));
        let f = call(&dev, &ctx, "scroll_until_visible", &args(json!({ "element": { "id": "Nope" }, "timeout": 0 })), None).unwrap_err();
        assert!(f.evidence.is_some_and(|e| e["swipes"] == 0), "a required miss keeps its evidence");
    }

    #[test]
    fn given_a_golden_when_the_screen_is_asserted_then_a_missing_one_fails_an_update_writes_it_and_the_same_screen_matches() {
        let (dev, goldens, run) = (Fake::default(), dir(), dir());
        let compare = Goldens::new(goldens.path().into(), crate::golden::Mode::Compare, run.path().into());
        let update = Goldens::new(goldens.path().into(), crate::golden::Mode::Update, run.path().into());
        let (ctx, updating) = (Ctx { goldens: Some(&compare), ..Ctx::default() }, Ctx { goldens: Some(&update), ..Ctx::default() });
        let missing = call(&dev, &ctx, "assert_screenshot", &args(json!({ "path": "home.png" })), None).unwrap_err();
        assert!(missing.error.contains("no golden at") && missing.next.as_deref().is_some_and(|n| n.starts_with("record it")), "{missing:?}");
        assert_eq!(call(&dev, &updating, "assert_screenshot", &args(json!({ "path": "home" })), None).unwrap()["verdict"], "updated");
        let matched = call(&dev, &ctx, "assert_screenshot", &args(json!({ "path": "home", "threshold_percentage": 100, "ignore": [[0, 0, 1, 1]] })), None).unwrap();
        assert_eq!((matched["verdict"].clone(), matched["match_percentage"].clone()), (json!("match"), json!(100.0)));
        assert!(call(&dev, &ctx, "assert_screenshot", &args(json!({ "path": "home", "threshold_percentage": 101 })), None).unwrap_err().usage);
        assert!(call(&dev, &Ctx::default(), "assert_screenshot", &args(json!({ "path": "home" })), None).unwrap_err().usage, "no goldens, no comparison");
    }

    #[test]
    fn given_a_partly_clipped_element_when_its_share_on_screen_is_read_then_it_is_its_visible_part_over_its_box() {
        let r = |left, top, right, bottom| Rect { left, top, right, bottom };
        let el = |bounds, full| Element { bounds, full, text: None, frame: "app" };
        assert_eq!(visible_pct(&el(r(0, 50, 100, 100), r(0, 0, 100, 100))), 50.0);
        assert_eq!(visible_pct(&el(r(10, 20, 110, 60), r(10, 20, 110, 60))), 100.0);
        assert_eq!(visible_pct(&el(r(0, 0, 0, 0), r(0, 0, 0, 0))), 0.0);
    }

    #[test]
    fn given_a_stub_when_its_spec_is_built_then_globs_and_methods_read_as_the_drivers_take_them() {
        let spec = stub_spec(&args(json!({ "method": "post", "url_pattern": "**/api/plan", "status": 201, "json_body": { "a": 1 }, "delay": 50, "optional": false })));
        assert_eq!(spec, json!({ "status": 201, "json_body": { "a": 1 }, "delay": 50, "pattern": "*/api/plan", "method": "POST" }));
        assert!(stub_spec(&args(json!({ "method": "*", "url_pattern": "x" }))).get("method").is_none());
    }
}
