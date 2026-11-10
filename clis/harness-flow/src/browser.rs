//! The headless Chromium of the flow harness: paints a frozen flow DOM for `wf flow goldens` and serves the visual tier of
//! `wf check --visual`, which adds its own page probe on top of `page`.
use cdp::attached::Attached;
use cdp::launch::{find_chromium, launch};
use std::path::Path;
use std::time::{Duration, Instant};

type Res<T> = Result<T, String>;

pub struct VisualBrowser {
    pub page: Attached,
    launched: Option<cdp::launch::Launched>,
}

/// Chromium reaches tailnet names (the backends) through tailscaled's SOCKS5 listener when `WF_SOCKS_PORT` names it, as `wf browser` does.
fn tailnet_socks_arg(port: Option<&str>) -> Option<String> {
    port.filter(|p| !p.is_empty()).map(|p| format!("--proxy-server=socks5://localhost:{p}"))
}

impl VisualBrowser {
    /// A plain headless Chromium of the given viewport, with `init` run in every new document.
    pub fn open_with(viewport: (i64, i64), init: Option<&str>) -> Res<VisualBrowser> {
        let exe = find_chromium().ok_or("no Chromium found (set WF_CHROMIUM or install one with Playwright)")?;
        let mut chrome_args = vec!["--autoplay-policy=no-user-gesture-required".to_string()];
        chrome_args.extend(tailnet_socks_arg(std::env::var("WF_SOCKS_PORT").ok().as_deref()));
        let launched = launch(&exe, &chrome_args, &[], true)?;
        let setup = (|| -> Res<Attached> {
            let mut page = Attached::attach(&launched.endpoint)?;
            page.set_metrics(viewport.0, viewport.1, 1.0, false)?;
            if let Some(script) = init {
                page.add_init_script(script)?;
            }
            Ok(page)
        })();
        match setup {
            Ok(page) => Ok(VisualBrowser { page, launched: Some(launched) }),
            Err(e) => {
                launched.close();
                Err(e)
            }
        }
    }

    /// Opens `url` and waits for its load event, as a navigation that resolves on `load` does.
    pub fn navigate_loaded(&mut self, url: &str) -> Res<()> {
        self.navigate_loaded_within(url, Duration::from_secs(30))
    }

    /// The same with its own load budget: a server that is still building its first page answers it in minutes.
    pub fn navigate_loaded_within(&mut self, url: &str, budget: Duration) -> Res<()> {
        // Page.navigate answers once the response arrives, under a fixed 60 s reply limit that a loaded server rendering a
        // deep route exceeds: that one error is not a failure, the budget below does the waiting.
        // The marker tells the old document from the new one, even when both have the same URL.
        let _ = self.page.eval_page("window.__wfLeaving = true");
        if let Err(e) = self.page.navigate(url) {
            if !e.contains("timed out") {
                return Err(e);
            }
        }
        let loading = Instant::now();
        loop {
            let state = self.page.eval_page("(window.__wfLeaving ? 'leaving ' : '') + location.href + ' ' + document.readyState").ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            if state.starts_with(url) && state.ends_with("complete") {
                return Ok(());
            }
            if state.starts_with("chrome-error:") {
                return Err(format!("page.goto: net::ERR_FAILED at {url}"));
            }
            if loading.elapsed() > budget {
                return Err(format!("page.goto: Timeout {}ms exceeded.", budget.as_millis()));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn screenshot(&mut self, dest: &Path) -> Res<()> {
        let png = self.page.screenshot_png(true)?;
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(dest, png).map_err(|e| e.to_string())
    }

    pub fn close(mut self) {
        if let Some(l) = self.launched.take() {
            l.close();
        }
    }
}
