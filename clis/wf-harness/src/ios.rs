use harness_core::device::{cli, Device, Element, Key, Level, Logpoint, Part, Rect, Sel};
use harness_core::platform::{NoScope, Platform, RunScope};
use harness_core::run_report::Facts;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdout, Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};
use wf_kit::args::Parsed;

struct Bridge {
    child: Child,
    responses: BufReader<ChildStdout>,
}
struct Simulator {
    udid: String,
    bridge: RefCell<Option<Bridge>>,
    recording: RefCell<Option<(Child, String)>>,
}

fn unsupported<T>(what: &str) -> Result<T, String> {
    Err(format!("{what} is unavailable on the iOS simulator"))
}

impl Simulator {
    fn new(udid: String) -> Self {
        Self {
            udid,
            bridge: RefCell::new(None),
            recording: RefCell::new(None),
        }
    }

    fn call(&self, request: Value) -> Result<Value, String> {
        let mut bridge = self.bridge.borrow_mut();
        if bridge.is_none() {
            let script = wf_kit::root::repo_root().join("clis/wf-harness/src/ios/bridge.py");
            let python = std::env::var("WF_IDB_PYTHON").unwrap_or_else(|_| "python3".into());
            let mut child = Command::new(python)
                .arg(script)
                .arg(&self.udid)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .map_err(|e| format!("cannot start iOS bridge: {e}"))?;
            let responses = BufReader::new(child.stdout.take().ok_or("missing bridge output")?);
            *bridge = Some(Bridge { child, responses });
        }
        let bridge = bridge.as_mut().unwrap();
        writeln!(
            bridge.child.stdin.as_mut().ok_or("missing bridge input")?,
            "{request}"
        )
        .map_err(|e| e.to_string())?;
        let mut line = String::new();
        bridge
            .responses
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let response: Value =
            serde_json::from_str(&line).map_err(|e| format!("iOS bridge response: {e}"))?;
        match response["error"].as_str() {
            Some(error) => Err(error.into()),
            None => Ok(response["value"].clone()),
        }
    }

    fn elements(&self, sel: &Sel) -> Result<Vec<Element>, String> {
        if sel.css.is_some() {
            return unsupported("CSS lookup");
        }
        let tree = self.hierarchy()?;
        let screen = self.call(json!(["screen"]))?;
        let mut found = Vec::new();
        for row in tree.as_array().ok_or("invalid accessibility tree")? {
            let text = |key: &str| row[key].as_str().unwrap_or("");
            if sel
                .id
                .as_ref()
                .is_some_and(|p| !p.is_match(text("AXUniqueId")))
                || sel.text.as_ref().is_some_and(|p| {
                    !["AXLabel", "AXValue", "title"]
                        .iter()
                        .any(|k| p.is_match(text(k)))
                })
            {
                continue;
            }
            let frame = &row["frame"];
            let number = |key: &str| frame[key].as_f64().unwrap_or_default();
            let full = Rect {
                left: number("x").round() as i64,
                top: number("y").round() as i64,
                right: (number("x") + number("width")).round() as i64,
                bottom: (number("y") + number("height")).round() as i64,
            };
            let bounds = Rect {
                left: full.left.max(0),
                top: full.top.max(0),
                right: full
                    .right
                    .min(screen["width"].as_i64().ok_or("missing width")?),
                bottom: full
                    .bottom
                    .min(screen["height"].as_i64().ok_or("missing height")?),
            };
            if bounds.left < bounds.right && bounds.top < bounds.bottom {
                let value = ["AXValue", "AXLabel", "title"]
                    .iter()
                    .find_map(|k| row[k].as_str().filter(|s| !s.is_empty()));
                found.push(Element {
                    bounds,
                    full,
                    text: value.map(str::to_string),
                    frame: "native",
                });
            }
        }
        Ok(found)
    }
}

impl Device for Simulator {
    fn platform(&self) -> &'static str {
        "ios"
    }
    fn relaunch_link(&self, link: &str) -> Result<(), String> {
        self.call(json!(["stop"]))?;
        self.open_link(link)
    }
    fn open_link(&self, link: &str) -> Result<(), String> {
        self.call(json!(["open", link])).map(drop)
    }
    fn hierarchy(&self) -> Result<Value, String> {
        self.call(json!(["tree"]))
    }
    fn tap_at(&self, x: f64, y: f64) -> Result<(), String> {
        self.call(json!(["tap", x, y])).map(drop)
    }
    fn tap_element(&self, el: &Element, _: &Sel) -> Result<(f64, f64), String> {
        let p = (
            (el.bounds.left + el.bounds.right) as f64 / 2.,
            (el.bounds.top + el.bounds.bottom) as f64 / 2.,
        );
        self.tap_at(p.0, p.1)?;
        Ok(p)
    }
    fn swipe(&self, from: (f64, f64), to: (f64, f64), ms: u64) -> Result<(), String> {
        self.call(json!(["swipe", from, to, ms])).map(drop)
    }
    fn input_text(&self, text: &str) -> Result<(), String> {
        self.call(json!(["text", text])).map(drop)
    }
    fn press_key(&self, key: Key) -> Result<(), String> {
        let code = match key {
            Key::Enter => 40,
            Key::Backspace => 42,
            Key::Tab => 43,
            Key::Left => 80,
            Key::Right => 79,
            Key::Up => 82,
            Key::Down => 81,
            Key::SelectAll => return self.call(json!(["select-all"])).map(drop),
            Key::CtrlEnd => return self.call(json!(["end"])).map(drop),
        };
        self.call(json!(["key", code])).map(drop)
    }
    fn back(&self) -> Result<(), String> {
        unsupported("system Back")
    }
    fn get_element(&self, sel: &Sel) -> Result<Option<Element>, String> {
        Ok(self.elements(sel)?.into_iter().next())
    }
    fn count_matches(&self, sel: &Sel) -> Result<usize, String> {
        Ok(self.elements(sel)?.len())
    }
    fn wait_visibility(&self, sel: &Sel, want: bool, ms: u64) -> Result<bool, String> {
        let start = Instant::now();
        loop {
            harness_core::run_stop::check()?;
            if self.get_element(sel)?.is_some() == want {
                return Ok(true);
            }
            if start.elapsed() >= Duration::from_millis(ms) {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    fn get_device_state(&self, parts: &[Part]) -> Result<Value, String> {
        let mut result = json!({});
        for part in parts {
            result[part.name()] = match part {
                Part::Screen => self.call(json!(["screen"]))?,
                Part::Keyboard => {
                    let shown = self.call(json!(["keyboard"]))?;
                    json!({"visible":shown,"bounds":null})
                }
                Part::Microphone => Value::Null,
            };
        }
        Ok(result)
    }
    fn take_screenshot(&self, path: &str) -> Result<(u32, u32), String> {
        self.call(json!(["screenshot", path]))?;
        harness_core::device::png_size(path)
    }
    fn start_recording(&self, path: &str) -> Result<(), String> {
        let child = Command::new("xcrun")
            .args([
                "simctl",
                "io",
                &self.udid,
                "recordVideo",
                "--codec=h264",
                path,
            ])
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        *self.recording.borrow_mut() = Some((child, path.into()));
        Ok(())
    }
    fn stop_recording(&self) -> Result<Vec<String>, String> {
        let (mut child, path) = self
            .recording
            .borrow_mut()
            .take()
            .ok_or("no simulator recording")?;
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .map_err(|e| e.to_string())?;
        child.wait().map_err(|e| e.to_string())?;
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() == 0 {
            return Err("empty simulator recording".into());
        }
        Ok(vec![path])
    }
    fn draw_strokes_on(
        &self,
        el: &Element,
        _: &Sel,
        source: &str,
        _: bool,
    ) -> Result<Value, String> {
        if source == "template" {
            let screenshot = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
            let path = screenshot
                .path()
                .to_str()
                .ok_or("invalid screenshot path")?;
            self.take_screenshot(path)?;
            let png = std::fs::read(path).map_err(|e| e.to_string())?;
            let screen = self.call(json!(["screen"]))?;
            let bounds = el.bounds;
            let left = bounds.left as f64;
            let top = bounds.top as f64;
            let strokes = harness_core::trace::template_strokes_in_viewport(
                &png,
                screen["width"].as_f64().ok_or("missing screen width")?,
                [
                    left,
                    top,
                    (bounds.right - bounds.left) as f64,
                    (bounds.bottom - bounds.top) as f64,
                ],
            )?;
            if strokes.is_empty() {
                return Err("no writing template is visible".into());
            }
            for stroke in &strokes {
                let points: Vec<_> = stroke.iter().map(|(x, y)| [left + x, top + y]).collect();
                self.call(json!(["stroke", points]))?;
            }
            return Ok(json!({"strokes": strokes.len()}));
        }
        let paths: Value = serde_json::from_str(el.text.as_deref().unwrap_or(""))
            .map_err(|_| "missing simulator tracing geometry")?;
        let strokes = paths[source].as_array().ok_or("missing guide strokes")?;
        if strokes.is_empty() {
            return Err("empty guide".into());
        }
        for stroke in strokes {
            let points = stroke
                .as_array()
                .ok_or("invalid guide")?
                .iter()
                .map(|p| {
                    Ok([
                        el.full.left as f64 + p[0].as_f64().or_else(|| p["x"].as_f64()).ok_or("missing x")?,
                        el.full.top as f64 + p[1].as_f64().or_else(|| p["y"].as_f64()).ok_or("missing y")?,
                    ])
                })
                .collect::<Result<Vec<_>, String>>()?;
            self.call(json!(["stroke", points]))?;
        }
        Ok(json!({"strokes":strokes.len()}))
    }
    fn get_logs(&self, _: Level) -> Result<Vec<String>, String> {
        unsupported("app log capture")
    }
    fn clear_logs(&self) -> Result<usize, String> {
        unsupported("app log capture")
    }
    fn set_microphone_input(&self, _: &str) -> Result<Value, String> {
        unsupported("microphone input")
    }
    fn run_shell(&self, _: &str) -> Result<(String, i32), String> {
        unsupported("device shell")
    }
    fn add_network_stub(&self, _: &Value) -> Result<Value, String> {
        unsupported("network stubs")
    }
    fn get_network_stub_hits(&self) -> Result<Vec<Value>, String> {
        unsupported("network stubs")
    }
    fn get_debug_hits(&self) -> Result<Vec<Value>, String> {
        unsupported("debugger")
    }
    fn arm_logpoints(&self, _: &[Logpoint]) -> Result<(), String> {
        unsupported("logpoints")
    }
    fn shutdown(&self) {
        if let Some(mut b) = self.bridge.borrow_mut().take() {
            drop(b.child.stdin.take());
            let _ = b.child.wait();
        }
    }
}

struct Ios;
impl Platform for Ios {
    fn name(&self) -> &'static str {
        "ios"
    }
    fn usage_options(&self) -> (&'static str, &'static str) {
        ("", "")
    }
    fn check_options(&self, p: &Parsed) -> Result<(), String> {
        p.val("device")
            .filter(|s| !s.is_empty())
            .map(|_| ())
            .ok_or("select the iOS simulator using --device <UDID>".into())
    }
    fn begin_run(&self, _: &str, _: i64, _: &Parsed) -> Box<dyn RunScope> {
        Box::new(NoScope)
    }
    fn open_run_device(&self, serial: Option<String>, _: String, f: &mut Facts) -> Rc<dyn Device> {
        f.device = serial.clone();
        Rc::new(Simulator::new(serial.unwrap_or_default()))
    }
    fn refuse_option(&self, p: &Parsed) -> Option<String> {
        (!p.values("logpoint").is_empty()).then(|| "logpoints require the browser".into())
    }
    fn before_script(&self, _: &dyn Device, _: &Parsed, _: &mut Facts) {}
    fn pick(&self, given: Option<&str>) -> Result<(Rc<dyn Device>, String), (i32, String)> {
        let udid = given
            .filter(|s| !s.is_empty())
            .ok_or((2, "select a simulator with --device <UDID>".into()))?;
        Ok((Rc::new(Simulator::new(udid.into())), udid.into()))
    }
}
pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    if verb == "test" {
        harness_core::run::test_for(&Ios, rest)
    } else {
        Some(cli::run(&Ios, verb, rest))
    }
}
