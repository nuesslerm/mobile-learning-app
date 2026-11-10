//! The browser the human finishes a login in: one headed kiosk Chromium on a bare Xvfb screen, driven over CDP and never relaunched under them. There
//! is no display server, window manager or streamed desktop; the human watches JPEG frames of the page and sends taps and
//! keys back as CDP input (see `view`). Every operation is best-effort: a badly sized login is completable, no login at all
//! is not, so failures are logged and never block a session.
use super::descriptor::safe_url;
use super::log;
use cdp::attached::Attached;
use serde_json::Value;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_CDP_ENDPOINT: &str = "http://127.0.0.1:9222";
/// A real, current iOS Safari UA: login providers fingerprint it, and a string no device sends trips a bot check.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
const BROWSER_START_TIMEOUT: Duration = Duration::from_secs(30);
const COOKIE_BANNER_DELAY: Duration = Duration::from_millis(800);
const COOKIE_BANNER_ATTEMPTS: u32 = 6;
/// The phone the login is presented on: the device preset's own viewport (iPhone 15 Pro).
const VIEWPORT: (i64, i64) = (393, 852);
/// How long the capture loop keeps going after the last viewer request, and how it paces itself: fast while a person is
/// acting or the page is changing, slow while it holds still.
const FEED_IDLE_AFTER: Duration = Duration::from_secs(20);
const FEED_FAST_PAUSE: Duration = Duration::from_millis(60);
const FEED_STILL_PAUSE: Duration = Duration::from_millis(400);
const FEED_ACTIVE_FOR: Duration = Duration::from_secs(2);
const FEED_STILL_AFTER: u32 = 4;
const FRAME_QUALITY: u8 = 60;
const MAX_TYPED: usize = 256;
const DISMISS_COOKIE_BANNER_JS: &str = include_str!("js/dismiss_cookie_banner.js");
const PREFILL_JS: &str = include_str!("js/prefill_login.js");
const EDITABLE_JS: &str = "(() => { const a = document.activeElement; return !!a && (a.isContentEditable || /^(INPUT|TEXTAREA)$/.test(a.tagName)); })()";

pub struct Config {
    pub user: String,
    pub cdp_endpoint: String,
}

impl Config {
    pub fn from_env() -> Config {
        Config {
            user: "abc".to_string(),
            cdp_endpoint: std::env::var("VNC_AUTH_CDP_ENDPOINT").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| DEFAULT_CDP_ENDPOINT.to_string()),
        }
    }
}

/// One frame of the page and whether a text field has the focus (the viewer raises the phone keyboard for it).
pub struct Frame {
    pub jpeg: Vec<u8>,
    pub editable: bool,
}

struct Desktop {
    config: Config,
    attached: Option<Attached>,
}

struct FeedState {
    seq: u64,
    frame: Option<Arc<Frame>>,
    error: Option<String>,
    wanted_at: Instant,
    active_until: Instant,
    started: bool,
}

/// The latest frame of the page, taken by one thread for every viewer and numbered: a viewer asks for the frame after the one
/// it has and waits until the page changed, so a page that holds still costs no traffic and no screenshot per viewer.
struct Feed {
    state: Mutex<FeedState>,
    changed: Condvar,
}

/// Three browser connections, each behind its own lock: navigation and cookie work hold theirs for seconds, and the viewer's
/// screenshots and the person's taps must wait for neither the navigation nor each other.
pub struct Launcher {
    inner: Mutex<Desktop>,
    view: Mutex<Option<Attached>>,
    input: Mutex<Option<Attached>>,
    feed: Feed,
    cdp_endpoint: String,
    /// Kept outside `inner`: starting a login command must not wait for the navigation that holds that lock.
    user: String,
}

fn spawn_detached(mut command: Command, event: &str, fields: Vec<(&'static str, String)>) {
    use std::os::unix::process::CommandExt;
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => {
            let mut all = fields;
            all.push(("err", e.to_string()));
            log::error(event, &all);
        }
    }
}

fn file_safe(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

impl Desktop {
    fn cdp_is_up(&self) -> bool {
        cdp::attached::browser_ws_url(&self.config.cdp_endpoint).is_ok()
    }

    /// Starts the browser at `url`: only the cold path, afterwards everything goes over CDP. `chromium` is the wrapper the
    /// image installs, which makes it a kiosk window with CDP on.
    fn spawn_browser(&self, url: &str) {
        let mut command = Command::new("s6-setuidgid");
        command.args([&self.config.user, "sh", "-c", "exec chromium \"$1\"", "exec", url]);
        spawn_detached(command, "vnc_auth.desktop_launcher.browser_spawn_failed", vec![("url", safe_url(url))]);
    }

    fn wait_for_cdp(&self) -> bool {
        let deadline = Instant::now() + BROWSER_START_TIMEOUT;
        while Instant::now() < deadline {
            if self.cdp_is_up() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        false
    }

    fn apply_mobile_emulation(page: &mut Attached) {
        match page.emulate_phone(MOBILE_USER_AGENT, VIEWPORT.0, VIEWPORT.1) {
            Ok(()) => log::info("vnc_auth.desktop_launcher.mobile_emulation_applied", &[("width", VIEWPORT.0.to_string()), ("height", VIEWPORT.1.to_string())]),
            Err(e) => log::error("vnc_auth.desktop_launcher.mobile_emulation_failed", &[("err", e)]),
        }
    }

    /// Attaches once and reuses; `None` when no browser is running to attach to.
    fn attached(&mut self) -> Result<Option<&mut Attached>, String> {
        if self.attached.is_none() {
            if !self.cdp_is_up() {
                return Ok(None);
            }
            let mut page = Attached::attach(&self.config.cdp_endpoint)?;
            Self::apply_mobile_emulation(&mut page);
            self.attached = Some(page);
        }
        Ok(self.attached.as_mut())
    }

    /// Drops a connection that went stale (the browser restarted under it).
    fn detach(&mut self) {
        self.attached = None;
    }

    /// Dismisses whatever consent banner the loaded page shows, in every frame (consent platforms often render inside an
    /// iframe), polling because a banner can render after the page's own load event.
    fn dismiss_cookie_banner(page: &mut Attached) {
        for attempt in 1..=COOKIE_BANNER_ATTEMPTS {
            std::thread::sleep(COOKIE_BANNER_DELAY);
            let frames = match page.frames() {
                Ok(f) => f,
                Err(e) => {
                    log::error("vnc_auth.desktop_launcher.cookie_banner_check_failed", &[("attempt", attempt.to_string()), ("err", e)]);
                    return;
                }
            };
            let (mut dismissed, mut action, mut label) = (false, "none".to_string(), String::new());
            for frame in &frames {
                if let Ok(result) = page.eval_in_frame(&frame.id, DISMISS_COOKIE_BANNER_JS) {
                    if result["dismissed"] == Value::Bool(true) {
                        dismissed = true;
                        action = result["action"].as_str().unwrap_or("unknown").to_string();
                        label = result["label"].as_str().unwrap_or("").to_string();
                        break;
                    }
                }
            }
            log::info(
                "vnc_auth.desktop_launcher.cookie_banner_checked",
                &[("attempt", attempt.to_string()), ("frameCount", frames.len().to_string()), ("dismissed", dismissed.to_string()), ("action", action), ("label", label)],
            );
            if dismissed {
                return;
            }
        }
    }

    fn navigate_inner(&mut self, url: &str) -> Result<(), String> {
        if self.attached()?.is_none() {
            // Cold start: spawn AT the target so the human never sees a blank page that navigates a moment later.
            self.spawn_browser(url);
            if !self.wait_for_cdp() {
                log::error("vnc_auth.desktop_launcher.browser_never_came_up", &[("url", safe_url(url))]);
                return Ok(());
            }
            if self.attached()?.is_none() {
                return Ok(());
            }
        }
        let mut page = self.attached.take().ok_or("no browser attached")?;
        Self::apply_mobile_emulation(&mut page);
        let result = (|| {
            page.navigate(url)?;
            // A successful CDP call is not a successful navigation: assert against the document the browser ended up on.
            match page.eval("location.href") {
                Ok(landed) => {
                    let href = landed.as_str().unwrap_or("").to_string();
                    let wanted = url.split_once("://").map_or("", |(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""));
                    if !href.is_empty() && !href.contains(wanted) {
                        log::error("vnc_auth.desktop_launcher.navigate_target_mismatch", &[("wantedHost", wanted.to_string()), ("landedUrl", safe_url(&href))]);
                    }
                }
                Err(e) => log::error("vnc_auth.desktop_launcher.navigate_verify_failed", &[("err", e)]),
            }
            log::info("vnc_auth.desktop_launcher.navigated", &[("url", safe_url(url))]);
            Self::dismiss_cookie_banner(&mut page);
            Ok::<(), String>(())
        })();
        if result.is_ok() {
            self.attached = Some(page);
        }
        result
    }
}

impl Launcher {
    pub fn new(config: Config) -> Launcher {
        let (cdp_endpoint, user) = (config.cdp_endpoint.clone(), config.user.clone());
        let now = Instant::now();
        let state = FeedState { seq: 0, frame: None, error: None, wanted_at: now, active_until: now, started: false };
        Launcher { inner: Mutex::new(Desktop { config, attached: None }), view: Mutex::new(None), input: Mutex::new(None), feed: Feed { state: Mutex::new(state), changed: Condvar::new() }, cdp_endpoint, user }
    }

    /// The user the login commands run as: what checks of a login must run as too.
    pub fn user(&self) -> &str {
        &self.user
    }

    fn desktop(&self) -> std::sync::MutexGuard<'_, Desktop> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs the tool's login command in the background, its output kept in a log file. It finds the browser through
    /// `$BROWSER`; no display is needed, only a name for the tools that refuse to open a browser without one.
    pub fn run_login(&self, tool: &str, command: &str) {
        let script = format!("{command} >/tmp/vnc-auth-login-{}.log 2>&1", file_safe(tool));
        let mut login = Command::new("s6-setuidgid");
        login.args([&self.user, "bash", "-lc", &script]).env("BROWSER", "chromium").env("DISPLAY", ":99");
        spawn_detached(login, "vnc_auth.desktop_launcher.spawn_failed", vec![("tool", tool.to_string())]);
    }

    pub fn navigate(&self, url: &str) {
        let mut desktop = self.desktop();
        if let Err(e) = desktop.navigate_inner(url) {
            desktop.detach();
            // A navigation error quotes the whole URL, which may carry a magic-link token.
            log::error("vnc_auth.desktop_launcher.navigate_failed", &[("url", safe_url(url)), ("err", e.replace(url, &safe_url(url)))]);
        }
    }

    /// Fills the login email field (the page's own email/username input), conservatively: nothing is touched otherwise.
    pub fn prefill_login(&self, email: &str) {
        let mut desktop = self.desktop();
        let outcome = desktop.attached().and_then(|page| match page {
            Some(page) => page.eval(&PREFILL_JS.replace("EMAIL_PLACEHOLDER", &Value::String(email.to_string()).to_string())).map(Some),
            None => Ok(None),
        });
        match outcome {
            Ok(Some(result)) => log::info("vnc_auth.desktop_launcher.prefilled", &[("result", result.to_string())]),
            Ok(None) => {}
            Err(e) => log::error("vnc_auth.desktop_launcher.prefill_failed", &[("err", e)]),
        }
    }

    /// One of the viewer's own connections, attached on first use and dropped when it fails (the browser restarted under it).
    fn with_connection<T>(&self, slot: &Mutex<Option<Attached>>, operation: impl FnOnce(&mut Attached) -> Result<T, String>) -> Result<T, String> {
        let mut connection = slot.lock().unwrap_or_else(|e| e.into_inner());
        if connection.is_none() {
            if cdp::attached::browser_ws_url(&self.cdp_endpoint).is_err() {
                return Err("no browser".to_string());
            }
            *connection = Some(Attached::attach(&self.cdp_endpoint)?);
        }
        let outcome = operation(connection.as_mut().ok_or("no browser")?);
        if outcome.is_err() {
            *connection = None;
        }
        outcome
    }

    fn with_view<T>(&self, operation: impl FnOnce(&mut Attached) -> Result<T, String>) -> Result<T, String> {
        self.with_connection(&self.view, operation)
    }

    fn with_input<T>(&self, operation: impl FnOnce(&mut Attached) -> Result<T, String>) -> Result<T, String> {
        let outcome = self.with_connection(&self.input, operation);
        self.feed.state.lock().unwrap_or_else(|e| e.into_inner()).active_until = Instant::now() + FEED_ACTIVE_FOR;
        outcome
    }

    fn take_frame(&self) -> Result<Frame, String> {
        self.with_view(|page| {
            let jpeg = page.screenshot_jpeg(FRAME_QUALITY)?;
            let editable = page.eval(EDITABLE_JS).ok().and_then(|v| v.as_bool()).unwrap_or(false);
            Ok(Frame { jpeg, editable })
        })
    }

    /// The capture loop: it runs while a viewer has asked for a frame lately, stores a frame only when the page changed, and
    /// records the error (no browser, a dead connection) so a waiting viewer hears of it.
    fn capture_loop(launcher: Arc<Launcher>) {
        let mut previous: Vec<u8> = Vec::new();
        let mut still = 0;
        loop {
            let (idle, active) = {
                let state = launcher.feed.state.lock().unwrap_or_else(|e| e.into_inner());
                (state.wanted_at.elapsed() > FEED_IDLE_AFTER, Instant::now() < state.active_until)
            };
            if idle {
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            let shot = launcher.take_frame();
            let mut state = launcher.feed.state.lock().unwrap_or_else(|e| e.into_inner());
            match shot {
                Ok(frame) => {
                    let recovered = state.error.take().is_some();
                    if frame.jpeg != previous || state.frame.is_none() {
                        previous = frame.jpeg.clone();
                        state.seq += 1;
                        state.frame = Some(Arc::new(frame));
                        still = 0;
                        launcher.feed.changed.notify_all();
                    } else {
                        still += 1;
                        if recovered {
                            launcher.feed.changed.notify_all();
                        }
                    }
                }
                Err(e) => {
                    state.error = Some(e);
                    launcher.feed.changed.notify_all();
                }
            }
            let failed = state.error.is_some();
            drop(state);
            std::thread::sleep(if failed { Duration::from_millis(500) } else if active || still < FEED_STILL_AFTER { FEED_FAST_PAUSE } else { FEED_STILL_PAUSE });
        }
    }

    /// The page as it looks, once it differs from frame number `after` (any frame when `None`); `Ok(None)` when it still
    /// looks the same after `wait`. The number goes back with the frame.
    pub fn frame_after(self: &Arc<Self>, after: Option<u64>, wait: Duration) -> Result<Option<(u64, Arc<Frame>)>, String> {
        let deadline = Instant::now() + wait;
        let mut state = self.feed.state.lock().unwrap_or_else(|e| e.into_inner());
        state.wanted_at = Instant::now();
        if !state.started {
            state.started = true;
            let launcher = Arc::clone(self);
            std::thread::spawn(move || Launcher::capture_loop(launcher));
        }
        loop {
            if let Some(e) = &state.error {
                return Err(e.clone());
            }
            if let Some(frame) = &state.frame {
                if after.is_none_or(|a| state.seq > a) {
                    return Ok(Some((state.seq, Arc::clone(frame))));
                }
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) else { return Ok(None) };
            state = self.feed.changed.wait_timeout(state, left).unwrap_or_else(|e| e.into_inner()).0;
            state.wanted_at = Instant::now();
        }
    }

    /// Clears the login browser's cookies and cache, so the next sign-in starts from the account chooser. `false` when no
    /// browser is running (nothing to clear) or it could not be done.
    pub fn clear_browser(&self) -> bool {
        self.with_input(|page| page.clear_browser_data()).is_ok()
    }

    /// One input from the viewer: `tap {x,y}`, `scroll {x,y,dy}`, `text {text}` or `key {key}`.
    pub fn input(&self, event: &Value) -> Result<(), String> {
        let number = |key: &str| event[key].as_f64().filter(|n| n.is_finite()).ok_or_else(|| format!("missing {key}"));
        match event["type"].as_str() {
            Some("tap") => {
                let (x, y) = (number("x")?, number("y")?);
                self.with_input(|page| page.click(x, y))
            }
            Some("scroll") => {
                let (x, y, dy) = (number("x")?, number("y")?, number("dy")?);
                self.with_input(|page| page.wheel(x, y, dy))
            }
            Some("text") => {
                let text: String = event["text"].as_str().ok_or("missing text")?.chars().take(MAX_TYPED).collect();
                self.with_input(|page| page.insert_text(&text))
            }
            Some("key") => {
                let key = event["key"].as_str().ok_or("missing key")?.to_string();
                self.with_input(|page| page.press_key(&key))
            }
            _ => Err("unknown input type".to_string()),
        }
    }

    /// The `(name, value)` of every cookie the browser would send to `urls`; empty when there is no browser.
    pub fn cookies(&self, urls: &[String]) -> Vec<(String, String)> {
        let mut desktop = self.desktop();
        let found = desktop.attached().and_then(|page| match page {
            Some(page) => page.cookies(Some(urls)).map(Some),
            None => Ok(None),
        });
        match found {
            // Never log values: a session cookie is a bearer credential.
            Ok(Some(all)) => {
                log::info("vnc_auth.desktop_launcher.cookies_read", &[("count", all.len().to_string())]);
                all.iter().filter_map(|c| Some((c["name"].as_str()?.to_string(), c["value"].as_str()?.to_string()))).collect()
            }
            Ok(None) => Vec::new(),
            Err(e) => {
                log::error("vnc_auth.desktop_launcher.cookies_failed", &[("err", e)]);
                Vec::new()
            }
        }
    }
}
