# Harness API: implementation contract

The spec (`spec.md`) says what each command does. This file says how the code is shaped, so every stage builds against the same seams. Paths are under `clis/wf/src/` unless named otherwise.

## One table, one dispatcher, two front ends

- `device/api.rs` holds the command table, `pub static COMMANDS: &[Command]`. A `Command` has:
  - `name` (snake_case; the CLI verb is the same word in kebab-case) and `maestro` (the Maestro command it mirrors, or `None`);
  - `params: &[Param]`; a `Param` has `name` (snake_case in Lua, `--kebab-case` on the CLI), `kind` (`Str`, `Int`, `Bool`, `Sel`, `Point`, `Path`, `Lua`, `Enum(&[..])`), `default`, `positional` and `doc`;
  - `platforms` (`Both`, `Android`, `Browser`, plus `why` for a one-platform command);
  - `guarded` (the Android lease guard runs before it);
  - `action` (`true` for anything that changes the screen or the device; a condition may not call an action);
  - `surface` (`Both`, `LuaOnly`, `CliOnly`);
  - `result` (a one-line schema) and `doc`.
  The table is the only list of commands. These are generated from it:
  - `wf browser <verb> --help`, `wf android <verb> --help`, `wf <noun> --help`;
  - `wf <noun> test --api`;
  - the Lua binder;
  - the parity flow test, which reads the table through `wf <noun> test --api --json`.
- `device/commands.rs` has the dispatcher: `pub fn call(dev: &dyn Device, ctx: &Ctx, name: &str, args: &Args, cond: Option<&dyn Fn() -> Result<Option<Value>, String>>) -> Result<Value, Failure>`.
  - `Args` is a `serde_json::Map` keyed by param name, already checked against the table. An unknown key, a wrong kind or a missing required param is a usage error, never ignored.
  - `ctx` carries `optional`, `label`, the test output dir (for evidence), the input log path and whether the call runs inside a condition.
  - Generic logic lives here, once, for both platforms:
    - the implicit wait of `tap_on` and `draw_strokes_on`;
    - `extended_wait_until`, `assert_visible` and `assert_not_visible`;
    - `scroll_until_visible` (a loop over `swipe` and `get_element`);
    - `get_pixel` (over `take_screenshot` and `crate::golden::decode`);
    - `assert_screenshot` (over `device/golden.rs`);
    - optional semantics and evidence collection.
- `device/cli.rs` is the CLI front end. `wf browser <verb>` and `wf android <verb>` route here when the verb is in the table.
  - It parses flags from the table, picks the device (`--device`, or the only one), runs the lease guard for guarded Android verbs, calls `commands::call` and prints the envelope.
  - `--condition '<expr>'` runs in a fresh read-only Lua state (`device/lua.rs` exposes `pub fn condition(dev, expr) -> impl Fn() -> Result<Option<Value>, String>`).
- `device/lua.rs` is the Lua front end. One method per table entry converts its Lua arguments to `Args` (positional, then an options table) and calls `commands::call`.
  - `extended_wait_until{condition=fn}` passes the Lua function as `cond`.
  - While `cond` runs, `ctx.in_condition` is set and an `action` command raises `a condition must not act: <name>`.
  - The old method names exist only as `renamed: use device:<new>` errors (and, until step 7 lands, as thin wrappers over the new commands so the old scripts keep running); step 8 removes them, after which an old name is no method and calling it fails with Lua's own error at the script line.

## Device trait (device/mod.rs)

Drivers do platform work only: no waiting policy, no evidence policy, no option parsing.

```rust
pub struct Rect { pub left: i64, pub top: i64, pub right: i64, pub bottom: i64 }   // the on-screen part, tap units
pub struct Element { pub bounds: Rect, pub text: Option<String>, pub frame: &'static str /* "app" | "editor" | "native" */ }
pub enum Part { Screen, Keyboard, Microphone }
pub trait Device {
    fn platform(&self) -> &'static str;                                  // "android" | "browser"
    fn open_link(&self, link: &str) -> Result<(), String>;
    fn tap_at(&self, x: f64, y: f64) -> Result<(), String>;              // tap units; browser draws the tap dot
    fn tap_element(&self, el: &Element, sel: &Sel) -> Result<(f64, f64), String>; // inside the on-screen part; returns the point
    fn swipe(&self, from: (f64, f64), to: (f64, f64), duration_ms: u64) -> Result<(), String>;
    fn input_text(&self, text: &str) -> Result<(), String>;              // refuses when the keyboard is down
    fn press_key(&self, key: Key) -> Result<(), String>;                 // Key: Enter|Backspace|Tab|Left|Right|Up|Down|CtrlEnd
    fn back(&self) -> Result<(), String>;                                // android; refuses while the keyboard is up
    fn draw_strokes_on(&self, el: &Element, sel: &Sel, source: &str, human: bool) -> Result<Value, String>;
    fn set_microphone_input(&self, path: &str) -> Result<Value, String>;
    fn run_shell(&self, cmd: &str) -> Result<(String, i32), String>;     // android
    fn take_screenshot(&self, path: &str) -> Result<(u32, u32), String>; // raw PNG at path
    fn start_recording(&self, path: &str) -> Result<(), String>;
    fn stop_recording(&self) -> Result<Vec<String>, String>;
    fn get_element(&self, sel: &Sel) -> Result<Option<Element>, String>; // one look; None unless visible
    fn count_matches(&self, sel: &Sel) -> Result<usize, String>;         // ignoring visibility, for evidence
    fn get_device_state(&self, parts: &[Part]) -> Result<Value, String>; // only the parts asked for are read
    fn get_logs(&self, level: Level) -> Result<Vec<String>, String>;
    fn clear_logs(&self) -> Result<usize, String>;
    fn hierarchy(&self) -> Result<Value, String>;
    /// Event-driven where the platform can: resolves when the selector's visibility equals `want`, or after `ms`.
    fn wait_visibility(&self, sel: &Sel, want: bool, ms: u64) -> Result<bool, String>;
    // run state (Lua only)
    fn add_network_stub(&self, spec: &Value) -> Result<Value, String>;
    fn get_network_stub_hits(&self) -> Result<Vec<Value>, String>;
    fn get_debug_hits(&self) -> Result<Vec<Value>, String>;
    fn arm_logpoints(&self, points: &[Logpoint]) -> Result<(), String>;  // test --logpoint, before the first page or attach
    fn debug_status(&self) -> Option<Value>;
    fn summary(&self) -> Option<Value>;
    fn shutdown(&self);
}
```

`get_device_state` returns this shape, with only the requested keys:
- `screen`: `{width, height, dpr}`
- `keyboard`: `{visible, bounds}` (`bounds` is null when the keyboard is down)
- `microphone`: `{input, consumed}`

`Element.text` is:
- for an input, a textarea, an Android EditText, a contenteditable, or a web view hosting one: the field's value; the editor buffer's lines are joined with `\n`;
- otherwise the visible text (browser `innerText` trimmed, Android node text);
- otherwise the accessibility label.

## Selector (device/sel.rs)

- `pub struct Sel { pub id: Option<Pattern>, pub text: Option<Pattern>, pub css: Option<String>, pub point: Option<Point> }`.
- A `Point` is two `Coord::{Px(f64), Pct(f64)}` values, parsed from `"x,y"` or `"x%,y%"` and resolved against the screen size.
- `Pattern::parse(src)` checks the shared grammar with the `regex-syntax` crate, and refuses everything outside it (spec, selector section).
- Each engine gets its own form: `Pattern::java()` (for the UiSelector), `Pattern::js()` (a source plus flags `imsu`) and `Pattern::is_match(&str)` (the Rust engine).
- Matching is Maestro's rule: case-insensitive, dot matches newline, multiline, a full match OR literal equality, and either of those after each newline in the value is read as a space.
- `Sel::from_lua(Value)` and `Sel::from_cli(&Args)`. A bare string means `{text=...}`. A string starting with `id:`, `text:`, `css:`, `xy:` or `label:` is refused with `old selector form: use {id='X'}`.
- `point` combined with another field is a usage error. `css` on Android is a usage error.

## Failure and the envelope

- `pub struct Failure { pub error: String, pub usage: bool, pub evidence: Option<Value>, pub verdict: Option<&'static str>, pub met_after_ms: Option<u64>, pub next: Option<String> }`.
- `commands::evidence(dev, ctx, sel?)` collects:
  - `screenshot` and `hierarchy` files in the test output dir (`wait-timeout-<ms>.png` and `.hierarchy.json`);
  - `element` state and `matches` (`count_matches`);
  - `keyboard_visible`;
  - the last 3 warn-or-above `logs`;
  - for a condition, its `last_value` (capped at 2 KB) and `checks`.
- In Lua a failure raises `<file>:<line>: <command>{...} '<label>' <error> -- <verdict>; evidence: <json path>`. The evidence JSON is written beside the screenshot and listed in report.json.
- On the CLI, every browser and android device verb prints one line and exits with:
  - success: `{"ok":true,"command","platform","device","ms","result"}`, exit 0;
  - failure: `{"ok":false,...,"error","evidence","verdict","met_after_ms","next"}`, exit 1;
  - usage error: `{"ok":false,"error","usage"}`, exit 2;
  - lease conflict: exit 3.
- `--json` is accepted and ignored. Only `wf browser` and `wf android` change. `start`, `status` and `stop` on the browser keep `result.url`.

## Waits (device/commands.rs)

- `extended_wait_until` takes exactly one of `visible`, `not_visible` and `condition`, plus `timeout` (ms), `interval` (ms; condition only, default 200, minimum 50), `optional` and `label`.
- The default timeout is `wait_budget_ms()` (`WF_WAIT_BUDGET_MS`, 10000). With `optional` and no timeout, it is `optional_lookup_ms()` (`WF_OPTIONAL_LOOKUP_MS`, 1000).
- `visible` and `not_visible` call `dev.wait_visibility`.
- `condition` calls `cond` at once, then again `interval` ms after each return (sooner on the browser when the app's DOM changes, but not within 50 ms), until it returns a truthy value.
- On a miss:
  - optional: return `Value::Null`, with span outcome `miss` and no evidence;
  - otherwise, when the timeout is above 0, keep checking for `WF_WAIT_DIAGNOSE_MS` (8000) more to set `verdict` to `slow` or `broken` and `met_after_ms`, then fail with evidence.
- What a wait returns:
  - visible: the element (`{bounds:{left,top,right,bottom,cx,cy}, text}`);
  - not_visible: `true`;
  - condition: its first truthy value.
- `assert_visible(sel, {timeout, optional, label})` and `assert_not_visible(...)` are the same code path, under their own span names.
- Spans are `extended_wait_until:visible|not_visible|condition`, with `:optional` added when it is set. `WF_WAIT_LOG` lines are `{call, sel|label, ms, ok, timeoutMs, budgetMs, platform}`.
- The run-stop check (`crate::run_stop::check`) runs on every loop turn.

## Drivers

- **Browser (device/browser.rs)** talks to the daemon in process: a `DaemonClient { url, session }` posting `/call {method, args}` (the same client `browser.rs` uses, made `pub(crate)`). No `wf browser` child runs, and `WF_BROWSER_CMD` is gone.
  - A test session is opened by the first `open_link` (start-device semantics, with `owner = <pid>`).
  - A CLI verb attaches to an existing session. It refuses a session with an owner unless `--device` names it.
  - Selectors resolve in page JS with no writes to the DOM.
  - Bounds are clipped to the screen and to clipping ancestors.
  - The app page is searched first, then same-origin iframes.
- **Android (device/android.rs)** calls typed functions in `android_input.rs` and `android_u2.rs` (refactored out of their CLI parsers). No `wf android` child runs.
  - Selectors go to the UiSelector `textMatches`, `descriptionMatches` and `resourceIdMatches` with `Pattern::java()`. The dump fallback (`WF_ANDROID_UIA=dump`) uses `Pattern::is_match`.
  - `wait_visibility` asks again 100 ms after each answer.
- **Lease**: `android_lease::guard()` runs before every `guarded` command, in process, and renews the lease. `wf android test` also renews it from a thread every 60 s. `start-device` and `stop-device` are never guarded.

## Daemon RPC (additive only)

- `tap` accepts `{x, y}` beside `ref`.
- `swipe` accepts `durationMs`.
- New: `consoleLogs {level?, clear?}`, filled per session from CDP `Runtime.consoleAPICalled` and `Runtime.exceptionThrown` from open.
- New: `recordStart {path}` and `recordStop`.
- `audioInject` accepts `replace: true`.
- `open` accepts `owner`.
- Every existing method and argument keeps working: @lib/browser-client, the cookie-capture service and wf bench call them.

## Tests (clis/wf/tests/flow)

- `support/mock_daemon.rs` records `/call` requests. The browser cases in `harness.rs` use it with `WF_BROWSER_NO_START`.
- New flow tests:
  - parity across both CLIs and the Lua binder;
  - the regex grammar on three engines, with the literal fallback;
  - a swipe;
  - the lease renewed across a long read-only wait;
  - a test-owned session refused;
  - a clipped tap point;
  - a condition that acts raises;
  - an optional miss returns nil with no evidence;
  - `get_element({id='CodeEditorInput'}).text` is the buffer.
