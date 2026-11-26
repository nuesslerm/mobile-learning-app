//! The browser table: one headless Chromium for the executor, started by the first page and stopped with the last, and a page per key. A page is
//! opened at most `MAX_PAGES` at a time, closed after `IDLE_MS` unused (a reaper thread, so an idle browser frees its memory with no call to
//! trigger it). The browser runs through the executor's filtering proxy (`browser_proxy`), which resolves each name itself and refuses the host's
//! own network, so a page's requests, redirects and WebSockets cannot reach it; the page's address is also checked before each navigation.
use base64::Engine;
use cdp::attached::Attached;
use crate::adapters::browser_proxy::{blocked_ip, Egress, Proxy};
use cdp::launch::{find_chromium, launch, Launched};
use exec_browser::{parse_url, Closed, Target, Direction, Evaluated, Input, Inputted, Listed, PageInfo, PageRow, Policy, Shot, MAX_PAGES, IDLE_MS, SCREENSHOT_MAX, VALUE_MAX};
use exec_proto::{Code, ExecError, ExecResult};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

const CHROMIUM_ARGS: [&str; 9] = ["--disable-gpu", "--disable-dev-shm-usage", "--disable-extensions", "--mute-audio", "--window-size=1280,800", "--disable-features=OverscrollHistoryNavigation,BackForwardCache", "--proxy-bypass-list=<-loopback>", "--disable-quic", "--force-webrtc-ip-handling-policy=disable_non_proxied_udp"];

struct Drive {
    next_navigation_sample: Instant,
    navigation: Option<exec_browser::Navigation>,
    viewport: Option<exec_browser::FrameViewport>,
    page: Attached,
    x: f64,
    y: f64,
    buttons: u8,
}

impl Drop for Drive {
    fn drop(&mut self) {
        self.page.close_target();
    }
}

struct Entry {
    key: String,
    drive: Arc<Mutex<Drive>>,
    used: Instant,
}

#[derive(Default)]
struct State {
    browser: Option<Launched>,
    proxy: Option<Proxy>,
    pages: Vec<Entry>,
}

impl State {
    fn stop_browser(&mut self) {
        self.pages.clear();
        self.proxy = None;
        if let Some(b) = self.browser.take() {
            b.close();
        }
    }
}

struct Inner {
    policy: Policy,
    idle: Duration,
    max_pages: usize,
    chromium: Option<PathBuf>,
    state: Mutex<State>,
    reaper: Mutex<bool>,
}

pub struct BrowserTable {
    inner: Arc<Inner>,
}

fn io(e: impl std::fmt::Display) -> ExecError {
    ExecError::new(Code::Io, e.to_string())
}

fn no_page(key: &str) -> ExecError {
    ExecError::new(Code::NotFound, format!("no page is open for key {key}"))
}

/// Whether the host of the target is an address the executor blocks (a name is not: its answers are checked per request).
fn blocked_host(target: &Target) -> bool {
    target.host.parse::<IpAddr>().is_ok_and(blocked_ip)
}

/// Whether the page may make this request at all: only web addresses and the inline kinds, and for a web address the policy and its own host (the
/// proxy judges the addresses a name resolves to).
fn request_allowed(policy: &Policy, url: &str) -> bool {
    if ["data:", "about:", "blob:"].iter().any(|s| url.starts_with(s)) {
        return true;
    }
    let Ok(target) = parse_url(url) else { return false };
    policy.names(&target) || !(policy.internal(&target) || blocked_host(&target))
}

fn mask(button: &str) -> u8 {
    match button {
        "right" => 2,
        "middle" => 4,
        _ => 1,
    }
}

fn modifier_bits(modifiers: &[String]) -> u32 {
    modifiers.iter().fold(0, |bits, m| bits | match m.as_str() {
        "alt" => 1,
        "ctrl" => 2,
        "cmd" => 4,
        _ => 8,
    })
}

impl Default for BrowserTable {
    fn default() -> Self {
        BrowserTable::new(Policy::default(), Duration::from_millis(IDLE_MS), MAX_PAGES)
    }
}

impl BrowserTable {
    pub fn new(policy: Policy, idle: Duration, max_pages: usize) -> BrowserTable {
        BrowserTable { inner: Arc::new(Inner { policy, idle, max_pages, chromium: None, state: Mutex::new(State::default()), reaper: Mutex::new(false) }) }
    }

    /// Runs this Chromium in place of the one found on the machine.
    pub fn with_chromium(mut self, exe: PathBuf) -> BrowserTable {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.chromium = Some(exe);
        }
        self
    }

    pub fn open(&self, key: &str, url: Option<&str>) -> ExecResult<PageInfo> {
        if let Some(url) = url {
            self.inner.policy.check_with(url, blocked_host)?;
        }
        let drive = self.inner.page_for_open(key)?;
        let mut d = drive.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(url) = url {
            d.page.navigate(url).map_err(io)?;
        }
        let info = Self::info(key, &mut d)?;
        drop(d);
        self.inner.touch(key);
        Ok(info)
    }

    fn info(key: &str, d: &mut Drive) -> ExecResult<PageInfo> {
        let (url, title) = d.page.location().map_err(io)?;
        Ok(PageInfo { key: key.to_string(), url, title })
    }

    /// Runs `act` on the page of `key` with the page held, so nothing else drives it meanwhile and the reaper leaves it.
    fn drive<T>(&self, key: &str, act: impl FnOnce(&mut Drive) -> ExecResult<T>) -> ExecResult<T> {
        let drive = self.inner.checkout(key)?;
        let done = act(&mut drive.lock().unwrap_or_else(|p| p.into_inner()));
        drop(drive);
        self.inner.touch(key);
        done
    }

    pub fn goto(&self, key: &str, url: &str) -> ExecResult<PageInfo> {
        self.inner.policy.check_with(url, blocked_host)?;
        self.drive(key, |d| {
            d.page.navigate(url).map_err(io)?;
            Self::info(key, d)
        })
    }

    pub fn history(&self, key: &str, direction: Direction) -> ExecResult<PageInfo> {
        self.drive(key, |d| {
            match direction {
                Direction::Back => d.page.history_step(-1),
                Direction::Forward => d.page.history_step(1),
                Direction::Reload => d.page.reload(),
            }
            .map_err(io)?;
            Self::info(key, d)
        })
    }

    pub fn eval(&self, key: &str, expression: &str) -> ExecResult<Evaluated> {
        self.drive(key, |d| {
            let value = d.page.eval_page(expression).map_err(|e| ExecError::new(Code::Io, format!("the expression failed: {e}")))?;
            if value.to_string().len() > VALUE_MAX {
                return Err(ExecError::new(Code::Io, format!("the value is larger than {VALUE_MAX} bytes")));
            }
            Ok(Evaluated { value })
        })
    }

    pub fn screenshot(&self, key: &str) -> ExecResult<Shot> {
        self.drive(key, |d| {
            let png = d.page.screenshot_png(false).map_err(io)?;
            if png.len() > SCREENSHOT_MAX {
                return Err(ExecError::new(Code::Io, format!("the picture is larger than {SCREENSHOT_MAX} bytes")));
            }
            Ok(Shot { navigation: None, mime: "image/png".into(), bytes: png.len(), data: base64::engine::general_purpose::STANDARD.encode(png) })
        })
    }

    pub fn frame(&self, key: &str, viewport: exec_browser::FrameViewport, quality: u8) -> ExecResult<Shot> {
        self.drive(key, |d| {
            if d.viewport != Some(viewport) {
                d.page.set_metrics(viewport.width.into(), viewport.height.into(), 1.0, viewport.mobile).map_err(io)?;
                d.viewport = Some(viewport);
            }
            let jpeg = d.page.screenshot_jpeg(quality).map_err(io)?;
            if jpeg.len() > SCREENSHOT_MAX {
                return Err(ExecError::new(Code::Io, format!("the picture is larger than {SCREENSHOT_MAX} bytes")));
            }
            if Instant::now() >= d.next_navigation_sample {
                d.next_navigation_sample = Instant::now() + Duration::from_millis(400);
                let history = d.page.navigation_history().map_err(io)?;
                let index = history["currentIndex"].as_u64().map(|i| i as usize);
                d.navigation = index.and_then(|i| {
                    let entries = history["entries"].as_array()?;
                    let entry = entries.get(i)?;
                    Some(exec_browser::Navigation {
                        url: entry["url"].as_str()?.to_owned(),
                        title: entry["title"].as_str().unwrap_or_default().to_owned(),
                        can_go_back: i > 0,
                        can_go_forward: i + 1 < entries.len(),
                    })
                });
            }
            Ok(Shot { navigation: d.navigation.clone(), mime: "image/jpeg".into(), bytes: jpeg.len(), data: base64::engine::general_purpose::STANDARD.encode(jpeg) })
        })
    }

    pub fn input(&self, key: &str, input: &Input) -> ExecResult<Inputted> {
        self.drive(key, |d| {
            let pointer = |d: &mut Drive, params: serde_json::Value| d.page.mouse_event(params).map_err(io);
            match input {
                Input::MouseMove { x, y } => {
                    (d.x, d.y) = (*x, *y);
                    let held = if d.buttons & 1 != 0 { "left" } else if d.buttons & 4 != 0 { "middle" } else if d.buttons & 2 != 0 { "right" } else { "none" };
                    pointer(d, serde_json::json!({"type": "mouseMoved", "x": d.x, "y": d.y, "button": held, "buttons": d.buttons}))?;
                }
                Input::MouseDown { button } => {
                    d.buttons |= mask(button);
                    pointer(d, serde_json::json!({"type": "mousePressed", "x": d.x, "y": d.y, "button": button, "buttons": d.buttons, "clickCount": 1}))?;
                }
                Input::MouseUp { button } => {
                    let button = button.clone().unwrap_or_else(|| if d.buttons & 2 != 0 && d.buttons & 1 == 0 { "right" } else if d.buttons & 4 != 0 && d.buttons & 1 == 0 { "middle" } else { "left" }.to_string());
                    d.buttons &= !mask(&button);
                    pointer(d, serde_json::json!({"type": "mouseReleased", "x": d.x, "y": d.y, "button": button, "buttons": d.buttons, "clickCount": 1}))?;
                }
                Input::MouseClick { x, y, button, modifiers } => {
                    (d.x, d.y) = (*x, *y);
                    let bits = modifier_bits(modifiers);
                    pointer(d, serde_json::json!({"type": "mouseMoved", "x": d.x, "y": d.y, "modifiers": bits}))?;
                    pointer(d, serde_json::json!({"type": "mousePressed", "x": d.x, "y": d.y, "button": button, "buttons": mask(button), "modifiers": bits, "clickCount": 1}))?;
                    pointer(d, serde_json::json!({"type": "mouseReleased", "x": d.x, "y": d.y, "button": button, "buttons": 0, "modifiers": bits, "clickCount": 1}))?;
                    d.buttons = 0;
                }
                Input::MouseWheel { dx, dy } => pointer(d, serde_json::json!({"type": "mouseWheel", "x": d.x, "y": d.y, "deltaX": dx, "deltaY": dy, "buttons": d.buttons}))?,
                Input::KeyPress { key } => {
                    let mut chars = key.chars();
                    match (chars.next(), chars.next()) {
                        (Some(ch), None) => d.page.type_char(ch, "desktop").map_err(io)?,
                        _ => d.page.press_key(key).map_err(|e| if e.starts_with("unsupported key") { ExecError::invalid_args(format!("`text` names a key this executor does not press: {key}")) } else { io(e) })?,
                    }
                }
                Input::InsertText { text } => d.page.insert_text(text).map_err(io)?,
            }
            Ok(Inputted { x: d.x, y: d.y, buttons: d.buttons })
        })
    }

    pub fn close(&self, key: &str) -> Closed {
        Closed { closed: self.inner.remove(key) }
    }

    pub fn list(&self) -> Listed {
        let rows: Vec<(String, Arc<Mutex<Drive>>, Instant)> = {
            let mut state = self.inner.lock();
            self.inner.reap_locked(&mut state);
            state.pages.iter().map(|p| (p.key.clone(), p.drive.clone(), p.used)).collect()
        };
        let now = Instant::now();
        let pages = rows
            .into_iter()
            .map(|(key, drive, used)| {
                let url = drive.try_lock().ok().and_then(|mut d| d.page.location().ok()).map(|l| l.0).unwrap_or_default();
                PageRow { key, url, idle_ms: now.duration_since(used).as_millis() as u64 }
            })
            .collect();
        Listed { pages, max_pages: self.inner.max_pages, idle_limit_ms: self.inner.idle.as_millis() as u64 }
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn touch(&self, key: &str) {
        if let Some(p) = self.lock().pages.iter_mut().find(|p| p.key == key) {
            p.used = Instant::now();
        }
    }

    fn checkout(&self, key: &str) -> ExecResult<Arc<Mutex<Drive>>> {
        let mut state = self.lock();
        self.reap_locked(&mut state);
        let page = state.pages.iter_mut().find(|p| p.key == key).ok_or_else(|| no_page(key))?;
        page.used = Instant::now();
        Ok(page.drive.clone())
    }

    /// The page of `key`, opened (with the browser, if it is the first) when there is none.
    fn page_for_open(self: &Arc<Self>, key: &str) -> ExecResult<Arc<Mutex<Drive>>> {
        let mut state = self.lock();
        self.reap_locked(&mut state);
        if let Some(p) = state.pages.iter_mut().find(|p| p.key == key) {
            p.used = Instant::now();
            return Ok(p.drive.clone());
        }
        if state.pages.len() >= self.max_pages {
            return Err(ExecError::new(Code::AlreadyExists, format!("{} browser pages are already open; close one first", self.max_pages)));
        }
        if state.browser.is_none() {
            let exe = self.chromium.clone().or_else(find_chromium).ok_or_else(|| ExecError::new(Code::Io, "no Chromium is installed at this executor (set WF_CHROMIUM to one)"))?;
            let proxy = Proxy::start(Egress::new(self.policy.clone())).map_err(io)?;
            let mut args: Vec<String> = CHROMIUM_ARGS.iter().map(|a| a.to_string()).collect();
            args.push(format!("--proxy-server=http://127.0.0.1:{}", proxy.port()));
            let sandbox = std::env::var_os("RCA_EXEC_BROWSER_NO_SANDBOX").is_none();
            state.browser = Some(launch(&exe, &args, &[], true, sandbox).map_err(io)?);
            state.proxy = Some(proxy);
            self.start_reaper();
        }
        let endpoint = state.browser.as_ref().map(|b| b.endpoint.clone()).unwrap_or_default();
        let mut page = Attached::attach_new(&endpoint).map_err(io)?;
        let policy = self.policy.clone();
        page.guard_requests(Arc::new(move |url| request_allowed(&policy, url))).map_err(io)?;
        let drive = Arc::new(Mutex::new(Drive { next_navigation_sample: Instant::now(), navigation: None, viewport: None, page, x: 0.0, y: 0.0, buttons: 0 }));
        state.pages.push(Entry { key: key.to_string(), drive: drive.clone(), used: Instant::now() });
        Ok(drive)
    }

    fn remove(&self, key: &str) -> bool {
        let mut state = self.lock();
        let Some(at) = state.pages.iter().position(|p| p.key == key) else { return false };
        state.pages.remove(at);
        if state.pages.is_empty() {
            state.stop_browser();
        }
        true
    }

    /// Closes the pages idle for the limit that nothing holds, and the browser when none is left or it died.
    fn reap_locked(&self, state: &mut State) {
        if state.browser.as_mut().is_some_and(|b| matches!(b.child.try_wait(), Ok(Some(_)))) {
            state.stop_browser();
            return;
        }
        let now = Instant::now();
        state.pages.retain(|e| now.duration_since(e.used) < self.idle || Arc::strong_count(&e.drive) > 1);
        if state.pages.is_empty() {
            state.stop_browser();
        }
    }

    fn start_reaper(self: &Arc<Self>) {
        let mut started = self.reaper.lock().unwrap_or_else(|p| p.into_inner());
        if *started {
            return;
        }
        *started = true;
        let weak: Weak<Inner> = Arc::downgrade(self);
        let tick = (self.idle / 4).clamp(Duration::from_millis(50), Duration::from_secs(5));
        std::thread::spawn(move || loop {
            std::thread::sleep(tick);
            let Some(inner) = weak.upgrade() else { return };
            let mut state = inner.lock();
            inner.reap_locked(&mut state);
        });
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.state.get_mut().unwrap_or_else(|p| p.into_inner()).stop_browser();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_proto::Code;

    #[test]
    fn given_addresses_when_judged_then_loopback_private_link_local_metadata_and_shared_ranges_are_blocked_and_public_ones_are_not() {
        // Given the default policy
        let policy = Policy::default();
        for url in ["http://127.0.0.1:3000/", "http://169.254.169.254/latest/meta-data", "http://[::1]/", "http://[fe80::1]/", "http://[::ffff:127.0.0.1]/", "http://[::ffff:169.254.169.254]/", "http://10.0.0.5/", "http://192.168.1.1/", "http://172.16.0.1/", "http://100.64.1.1/", "http://0.0.0.0/", "http://[fd00::1]/", "http://localhost/", "http://printer.local/"] {
            // Then each is refused with the executor's wording, by the host it names
            let e = policy.check_with(url, blocked_host).unwrap_err();
            assert!(e.code == Code::InvalidArgs && e.message.contains("does not let a page reach"), "{url}: {e:?}");
            assert!(!request_allowed(&policy, url), "{url}");
        }
        for url in ["https://example.com/", "http://93.184.216.34/", "http://[2001:4860:4860::8888]/", "http://100.63.0.1/", "http://172.32.0.1/"] {
            assert!(policy.check_with(url, blocked_host).is_ok(), "{url}");
        }
        // And a request for a scheme that is no web page, or inline data, is judged by what it is
        assert!(request_allowed(&policy, "data:text/plain,hi") && !request_allowed(&policy, "file:///etc/passwd") && !request_allowed(&policy, "ftp://example.com/"));
    }

    #[test]
    fn given_an_allowlist_when_requests_are_judged_then_only_the_named_host_and_port_of_a_blocked_range_pass() {
        // Given a policy that names one loopback server
        let policy = Policy::parse("127.0.0.1:4100").unwrap();
        // Then it passes, its neighbours do not, and a name that resolves to loopback is blocked by its address
        assert!(request_allowed(&policy, "http://127.0.0.1:4100/a") && !request_allowed(&policy, "http://127.0.0.1:4101/a"));
        assert!(!request_allowed(&Policy::default(), "http://localtest.invalid.localhost:80/"));
    }
}
