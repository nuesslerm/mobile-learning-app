//! The Android driver, in process: every command is typed work in `android_input` (adb, and the on-device UI server of
//! `android_u2`) against the claimed or `--device` serial. No `wf android` child runs; the adb and UI-server calls are inner
//! spans of the command that makes them.
use harness_core::device::{Device, Element, Key, Level, Logpoint, Part, Rect, Sel};
use crate::android_input::{self as input, Node};
use harness_core::trace::Select;
use serde_json::{json, Value};
use std::cell::RefCell;

pub(crate) struct AndroidDevice {
    pub(crate) serial: Option<String>,
    /// Where input commands append their lines, unless `WF_ANDROID_INPUT_LOG` names another file.
    pub(crate) input_log: Option<String>,
    /// The stub server, started by the first stub, with the device's DNAT rules pointing at it.
    pub(crate) stubs: RefCell<Option<crate::stub_server::StubServer>>,
    /// The UI server's match behind the last element looked up, so a tap on it lands on that match.
    last: RefCell<Option<Seen>>,
    /// The path this process started a recording with; another process's stop reads the device's note instead.
    recording: RefCell<Option<String>>,
}

struct Seen {
    sel: String,
    bounds: Rect,
    at: (usize, usize),
}

/// Every rule a run adds carries this comment, so a run removes its own and any a killed run left behind.
const STUB_RULE: &str = "wf-stub";
const CLEAR_STUB_RULES: &str = "for t in nat filter; do iptables -t $t -S OUTPUT | grep -- '--comment wf-stub' | sed 's/^-A /-D /' | while read -r r; do eval iptables -t $t $r; done; done; true";

/// A path a later call in another directory still finds.
fn absolute(path: &str) -> String {
    std::path::absolute(path).map_or_else(|_| path.to_string(), |p| p.display().to_string())
}

fn rect(b: [i64; 4]) -> Rect {
    Rect { left: b[0], top: b[1], right: b[2], bottom: b[3] }
}

fn level_name(c: char) -> &'static str {
    match c {
        'V' => "verbose",
        'D' => "debug",
        'I' => "info",
        'W' => "warn",
        _ => "error",
    }
}

/// A logcat line as the browser's console lines read: `W/ReactNativeJS( 1234): boom` is `warn: boom`.
fn log_line(l: &str) -> String {
    let level = level_name(l.chars().next().unwrap_or('E'));
    let message = l.split_once("): ").or_else(|| l.split_once(": ")).map_or(l, |(_, m)| m);
    format!("{level}: {message}")
}

impl AndroidDevice {
    pub(crate) fn new(serial: Option<String>, input_log: Option<String>) -> Self {
        AndroidDevice { serial, input_log, stubs: RefCell::default(), last: RefCell::default(), recording: RefCell::default() }
    }

    fn serial(&self) -> Result<String, String> {
        crate::android::serial_or_env(self.serial.as_deref()).ok_or_else(|| "no Android device given: pass --device or set ANDROID_SERIAL (the one device starts with wf android start-device)".to_string())
    }

    fn log(&self) -> Option<String> {
        std::env::var("WF_ANDROID_INPUT_LOG").ok().filter(|p| !p.is_empty()).or_else(|| self.input_log.clone())
    }

    fn element(&self, sel: &Sel, n: Node) -> Element {
        let bounds = rect(n.visible);
        *self.last.borrow_mut() = n.at.map(|at| Seen { sel: sel.show(), bounds, at });
        Element { bounds, full: rect(n.bounds), text: n.value(), frame: if n.class.contains("WebView") { "editor" } else { "native" } }
    }

    /// The element a look found; react-native-webview's wrapper reads as the field drawn inside it, the only host a field is
    /// drawn in without being its child.
    fn found(&self, serial: &str, sel: &Sel, mut n: Node) -> Result<Element, String> {
        if n.value().is_none() && (n.class.ends_with("FrameLayout") || n.class.contains("WebView")) {
            n.inner = input::field_inside(serial, n.bounds)?;
        }
        Ok(self.element(sel, n))
    }

    /// The bounds are a second read, best-effort: a keyboard that is up stays up when its frame cannot be read.
    fn keyboard(&self, serial: &str) -> Result<Value, String> {
        let visible = input::keyboard_shown(serial)?;
        let bounds = if visible { input::keyboard_bounds(serial).ok().flatten() } else { None };
        Ok(json!({ "visible": visible, "bounds": bounds }))
    }

    /// Starts the stub server and sends the app's backend ports (`WF_ANDROID_STUB_PORTS`, default 8787 for the agent and 8789
    /// for the leetcode backend) to it: a DNAT rule per port to the device's gateway, the host the run lives on, and a reset
    /// for any connection to those ports that predates the rule.
    fn start_stubs(&self) -> Result<crate::stub_server::StubServer, String> {
        let server = crate::stub_server::StubServer::start("0.0.0.0:0")?;
        let serial = self.serial()?;
        input::ensure_root(&serial);
        let routes = input::adb_text(&serial, &["shell", "ip route show table all"]).map_err(|e| format!("stub: {e}"))?;
        let gateway = stub_target(&routes, std::env::var("WF_ANDROID_STUB_HOST").ok().as_deref())?;
        let ports = std::env::var("WF_ANDROID_STUB_PORTS").unwrap_or_else(|_| "8787,8789".into());
        let mut script = CLEAR_STUB_RULES.to_string();
        for port in ports.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            script.push_str(&format!(" && iptables -t nat -I OUTPUT -p tcp --dport {port} -m comment --comment {STUB_RULE} -j DNAT --to-destination {gateway}:{}", server.port));
            // a reused app keeps keep-alive connections opened before the DNAT rule, which NAT never revisits: reset them so
            // the app's HTTP client reconnects through the stub
            script.push_str(&format!(" && iptables -I OUTPUT -p tcp -m conntrack --ctorigdstport {port} ! --ctstate DNAT -m comment --comment {STUB_RULE} -j REJECT --reject-with tcp-reset"));
        }
        script.push_str(" && echo wf-stub-rules-ok");
        // a rule that fails part-way must not stay behind: the backend ports would point at a stub that is gone
        let added = input::adb_text(&serial, &["shell", &script]).map_err(|e| format!("stub: {e}"));
        let refused = match &added {
            Ok(o) if o.contains("wf-stub-rules-ok") => return Ok(server),
            Ok(o) => format!("stub: the device refused the DNAT rules: {}", o.trim()),
            Err(e) => e.clone(),
        };
        let _ = input::adb_run(&serial, &["shell", CLEAR_STUB_RULES]);
        Err(refused)
    }
}

impl Device for AndroidDevice {
    fn platform(&self) -> &'static str {
        "android"
    }

    fn guard(&self) -> Result<(), String> {
        crate::android_lease::check()
    }

    fn open_link(&self, link: &str) -> Result<(), String> {
        input::open_link(&self.serial()?, &harness_core::android_logs::link_with_run_id(link))
    }

    fn tap_at(&self, x: f64, y: f64) -> Result<(), String> {
        input::tap_point(&self.serial()?, x, y, self.log().as_deref())
    }

    fn tap_element(&self, el: &Element, sel: &Sel) -> Result<(f64, f64), String> {
        let b = el.bounds;
        let point = ((b.left + b.right) as f64 / 2.0, (b.top + b.bottom) as f64 / 2.0);
        let at = self.last.borrow().as_ref().filter(|s| s.sel == sel.show() && s.bounds == b).map(|s| s.at);
        input::tap_node(&self.serial()?, sel, at, point, self.log().as_deref())?;
        Ok(point)
    }

    fn swipe(&self, from: (f64, f64), to: (f64, f64), duration_ms: u64) -> Result<(), String> {
        input::swipe(&self.serial()?, from, to, Some(duration_ms), self.log().as_deref())
    }

    fn input_text(&self, text: &str) -> Result<(), String> {
        input::type_text(&self.serial()?, text, self.log().as_deref())
    }

    fn press_key(&self, key: Key) -> Result<(), String> {
        let serial = self.serial()?;
        if key == Key::CtrlEnd {
            return input::press_combination(&serial, &[113, 123], "ctrl-end", self.log().as_deref());
        }
        input::press_keys(&serial, &[key.short().to_string()], self.log().as_deref())
    }

    fn back(&self) -> Result<(), String> {
        input::press_keys(&self.serial()?, &["back".to_string()], self.log().as_deref())
    }

    fn draw_strokes_on(&self, el: &Element, sel: &Sel, source: &str, human: bool) -> Result<Value, String> {
        let select = match source {
            "guide" => Select::Guide,
            "template" => Select::Template,
            other => return Err(format!("draw_strokes_on: unknown source '{other}' (guide, template)")),
        };
        let serial = self.serial()?;
        let b = el.bounds;
        let strokes = input::read_strokes(&serial, [b.left, b.top, b.right, b.bottom], select, &format!("{} (source {source})", sel.show()))?;
        let points = input::draw_strokes(&serial, &strokes, human, self.log().as_deref())?;
        Ok(json!({ "strokes": strokes.len(), "points": points }))
    }

    fn set_microphone_input(&self, path: &str) -> Result<Value, String> {
        let bytes = std::fs::metadata(path).map_err(|e| format!("cannot read {path}: {e}"))?.len();
        input::set_microphone(&self.serial()?, &absolute(path))?;
        Ok(json!({ "path": path, "bytes": bytes }))
    }

    fn run_shell(&self, cmd: &str) -> Result<(String, i32), String> {
        input::adb_shell(&self.serial()?, cmd)
    }

    fn take_screenshot(&self, path: &str) -> Result<(u32, u32), String> {
        let png = input::screenshot(&self.serial()?)?;
        if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, &png).map_err(|e| format!("cannot write {path}: {e}"))?;
        harness_core::device::png_size(path)
    }

    /// adbd restarting as root (set_microphone_input) kills every shell it started, the recorder's loop among them (G1 lost
    /// its recording at e8589e17e): going root before the recorder starts makes that later call a no-op.
    fn start_recording(&self, path: &str) -> Result<(), String> {
        let serial = self.serial()?;
        input::ensure_root(&serial);
        input::record_start(&serial, Some(&absolute(path)))?;
        *self.recording.borrow_mut() = Some(path.to_string());
        Ok(())
    }

    fn stop_recording(&self) -> Result<Vec<String>, String> {
        let serial = self.serial()?;
        let out = match self.recording.borrow_mut().take() {
            Some(p) => p,
            None => input::recording(&serial)?.filter(|n| !n.is_empty()).unwrap_or_else(|| "android-record.mp4".into()),
        };
        input::record_stop(&serial, &out)
    }

    fn get_element(&self, sel: &Sel) -> Result<Option<Element>, String> {
        if sel.point.is_some() {
            return Err("a point is not an element: look one up by id or text".into());
        }
        let serial = self.serial()?;
        let Some(n) = input::locate(&serial, sel)? else { return Ok(None) };
        self.found(&serial, sel, n).map(Some)
    }

    fn keyboard_visible(&self) -> Result<bool, String> {
        input::keyboard_shown(&self.serial()?)
    }

    fn count_matches(&self, sel: &Sel) -> Result<usize, String> {
        input::count(&self.serial()?, sel)
    }

    fn get_device_state(&self, parts: &[Part]) -> Result<Value, String> {
        let serial = self.serial()?;
        let mut out = serde_json::Map::new();
        for part in parts {
            let v = match part {
                Part::Screen => {
                    let (w, h) = input::screen_size(&serial)?;
                    json!({ "width": w, "height": h, "dpr": 1 })
                }
                Part::Keyboard => self.keyboard(&serial)?,
                Part::Microphone => input::microphone(&serial)?,
            };
            out.insert(part.name().into(), v);
        }
        Ok(Value::Object(out))
    }

    fn get_logs(&self, level: Level) -> Result<Vec<String>, String> {
        let min = match level {
            Level::Debug => 'D',
            Level::Info => 'I',
            Level::Warn => 'W',
            Level::Error => 'E',
        };
        Ok(input::js_log(&self.serial()?, min)?.iter().map(|l| log_line(l)).collect())
    }

    fn clear_logs(&self) -> Result<usize, String> {
        input::js_log_clear(&self.serial()?)
    }

    fn hierarchy(&self) -> Result<Value, String> {
        input::hierarchy(&self.serial()?)
    }

    fn wait_visibility(&self, sel: &Sel, want: bool, ms: u64) -> Result<bool, String> {
        Ok(input::wait_visible(&self.serial()?, sel, want, std::time::Duration::from_millis(ms))?.is_some())
    }

    fn wait_shown(&self, sel: &Sel, ms: u64) -> Result<Option<Option<Element>>, String> {
        let serial = self.serial()?;
        let seen = input::wait_visible(&serial, sel, true, std::time::Duration::from_millis(ms))?;
        seen.map(|n| n.map(|n| self.found(&serial, sel, n)).transpose()).transpose()
    }

    fn add_network_stub(&self, spec: &Value) -> Result<Value, String> {
        let stub = crate::stub_server::stub_from_spec(spec)?;
        let mut slot = self.stubs.borrow_mut();
        if slot.is_none() {
            *slot = Some(self.start_stubs()?);
        }
        let server = slot.as_ref().expect("started");
        let summary = json!({ "pattern": stub.pattern, "method": stub.method, "status": stub.status, "bytes": stub.body.len(), "port": server.port });
        server.add(stub);
        Ok(summary)
    }

    fn get_network_stub_hits(&self) -> Result<Vec<Value>, String> {
        Ok(self.stubs.borrow().as_ref().map(|s| s.hits()).unwrap_or_default())
    }

    fn get_debug_hits(&self) -> Result<Vec<Value>, String> {
        Ok(Vec::new())
    }

    fn arm_logpoints(&self, _: &[Logpoint]) -> Result<(), String> {
        Err("logpoints are browser only: an Android run loads the EAS Update, which has no inspector".into())
    }

    fn stub_routing(&self) -> Option<Value> {
        self.stubs.borrow().as_ref()?;
        let o = input::adb_text(&self.serial().ok()?, &["shell", "iptables -t nat -L OUTPUT -v -n -x; iptables -L OUTPUT -v -n -x"]).ok()?;
        let rules: Vec<&str> = o.lines().filter(|l| l.contains(STUB_RULE)).map(str::trim).collect();
        Some(json!({ "rules": rules }))
    }

    fn shutdown(&self) {
        if self.stubs.borrow().is_some() {
            if let Err(e) = self.serial().and_then(|s| input::adb_run(&s, &["shell", CLEAR_STUB_RULES])) {
                wf_kit::wf_err!("wf: the device kept its stub rules: {e}");
            }
        }
    }
}

/// `wf android`: the platform the run engine and the verb front end name nothing else of.
pub(crate) struct AndroidPlatform;

pub(crate) static PLATFORM: AndroidPlatform = AndroidPlatform;

struct AndroidRun {
    run_id: String,
    _lease: Option<crate::android_lease::Renewal>,
}

impl harness_core::platform::RunScope for AndroidRun {
    fn finish(self: Box<Self>, p: &wf_kit::args::Parsed, f: &mut harness_core::run_report::Facts) {
        let group = p.values("env").iter().find_map(|kv| kv.strip_prefix("base=").and_then(harness_core::android_logs::group_of));
        f.logs = Some(harness_core::android_logs::collect(&self.run_id, group.as_deref(), "android"));
    }
}

impl harness_core::platform::Platform for AndroidPlatform {
    fn name(&self) -> &'static str {
        "android"
    }

    fn usage_options(&self) -> (&'static str, &'static str) {
        ("", " [--cold-start]")
    }

    fn check_options(&self, p: &wf_kit::args::Parsed) -> Result<(), String> {
        match p.val("screen-size").or(p.val("frame")) {
            Some(_) => Err("--screen-size and --frame are browser options: an Android screen is set when the device starts".into()),
            None => Ok(()),
        }
    }

    fn begin_run(&self, stem: &str, started_ms: i64, _: &wf_kit::args::Parsed) -> Box<dyn harness_core::platform::RunScope> {
        let run_id = harness_core::android_logs::run_id_for(stem, started_ms);
        harness_core::android_logs::start_run(&run_id);
        Box::new(AndroidRun { run_id, _lease: crate::android_lease::renew_while_running() })
    }

    fn open_run_device(&self, serial: Option<String>, inputs: String, f: &mut harness_core::run_report::Facts) -> std::rc::Rc<dyn Device> {
        f.device = crate::android::serial_or_env(serial.as_deref());
        f.inputs = Some(std::path::PathBuf::from(std::env::var("WF_ANDROID_INPUT_LOG").unwrap_or_else(|_| inputs.clone())));
        let device = AndroidDevice::new(serial, Some(inputs));
        // the backend names are mapped to the host (WF_ANDROID_HOSTS): the stub server must listen there even when the script stubs nothing, and forwards what it has no stub for
        if std::env::var("WF_ANDROID_HOSTS").is_ok_and(|h| !h.trim().is_empty()) {
            match device.start_stubs() {
                Ok(server) => *device.stubs.borrow_mut() = Some(server),
                Err(e) => wf_kit::wf_err!("wf: the forwarding server for the mapped backends did not start: {e}"),
            }
        }
        std::rc::Rc::new(device)
    }

    fn refuse_option(&self, p: &wf_kit::args::Parsed) -> Option<String> {
        (!p.values("logpoint").is_empty()).then(|| "--logpoint is browser only: an Android run loads the EAS Update, which has no inspector".into())
    }

    fn before_script(&self, device: &dyn Device, p: &wf_kit::args::Parsed, f: &mut harness_core::run_report::Facts) {
        f.density = device.run_shell("wm density").ok().and_then(|(out, _)| harness_core::run_report::parse_density(&out));
        // the app already running is reused unless the run asks for a cold start (`wf test run` starts it cold once per job)
        if p.flag("cold-start") {
            f.app_start = Some("cold");
            let stopped = device.run_shell("am force-stop host.exp.exponent").and_then(|(out, code)| if code == 0 { Ok(()) } else { Err(format!("exit {code}: {}", out.trim())) });
            if let Err(e) = stopped {
                wf_kit::wf_err!("wf: cannot force-stop Expo Go before the script: {e}");
                f.notes.push(format!("cannot force-stop Expo Go before the script: {e}"));
            }
        } else {
            f.app_start = Some("reused");
        }
    }

    fn pick(&self, given: Option<&str>) -> Result<(std::rc::Rc<dyn Device>, String), (i32, String)> {
        let serial = android_serial(given)?;
        Ok((std::rc::Rc::new(AndroidDevice::new(Some(serial.clone()), None)), serial))
    }
}

fn adb_devices() -> Vec<String> {
    let Ok(o) = std::process::Command::new("adb").arg("devices").stdin(std::process::Stdio::null()).output() else { return vec![] };
    String::from_utf8_lossy(&o.stdout).lines().skip(1).filter_map(|l| l.split_once('\t')).filter(|(_, state)| state.trim() == "device").map(|(s, _)| s.to_string()).collect()
}

/// The adb serial a verb drives: `--device`, else ANDROID_SERIAL or WF_ANDROID_SERIAL, else the only device adb lists.
/// Exit 2 when several are attached, 1 when none is.
pub(crate) fn android_serial(given: Option<&str>) -> Result<String, (i32, String)> {
    match crate::android::serial_or_env(given) {
        Some(s) => Ok(s),
        None => harness_core::device::cli::one(adb_devices(), "no Android device: pass --device or set ANDROID_SERIAL (the one device starts with wf android start-device)"),
    }
}


/// The address the device's DNAT rules point at: the device's default gateway, the host the run lives on when the run shares the
/// device's network, else `WF_ANDROID_STUB_HOST` (a name or an address) when the run lives on another node (CI's runner has its own
/// tailnet node, so the stub server is reached by the runner's tailnet address).
fn stub_target(routes: &str, host: Option<&str>) -> Result<String, String> {
    if let Some(host) = host.map(str::trim).filter(|h| !h.is_empty()) {
        if host.parse::<std::net::IpAddr>().is_ok() {
            return Ok(host.to_string());
        }
        return std::net::ToSocketAddrs::to_socket_addrs(&(host, 0))
            .map_err(|e| format!("stub: WF_ANDROID_STUB_HOST {host} does not resolve: {e}"))?
            .find(|a| a.is_ipv4())
            .map(|a| a.ip().to_string())
            .ok_or_else(|| format!("stub: WF_ANDROID_STUB_HOST {host} has no IPv4 address"));
    }
    routes
        .lines()
        .find_map(|l| l.split_whitespace().skip_while(|w| *w != "via").nth(1))
        .map(str::to_string)
        .ok_or_else(|| format!("stub: the device has no default route: {}", routes.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_logcat_lines_when_read_as_logs_then_each_reads_as_its_level_and_message() {
        assert_eq!(log_line("W/ReactNativeJS( 1234): Response.blob() is slow"), "warn: Response.blob() is slow");
        assert_eq!(log_line("E/ReactNativeJS(  12): boom: at x"), "error: boom: at x");
        assert_eq!(log_line("I/ReactNativeJS: fine"), "info: fine");
    }

    #[test]
    fn given_no_stub_host_when_the_target_is_chosen_then_it_is_the_devices_default_gateway() {
        let routes = "default via 172.17.0.1 dev eth0 \n172.17.0.0/16 dev eth0 scope link";

        assert_eq!(stub_target(routes, None).as_deref(), Ok("172.17.0.1"));
        assert_eq!(stub_target(routes, Some("  ")).as_deref(), Ok("172.17.0.1"));
    }

    #[test]
    fn given_a_stub_host_when_the_target_is_chosen_then_it_wins_over_the_gateway_and_a_host_that_does_not_resolve_is_an_error() {
        let routes = "default via 172.17.0.1 dev eth0";

        assert_eq!(stub_target(routes, Some("100.64.0.9")).as_deref(), Ok("100.64.0.9"));
        assert!(stub_target(routes, Some("no-such-host.invalid")).unwrap_err().contains("WF_ANDROID_STUB_HOST"));
    }

    #[test]
    fn given_no_default_route_and_no_stub_host_when_the_target_is_chosen_then_it_is_an_error_naming_the_routes() {
        assert!(stub_target("10.0.0.0/8 dev eth0", None).unwrap_err().contains("no default route"));
    }
}
