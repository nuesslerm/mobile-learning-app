# rustwright 0.3.0 against the `wf browser` engine (`clis/cdp`)

Date: 2026-10-09. Owner request: replace "the Playwright dependency of the browser CLI and SDK" with
[rustwright](https://github.com/Skyvern-AI/rustwright) and compare the earlier Playwright runbooks with the Rust rewrite.
Scope agreed with the owner: a spike in a scratch directory, no repository change until the numbers and gaps are read. Raw
Playwright was not run: the approve guard forbids hand-launching it, and the owner chose to skip it (the Playwright figure in
the rustwright README, 2.55x on 17 cases, is the vendor's own diagnostic and was not reproduced here).

## Where Playwright is today

- `wf browser` is the Rust daemon in `clis/wf` on our own CDP client, `clis/cdp` (about 2,000 lines plus eight injected scripts).
  There is no Playwright at run time.
- `@lib/browser-client`, the SDK, is a thin client of that daemon (`createContainer()`, every method one `POST /call`). It lists
  `playwright` as a devDependency only for types.
- Playwright still runs in `backend/leetcode-auth-cookie-capture` (the Node service; the guard allows it, tracked exception
  #201), in the frontend's `playwright test` specs and in the agent-shell image (browser install).

So "replace Playwright in the CLI and SDK" means replacing `clis/cdp` with rustwright's engine, not a dependency swap.

## What rustwright is

An alpha (0.3.0) Rust rewrite of Playwright for Chromium. `rustwright-core` is an async CDP client on Tokio (one 57,000-line
`lib.rs`), bound to Python (about 96% of Playwright's sync API, claimed), Node (ten methods) and the `rustwright` crate, an
"idiomatic synchronous Rust API": callers need no Tokio. MIT. No driver subprocess. One anonymous `engine_launched` event per
process goes to PostHog (`us.i.posthog.com`) with a persistent random id, unless `RUSTWRIGHT_DISABLE_TELEMETRY`,
`DISABLE_TELEMETRY` or `DO_NOT_TRACK` is set. The spike runs did not set it, so up to six `engine_launched` events may have left this sandbox (whether the egress proxy let
them through was not checked).

## Method

Same machine and Chromium (Playwright's build 1194, headless, `--no-sandbox`), a local page with two inputs, a button that
writes into a paragraph and 200 list items. Flow: open a page, goto, fill name and email, click, read the paragraph, count
the items, screenshot, close. Cold: from nothing to a loaded page, five runs. Steady: 30 iterations, medians.

- rustwright: the `rustwright` 0.3.0 crate, in process (`rwbench`, source below), one Chromium shared by the 30 pages.
- `wf browser`: the SDK to the daemon (`bench-wf.mjs`, source below), device preset `desktop` (1440x900 at 1x), `plain` render
  mode, the release `wf` of this branch. `openBrowser` starts a whole Chromium per session (3 more processes per open were
  counted), `snapshot` is needed to get the refs `type` and `tap` take.
- Not equivalent step for step: rustwright's `new_page` is a tab on a running browser, `wf browser`'s `open` is a browser.
  Compare cold start to cold start, and the actions to the actions.

## Results (x86_64 sandbox, medians, ms)

| Step | rustwright crate | `wf browser` via the SDK |
| --- | --- | --- |
| Cold: launch, first page, goto (rustwright) / daemon start, open, navigate (wf) | 354 (350 to 409) | 676 (649 to 781) |
| The browser and first page without the daemon (steady medians of `open` and `navigate`) | 354 | 433 (373 + 60) |
| New page on a running browser | 52.7 | not comparable (open is 373) |
| goto | 26.3 | 60.3 |
| Fill two inputs | 67.9 | 23.4 (two `type`) + 15.1 (`snapshot` for the refs) |
| Click | 49.3 | 25.9 |
| Read text | 4.6 | 3.2 |
| Evaluate | 1.1 | 3.1 |
| Screenshot | 24.7 | 100.0 |
| Close | 1.0 (page) | 57.4 (browser) |
| Client process resident set | 11 MB | 60 MB (daemon) |
| Errors over 30 iterations | 0 | 0 |

## What it says

- rustwright is faster where our engine is slow: page creation, goto, screenshot (4x) and the cold launch (about 80 ms of 433),
  and its client is small. Part of that is our design (a Chromium per `open`, device emulation, a 1440x900 viewport), and the
  screenshot gap is worth chasing in `clis/cdp` on its own.
- It is slower on actions: each fill takes about 34 ms and a click about 49 ms (Playwright-style actionability waits), against
  about 12 ms per `type` and 26 ms per `tap` here.
- Neither run failed in 30 iterations, so both are stable on this flow.

## What it cannot do for us yet

The `rustwright` crate's `Page` offers goto, reload, back, forward, click, dblclick, drag and drop, fill, type, press key,
select, hover, check, scroll, evaluate, screenshot, console and network records, dialogs, file choosers, a viewport size and
`connect_over_cdp` with headers. `wf browser` has about 35 commands. Not in the crate (a few exist in `rustwright-core`, which
is not an idiomatic or stable API; `cookies` and geolocation were seen there, the rest was not found):

| `wf browser` needs | rustwright crate 0.3.0 |
| --- | --- |
| Device presets: metrics, scale factor, mobile and touch emulation, orientation, colour scheme (`open`, `set-device`) | viewport size only |
| Init scripts for the fake microphone and camera, the tap indicators, the opened-URL hook (`audio-inject`, `camera-inject`) | none |
| Touch gestures (`swipe`, `stroke`, `tap` on touch) | none |
| Permissions, clipboard, cookies (`permission`, `clipboard`, `cookies`) | cookies in core only |
| JS and CSS coverage, heap and memory, traces (`coverage`, `memory`, `trace`) | none |
| Screencast video (`video-check`, `--video`) | none |
| Network proxy per session, HTTP credentials, the tailnet forward | a launch proxy option only |
| A raw CDP call and the screencast and pause handlers `wf visual` and the VNC desktop launcher use | `ws_endpoint()` only |
| Accessibility snapshot with refs (`snapshot`, `tap --ref`) | not in the crate |

The Node binding bridges ten methods (launch, newPage, goto, click, fill, title, textContent, evaluate, screenshot, close; no
contexts, locators, routes or `connect_over_cdp`), so it replaces nothing in `backend/leetcode-auth-cookie-capture` or the
frontend specs.

## Found on the way: a regression in the SDK's call shape

The SDK runs every command as `wf browser --json <command>`. The release binary refused that shape ("does not support these
arguments"), so `startDaemon()` failed. The cause is mine: phase 4d replaced `lift_globals`, which lifted `--json` and `--app`
from anywhere, with a parse that only reads them before the command or after its words, and no test sent the options between
the command and its first word. Fixed with `args::lift_leading_options` (unit tests and four black-box tests first, red on the
old binary): `wf <command> --json <word>` and `wf <command> --app <name> <word>` are served again.

## Recommendation

Do not swap `clis/cdp` onto rustwright now: it is alpha, its idiomatic crate covers only part of what the CLI and the VNC desktop
launcher use (the table above), the missing parts (device emulation, init scripts, CDP access) are the ones the runbooks lean on, and it sends
telemetry unless told not to. Worth doing instead:

1. Chase our own slow steps with these numbers (screenshot 100 ms, `open` 373 ms, goto 60 ms), in `clis/cdp`.
2. Re-run this harness when rustwright ships contexts, init scripts and a CDP session in the Rust crate; any use must set
   `RUSTWRIGHT_DISABLE_TELEMETRY=1`.
3. Keep Playwright where it is (the capture service, the `playwright test` specs); rustwright's Node binding cannot stand in.

## Reproducing

`cargo add rustwright@0.3.0 serde_json` in a scratch crate with the source below, a page server on port 8099 serving the
local page, `CHROME_BIN` pointing at a Chromium; for the daemon side, `WF_BIN=<release wf> node bench-wf.mjs`.

### rwbench (Rust, `rustwright` 0.3.0)

```rust
// The runbook-like flow on the rustwright 0.3.0 Rust crate (sync API): cold start (launch, first page, goto, close) and
// steady state per step, with the client process's resident set.
use rustwright::{chromium, ActionOptions, GotoOptions, LaunchOptions, ScreenshotOptions};
use std::time::Instant;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}
fn rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1).and_then(|n| n.parse::<u64>().ok())))
        .map_or(0, |kb| kb / 1024)
}
fn opts() -> LaunchOptions {
    LaunchOptions::default().headless(true).executable_path(std::env::var("CHROME_BIN").unwrap_or("/opt/pw-browsers/chromium-1194/chrome-linux/chrome".into())).arg("--no-sandbox")
}

fn main() -> rustwright::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let url = args.get(1).cloned().unwrap_or("http://127.0.0.1:8099/form.html".into());
    let cold_runs: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(5);
    let loops: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(30);
    let mut cold = vec![];
    for _ in 0..cold_runs {
        let t = Instant::now();
        let b = chromium().launch(opts())?;
        let p = b.new_page()?;
        p.goto(&url, GotoOptions::default())?;
        cold.push(t.elapsed().as_secs_f64() * 1000.0);
        b.close()?;
    }
    let browser = chromium().launch(opts())?;
    let names = ["newPage", "goto", "fill", "click", "textContent", "evaluate", "screenshot", "close"];
    let mut steps: Vec<Vec<f64>> = vec![vec![]; names.len()];
    let mut errors: Vec<String> = vec![];
    for i in 0..loops {
        let mut lap = |idx: usize, t: Instant| steps[idx].push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let page = match browser.new_page() { Ok(p) => p, Err(e) => { errors.push(format!("iteration {i}: new_page {e}")); continue } };
        lap(0, t);
        let t = Instant::now();
        if let Err(e) = page.goto(&url, GotoOptions::default()) { errors.push(format!("iteration {i}: goto {e}")); continue }
        lap(1, t);
        let t = Instant::now();
        let r = page.fill("#name", "Ada", ActionOptions::default()).and_then(|_| page.fill("#email", "ada@example.com", ActionOptions::default()));
        if let Err(e) = r { errors.push(format!("iteration {i}: fill {e}")); continue }
        lap(2, t);
        let t = Instant::now();
        if let Err(e) = page.click("#submit", ActionOptions::default()) { errors.push(format!("iteration {i}: click {e}")); continue }
        lap(3, t);
        let t = Instant::now();
        let text = page.text_content("#out", ActionOptions::default())?;
        lap(4, t);
        let t = Instant::now();
        let rows = page.evaluate("document.querySelectorAll('li').length", None, ActionOptions::default())?;
        lap(5, t);
        if text.as_deref() != Some("Hello Ada") || rows != serde_json::json!(200) {
            errors.push(format!("iteration {i}: out={text:?} rows={rows}"));
        }
        let t = Instant::now();
        let png = page.screenshot(ScreenshotOptions::default())?;
        lap(6, t);
        if png.len() < 1000 { errors.push(format!("iteration {i}: screenshot {} bytes", png.len())); }
        let t = Instant::now();
        page.close(Default::default())?;
        lap(7, t);
    }
    let rss = rss_mb();
    browser.close()?;
    let mut out = serde_json::Map::new();
    out.insert("engine".into(), "rustwright-rust".into());
    out.insert("coldMedian".into(), serde_json::json!((median(cold.clone()) * 10.0).round() / 10.0));
    out.insert("coldAll".into(), serde_json::json!(cold.iter().map(|x| x.round()).collect::<Vec<_>>()));
    let mut s = serde_json::Map::new();
    for (n, v) in names.iter().zip(steps) { if !v.is_empty() { s.insert((*n).into(), serde_json::json!((median(v) * 100.0).round() / 100.0)); } }
    out.insert("steps".into(), s.into());
    out.insert("clientRssMb".into(), rss.into());
    out.insert("errors".into(), errors.into());
    println!("{}", serde_json::Value::Object(out));
    Ok(())
}
```

### bench-wf.mjs (Node, the SDK)

```js
// The same runbook-like flow through the sanctioned route: the SDK (@lib/browser-client) to the wf browser daemon, which runs
// on the Rust clis/cdp. Cold: daemon start, open, navigate, close, stop. Steady: per command, medians over the loop.
import { performance } from 'node:perf_hooks';
import { execSync } from 'node:child_process';
import { createContainer } from '/home/user/expo-hello-world-app-2/libraries/browser-client/src/index.mjs';

const [urlArg, coldArg, loopsArg] = process.argv.slice(2);
const URL_ = urlArg ?? 'http://127.0.0.1:8099/form.html';
const COLD = Number(coldArg ?? 5), LOOPS = Number(loopsArg ?? 30);
const CHROME = process.env.CHROME_BIN ?? '/opt/pw-browsers/chromium-1194/chrome-linux/chrome';
const ms = () => performance.now();
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
const rssMb = (pattern) => {
  try { return Math.round(execSync(`ps -eo rss=,args= | grep -E '${pattern}' | grep -v grep | awk '{s+=$1} END {print s+0}'`).toString().trim() / 1024); } catch { return 0; }
};
const out = { engine: 'wf-browser (Rust cdp, via the SDK)', steps: {}, errors: [] };
const open = (c, name) => c.openBrowser({ name, headless: true, devicePreset: 'desktop', renderMode: 'plain' });
const cold = [];
for (let i = 0; i < COLD; i++) {
  const c = createContainer({ headless: true });
  const t = ms();
  await c.startDaemon({ browserExecutablePath: CHROME });
  await open(c, 'cold');
  await c.navigate({ name: 'cold', url: URL_ });
  cold.push(ms() - t);
  await c.closeBrowser({ name: 'cold' });
  await c.stopDaemon();
}
const c = createContainer({ headless: true });
await c.startDaemon({ browserExecutablePath: CHROME });
const steps = { open: [], navigate: [], snapshot: [], type: [], tap: [], textContent: [], evaluate: [], screenshot: [], close: [] };
const refOf = (text, re) => text.match(re)?.[1];
for (let i = 0; i < LOOPS; i++) {
  const name = `b${i}`;
  try {
    let t = ms(); await open(c, name); steps.open.push(ms() - t);
    t = ms(); await c.navigate({ name, url: URL_ }); steps.navigate.push(ms() - t);
    t = ms(); const snap = await c.snapshot({ name }); steps.snapshot.push(ms() - t);
    const text = String(snap.yaml ?? '');
    const nameRef = refOf(text, /textbox "Name" \[ref=(e\d+)\]/), mailRef = refOf(text, /textbox "Email" \[ref=(e\d+)\]/), btn = refOf(text, /button "Submit" \[ref=(e\d+)\]/);
    if (!nameRef || !mailRef || !btn) { out.errors.push(`iteration ${i}: refs ${nameRef} ${mailRef} ${btn}`); await c.closeBrowser({ name }); continue; }
    t = ms(); await c.type({ name, ref: nameRef, text: 'Ada' }); await c.type({ name, ref: mailRef, text: 'ada@example.com' }); steps.type.push(ms() - t);
    t = ms(); await c.tap({ name, ref: btn }); steps.tap.push(ms() - t);
    t = ms(); const r1 = await c.eval({ name, expression: "document.getElementById('out').textContent" }); steps.textContent.push(ms() - t);
    t = ms(); const r2 = await c.eval({ name, expression: "document.querySelectorAll('li').length" }); steps.evaluate.push(ms() - t);
    const dec = (v) => { try { return JSON.parse(v); } catch { return v; } };
    if (dec(r1.result) !== 'Hello Ada' || Number(dec(r2.result)) !== 200) out.errors.push(`iteration ${i}: out=${JSON.stringify(r1.result)} rows=${JSON.stringify(r2.result)}`);
    t = ms(); const shot = await c.screenshot({ name }); steps.screenshot.push(ms() - t);
    if (!shot) out.errors.push(`iteration ${i}: no screenshot result`);
    t = ms(); await c.closeBrowser({ name }); steps.close.push(ms() - t);
  } catch (e) { out.errors.push(`iteration ${i}: ${String(e).slice(0, 200)}`); try { await c.closeBrowser({ name }); } catch {} }
}
out.daemonRssMb = rssMb('wf.*browser-daemon|browser-daemon');
await c.stopDaemon();
for (const [k, v] of Object.entries(steps)) out.steps[k] = v.length ? Number(median(v).toFixed(2)) : null;
out.coldMedian = Number(median(cold).toFixed(1));
out.coldAll = cold.map((x) => Number(x.toFixed(0)));
console.log(JSON.stringify(out));
```
