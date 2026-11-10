//! Launched device-frame sessions for the Rust browser daemon (`open` without `--cdp-endpoint`): Chromium started with the
//! device-emulation flags, the context options as CDP overrides, the page bindings and init scripts, the harness page
//! (`/_mcp/device-frame`) with its `__shell` bridge, the mock keyboard, and the safe-area and identity overrides the
//! Node daemon applies after every navigation. Not reproduced: video, the audio file, the tailnet proxy, platform fonts.
use cdp::attached::{device_frame_url, fitted_shell_size, Attached};
use cdp::launch::{find_chromium, launch, Launched};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const SPECS: &str = include_str!("browser_device_specs.json");
const KEYBOARD_HEIGHT: i64 = 270;
/// How long the device frame's shell, and a new document in its app frame, may take to arrive before a navigation fails.
const NAVIGATION_WAIT: Duration = Duration::from_secs(30);
/// How long a navigation waits for an arrived app document to finish loading; one still loading then is left to load.
const LOAD_WAIT: Duration = Duration::from_secs(10);
const BINDING_WRAPPER: &str = "(() => { for (const n of ['__mcpOpenedUrl', '__mcpFocusChange']) { const raw = window[n]; if (typeof raw !== 'function') continue; window[n] = (p) => raw(typeof p === 'string' ? p : JSON.stringify(p)); } })();";
const FOCUS_SCRIPT: &str = "(() => { const isEditable = (el) => { if (!el) return false; const tag = (el.tagName || '').toLowerCase(); if (tag === 'input' || tag === 'textarea') return true; if (el.getAttribute('contenteditable') === 'true') return true; return el.getAttribute('role') === 'textbox'; }; const report = (p) => { const fn = window.__mcpFocusChange; if (typeof fn === 'function') fn(p); }; document.addEventListener('focusin', (e) => report({ type: 'in', editable: isEditable(e.target) }), true); document.addEventListener('focusout', () => report({ type: 'out' }), true); document.addEventListener('mousedown', (e) => { const t = e.target; if (t && t.closest && !t.closest('input, textarea, select, [contenteditable=\"true\"], [role=textbox]')) e.preventDefault(); }, true); })();";
const FAKE_MIC: &str = include_str!("../../cdp/src/js/fake_mic.js");
const FAKE_CAMERA: &str = include_str!("../../cdp/src/js/fake_camera.js");
const OPENED_URLS: &str = include_str!("../../cdp/src/js/opened_urls.js");

pub struct Device {
    spec: Value,
    preset: String,
    orientation: String,
    frame: String,
    pub color_scheme: String,
    emulates: bool,
    dpr: f64,
    version: String,
    shell_ready: bool,
    plain: bool,
    keyboard_mock: bool,
    auto_visible: bool,
    blur_at: Option<Instant>,
    polling: bool,
    last_poll: Instant,
    pub opened_urls: Vec<Value>,
    /// The payload of the last `__wfWaitDone` call: a `wait-observe` page script reporting its outcome.
    pub wait_done: Option<String>,
}

pub struct Session {
    pub page: Attached,
    /// The window's size in CSS pixels, what a recording is sized to.
    pub viewport: (i64, i64),
    pub launched: Launched,
    pub device: Device,
}

type Res<T> = Result<T, String>;

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn oriented_viewport(spec: &Value, orientation: &str) -> (f64, f64) {
    let (w, h) = (spec["viewport"]["width"].as_f64().unwrap_or(0.0), spec["viewport"]["height"].as_f64().unwrap_or(0.0));
    if orientation == "landscape" && spec["supportsLandscape"].as_bool() == Some(true) { (h, w) } else { (w, h) }
}

fn oriented_safe_area(spec: &Value, orientation: &str) -> Value {
    let s = &spec["safeArea"];
    if orientation == "landscape" && spec["supportsLandscape"].as_bool() == Some(true) {
        json!({ "top": s["left"], "right": s["top"], "bottom": s["right"], "left": s["bottom"] })
    } else {
        s.clone()
    }
}

fn fill_version(value: &str, major: &str) -> String {
    value.replace("{chrome}", major)
}

/// The identity of the preset with `{chrome}` replaced by the browser's major version.
fn resolve_identity(identity: &Value, version: &str) -> Value {
    let major = version.split('.').next().unwrap_or(version);
    let mut out = identity.clone();
    out["userAgent"] = json!(fill_version(identity["userAgent"].as_str().unwrap_or(""), major));
    if let Some(brands) = identity["userAgentMetadata"]["brands"].as_array() {
        out["userAgentMetadata"]["brands"] = json!(brands.iter().map(|b| json!({ "brand": b["brand"], "version": fill_version(b["version"].as_str().unwrap_or(""), major) })).collect::<Vec<_>>());
    }
    out
}

/// Opens a Chromium of our own, framed as the requested device.
/// Without the built page every device-frame open lands on the daemon's JSON 404 and the run waits out its budget.
fn frame_page_built(dist: &std::path::Path) -> Res<()> {
    if dist.join("index.html").is_file() {
        return Ok(());
    }
    Err(format!("the device-frame page is not built ({}): run `pnpm --filter @tooling/browser-harness run build`, or open with --plain", dist.display()))
}

pub fn open(args: &Value) -> Res<Session> {
    if text(args, "engine").is_some_and(|e| e != "chromium") {
        return Err("only the chromium engine is implemented by the Rust browser daemon".into());
    }
    let plain = match text(args, "renderMode") {
        None | Some("device-frame") => false,
        Some("plain") => true,
        Some(other) => return Err(format!("Unknown render mode: {other}")),
    };
    if !plain {
        frame_page_built(&crate::browser_daemon::harness_dist())?;
    }
    let preset = text(args, "devicePreset").unwrap_or("iphone-15-pro").to_string();
    let specs: Vec<Value> = serde_json::from_str(SPECS).map_err(|e| e.to_string())?;
    let spec = specs.into_iter().find(|s| s["id"] == preset).ok_or_else(|| format!("Unknown device preset: {preset}"))?;
    let orientation = text(args, "orientation").unwrap_or("portrait").to_string();
    let frame = text(args, "frame").unwrap_or("screen").to_string();
    let color_scheme = text(args, "colorScheme").unwrap_or("system").to_string();
    let keyboard_mock = !plain && text(args, "keyboardMode").unwrap_or("mock") == "mock";
    let (sw, sh) = oriented_viewport(&spec, &orientation);
    let (vw, vh) = if frame == "bezel" { ((sw + 140.0) as i64, (sh + 220.0) as i64) } else { fitted_shell_size(sw, sh) };
    let emulates = spec["platform"] != "desktop";
    let dpr = spec["dpr"].as_f64().unwrap_or(1.0);

    // a container's /dev/shm is 64 MB: a large page's renderer outgrew it on the CI runner (ERR_INSUFFICIENT_RESOURCES)
    let mut chrome_args = vec!["--autoplay-policy=no-user-gesture-required".to_string(), "--disable-dev-shm-usage".to_string()];
    if let Some(audio) = text(args, "audioFile") {
        if !std::path::Path::new(audio).exists() {
            return Err(format!("--audio-file not found: {audio}"));
        }
        if !audio.to_ascii_lowercase().ends_with(".wav") {
            return Err(format!("--audio-file must be a .wav -- Chromium's fake capturer reads PCM and falls back to a generated tone for anything else, which looks like a passing test that transcribes nothing. Got: {audio}"));
        }
        chrome_args.extend(["--use-fake-device-for-media-stream".to_string(), "--use-fake-ui-for-media-stream".to_string(), format!("--use-file-for-fake-audio-capture={audio}")]);
    }
    if emulates {
        chrome_args.extend([format!("--force-device-scale-factor={dpr}"), "--disable-site-isolation-trials".into(), "--use-fake-ui-for-media-stream".into(), "--use-fake-device-for-media-stream".into()]);
    }
    let mut proxy_credentials = None;
    if args.get("tailnetProxy").and_then(Value::as_bool) == Some(true) {
        // A raw TCP tunnel into the tailnet (tailscaled's userspace SOCKS5 listener): it wins over the general egress proxy.
        let port = std::env::var("WF_SOCKS_PORT").ok().filter(|p| !p.is_empty()).unwrap_or_else(|| "1055".into());
        chrome_args.push(format!("--proxy-server=socks5://localhost:{port}"));
    } else if let Some(proxy) = std::env::var("HTTPS_PROXY").ok().or_else(|| std::env::var("https_proxy").ok()).filter(|p| !p.is_empty()) {
        // The general egress proxy. Tailnet names have no route through it, and a post-quantum key share breaks its TLS.
        let (scheme, rest) = proxy.split_once("://").unwrap_or(("http", proxy.as_str()));
        let (userinfo, host) = rest.rsplit_once('@').map_or(("", rest), |(u, h)| (u, h));
        if let Some((u, p)) = userinfo.split_once(':') {
            proxy_credentials = Some((u.to_string(), p.to_string()));
        }
        chrome_args.extend([format!("--proxy-server={scheme}://{host}"), "--proxy-bypass-list=localhost;127.0.0.1;*.ts.net".into(), "--ssl-version-max=tls1.2".into()]);
    }
    let headless = args.get("headless").and_then(Value::as_bool).unwrap_or_else(|| std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none());
    if !headless {
        chrome_args.push(format!("--window-size={vw},{vh}"));
    }
    // Extra Chromium flags and environment from the caller (a container's `--no-sandbox`, the `DISPLAY` of its Xvfb).
    chrome_args.extend(args.get("launchArgs").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string));
    let launch_env: Vec<(String, String)> =
        args.get("launchEnv").and_then(Value::as_object).into_iter().flatten().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect();
    let exe = find_chromium().ok_or("no Chromium found (set WF_CHROMIUM or install one with Playwright)")?;
    let launched = launch(&exe, &chrome_args, &launch_env, headless)?;
    let attach = (|| -> Res<Attached> {
        let mut page = Attached::attach(&launched.endpoint)?;
        let version = page.browser_version()?;
        page.set_metrics(vw, vh, if emulates { dpr } else { 1.0 }, emulates)?;
        if emulates {
            if let Some(identity) = spec.get("identity").filter(|i| i.is_object()) {
                page.set_identity(resolve_identity(identity, &version)["userAgent"].as_str().unwrap_or(""), None, None, 1)?;
            }
        }
        if color_scheme == "light" || color_scheme == "dark" {
            page.set_color_scheme(&color_scheme)?;
        }
        page.emulate_focus()?;
        page.grant_clipboard_and_ignore_cert_errors()?;
        if let Some((u, p)) = &proxy_credentials {
            page.set_http_credentials(u, p)?;
        }
        if let Some(c) = args.get("httpCredentials").filter(|c| c.is_object()) {
            page.set_http_credentials(c["username"].as_str().unwrap_or(""), c["password"].as_str().unwrap_or(""))?;
        }
        page.add_binding("__mcpOpenedUrl")?;
        page.add_binding("__mcpSetColorScheme")?;
        page.add_binding("__wfWaitDone")?;
        if keyboard_mock {
            page.add_binding("__mcpFocusChange")?;
        }
        page.add_init_script(BINDING_WRAPPER)?;
        page.add_init_script(OPENED_URLS)?;
        page.add_init_script(FAKE_MIC)?;
        page.add_init_script(FAKE_CAMERA)?;
        if keyboard_mock {
            page.add_init_script(FOCUS_SCRIPT)?;
        }
        Ok(page)
    })();
    let page = match attach {
        Ok(p) => p,
        Err(e) => {
            launched.close();
            return Err(e);
        }
    };
    let mut page = page;
    let version = page.browser_version().unwrap_or_default();
    let device = Device {
        spec,
        preset,
        orientation,
        frame,
        color_scheme,
        emulates,
        dpr,
        version,
        shell_ready: false,
        plain,
        keyboard_mock,
        auto_visible: false,
        blur_at: None,
        polling: false,
        last_poll: Instant::now(),
        opened_urls: Vec::new(),
        wait_done: None,
    };
    Ok(Session { page, viewport: (vw, vh), launched, device })
}

impl Device {
    pub fn summary_fields(&self) -> [(&'static str, Value); 5] {
        [("renderMode", json!(if self.plain { "plain" } else { "device-frame" })), ("devicePreset", json!(self.preset)), ("orientation", json!(self.orientation)), ("keyboardMode", json!(if self.keyboard_mock { "mock" } else { "none" })), ("colorScheme", json!(self.color_scheme))]
    }
}

impl Session {
    fn emulate_device(&mut self) -> Res<()> {
        let d = &self.device;
        self.page.set_safe_area(&oriented_safe_area(&d.spec, &d.orientation))?;
        if let Some(identity) = d.spec.get("identity").filter(|i| i.is_object()) {
            let id = resolve_identity(identity, &d.version);
            self.page.set_identity(id["userAgent"].as_str().unwrap_or(""), id["platform"].as_str(), id.get("userAgentMetadata"), id["maxTouchPoints"].as_i64().unwrap_or(5))?;
        }
        Ok(())
    }

    fn fit_device_frame(&mut self) {
        let boxed = self.page.eval_page("(() => { const el = document.querySelector('.mcp-screen, .marvel-device, .mcp-custom-bezel'); if (!el) return null; const r = el.getBoundingClientRect(); return { width: r.width, height: r.height }; })()").unwrap_or(Value::Null);
        let (w, h) = (boxed["width"].as_f64().unwrap_or(0.0), boxed["height"].as_f64().unwrap_or(0.0));
        if w > 0.0 && h > 0.0 {
            let (fw, fh) = fitted_shell_size(w, h);
            let _ = self.page.set_metrics(fw, fh, if self.device.emulates { self.device.dpr } else { 1.0 }, self.device.emulates);
        }
    }

    /// The committed URL of the harness's app frame, if it exists yet.
    fn app_frame_url(&mut self) -> Option<String> {
        self.page.frames().ok()?.into_iter().find(|f| f.name == "device-frame-app").map(|f| f.url)
    }

    /// Waits until the app frame has committed a document other than `before` and finished loading, so what follows a
    /// navigation sees the page and not a blank or the previous frame. `Page.navigate` and the bridge's `setTarget` return
    /// before that. A frame that never leaves `before` is an error: the navigation did not happen.
    fn wait_app_loaded(&mut self, before: Option<String>, url: &str) -> Res<()> {
        let started = Instant::now();
        let mut arrived = before.as_deref().is_none_or(|b| b == url);
        let mut shown = "no app frame".to_string();
        while started.elapsed() < NAVIGATION_WAIT {
            if let Ok(Some(frame)) = self.page.find_frame_named("device-frame-app", Duration::from_millis(200)) {
                arrived = arrived || before.as_deref() != Some(frame.url.as_str());
                if arrived && frame.url != "about:blank" {
                    let state = self.page.eval_in_frame(&frame.id, "document.readyState").ok();
                    if started.elapsed() >= LOAD_WAIT || state.as_ref().and_then(Value::as_str) == Some("complete") {
                        return Ok(());
                    }
                }
                shown = frame.url;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!("the app frame did not leave {shown} for {url} within {} s", NAVIGATION_WAIT.as_secs()))
    }

    /// Waits (bounded) for the main frame of a plain page to finish loading.
    fn wait_page_loaded(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.page.eval_page("document.readyState").ok().as_ref().and_then(Value::as_str) == Some("complete") {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Loads `url` in the device frame: the harness page the first time (waiting for its `__shell` bridge), the bridge's
    /// `setTarget` after that; then the per-navigation device overrides. Returns once the app has loaded.
    pub fn navigate(&mut self, daemon_url: &str, url: &str) -> Res<()> {
        if self.device.plain {
            if let Err(e) = self.page.navigate(url) {
                if !e.contains("ERR_ABORTED") {
                    return Err(e);
                }
            }
            self.wait_page_loaded();
            return self.emulate_device();
        }
        if self.device.shell_ready {
            let before = self.app_frame_url();
            self.page.eval_page(&format!("window.__shell?.setTarget({})", json!(url)))?;
            self.wait_app_loaded(before, url)?;
            return self.emulate_device();
        }
        let d = &self.device;
        let harness = device_frame_url(daemon_url, url, &d.preset, &d.orientation, &d.color_scheme, &d.frame);
        if let Err(e) = self.page.navigate(&harness) {
            if !e.contains("ERR_ABORTED") {
                return Err(e);
            }
        }
        let started = Instant::now();
        while self.page.eval_page("typeof window.__shell !== 'undefined'").ok().and_then(|v| v.as_bool()) != Some(true) {
            if started.elapsed() >= NAVIGATION_WAIT {
                return Err(format!("the device frame's shell did not load within {} s", NAVIGATION_WAIT.as_secs()));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.device.shell_ready = true;
        self.fit_device_frame();
        self.emulate_device()?;
        if self.device.keyboard_mock {
            self.device.polling = true;
        }
        self.wait_app_loaded(None, url)
    }

    /// Points `eval` and what is built on it at the app frame (or the editable frame inside it), as `getAppTarget` does.
    pub fn use_app_target(&mut self, outer_only: bool) -> Res<()> {
        self.page.set_target(None);
        if !self.device.shell_ready {
            return Ok(());
        }
        let Some(outer) = self.page.find_frame_named("device-frame-app", Duration::from_secs(10))? else { return Ok(()) };
        let target = if outer_only { outer.id } else { self.page.find_editable_frame(&outer.id, 4)?.map_or(outer.id, |f| f.id) };
        self.page.set_target(Some(target));
        Ok(())
    }

    /// Switches the framed device (and optionally its orientation) without reopening. The browser's scale factor is
    /// what it launched with; the frame, the viewport, the safe areas and the identity follow the new preset.
    pub fn set_device(&mut self, preset: &str, orientation: Option<&str>) -> Res<(String, String)> {
        if self.device.plain {
            return Err("The plain render mode has no device frame to switch".into());
        }
        if !self.device.shell_ready {
            return Err("Shell not loaded, open first".into());
        }
        let specs: Vec<Value> = serde_json::from_str(SPECS).map_err(|e| e.to_string())?;
        let spec = specs.into_iter().find(|s| s["id"] == preset).ok_or_else(|| format!("Unknown device preset: {preset}"))?;
        self.device.spec = spec;
        self.device.preset = preset.to_string();
        if let Some(o) = orientation {
            self.device.orientation = o.to_string();
        }
        self.page.eval_page(&format!("window.__shell?.setDevice({}, {})", json!(self.device.preset), json!(self.device.orientation)))?;
        self.fit_device_frame();
        self.emulate_device()?;
        Ok((self.device.preset.clone(), self.device.orientation.clone()))
    }

    fn apply_keyboard(&mut self, visible: bool) {
        if !self.device.shell_ready {
            return;
        }
        let _ = self.page.eval_page(&format!("window.__shell?.toggleKeyboard({visible})"));
        let strategy = if self.device.spec["platform"] == "ios" { "overlay" } else { "resize" };
        let inset = if visible && strategy == "overlay" { KEYBOARD_HEIGHT } else { 0 };
        if let Ok(Some(app)) = self.page.find_frame_named("device-frame-app", Duration::from_millis(200)) {
            let _ = self.page.eval_in_frame(&app.id, &format!("(() => {{ window.__MCP_KEYBOARD_VISIBLE__ = {visible}; window.__MCP_KEYBOARD_MODE__ = '{strategy}'; window.__MCP_KEYBOARD_HEIGHT__ = {inset}; window.dispatchEvent(new Event('mcp:keyboardframe')); }})()"));
        }
        self.device.polling = visible;
    }

    /// Everything the page asked of the daemon since the last tick: focus changes (the mock keyboard), colour-scheme
    /// changes from the harness picker, opened URLs; the keyboard's blur debounce and key forwarding.
    pub fn tick(&mut self) {
        for call in self.page.take_binding_calls() {
            let payload = call["payload"].as_str().unwrap_or("");
            match call["name"].as_str() {
                Some("__mcpFocusChange") => {
                    let p: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
                    if p["type"] == "in" && p["editable"] == true {
                        self.device.blur_at = None;
                        if !self.device.auto_visible {
                            self.device.auto_visible = true;
                            self.apply_keyboard(true);
                        }
                    } else if p["type"] == "out" {
                        self.device.blur_at = Some(Instant::now() + Duration::from_millis(150));
                    }
                }
                Some("__mcpSetColorScheme") => {
                    let scheme = serde_json::from_str::<Value>(payload).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| payload.to_string());
                    if ["light", "dark", "system"].contains(&scheme.as_str()) {
                        self.device.color_scheme = scheme.clone();
                        let _ = self.page.set_color_scheme(&scheme);
                    }
                }
                Some("__wfWaitDone") => self.device.wait_done = Some(payload.to_string()),
                Some("__mcpOpenedUrl") => {
                    if let Ok(mut entry) = serde_json::from_str::<Value>(payload) {
                        entry["at"] = json!(wf_kit::time::now_ms());
                        self.device.opened_urls.push(entry);
                    }
                }
                _ => {}
            }
        }
        if self.device.blur_at.is_some_and(|t| Instant::now() >= t) {
            self.device.blur_at = None;
            self.device.auto_visible = false;
            self.apply_keyboard(false);
        }
        if self.device.polling && self.device.last_poll.elapsed() >= Duration::from_millis(100) {
            self.device.last_poll = Instant::now();
            let key = self.page.eval_page("(() => { const s = window.__shell; return s && typeof s.drainKeyQueue === 'function' ? s.drainKeyQueue() : null; })()").unwrap_or(Value::Null);
            if let Some(key) = key.as_str().filter(|k| !k.is_empty()) {
                let action = serde_json::from_str::<Value>(key).ok().filter(|p| p["v"].is_string()).unwrap_or_else(|| json!({ "k": "press", "v": key }));
                let value = action["v"].as_str().unwrap_or("");
                let _ = if action["k"] == "text" { self.page.insert_text(value) } else { self.page.press_key(value) };
            }
        }
    }

    /// The preset's platform (`ios`, `android` or `desktop`): which keyboard it types with.
    pub fn platform(&self) -> String {
        self.device.spec["platform"].as_str().unwrap_or("ios").to_string()
    }

    /// Whether the app runs inside the device frame's page (not a plain page of its own).
    pub fn framed(&self) -> bool {
        !self.device.plain
    }

    /// Whether taps are fingers: a phone or tablet preset (a desktop one taps with a mouse).
    pub fn touch(&self) -> bool {
        self.device.emulates
    }

    pub fn close(self) {
        self.launched.close();
    }
}

#[cfg(test)]
mod tests {
    use super::frame_page_built;

    #[test]
    fn given_no_built_frame_page_when_opening_then_it_names_the_build_command() {
        let dir = std::env::temp_dir().join(format!("wf-frame-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = frame_page_built(&dir).unwrap_err();
        assert!(err.contains("pnpm --filter @tooling/browser-harness run build"), "{err}");
        std::fs::write(dir.join("index.html"), "<html></html>").unwrap();
        assert!(frame_page_built(&dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
