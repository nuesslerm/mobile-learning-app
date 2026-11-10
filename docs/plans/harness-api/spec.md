# Harness API: one Maestro-shaped surface for the browser and redroid

Owner request, 20 Oct 2026: keep only the device commands that are used or needed; let the wait take an optional callback, as Maestro's `extendedWaitUntil` and Cypress's `waitUntil` do, so special waits such as audio-taken go; give reads explicit names (`pixel` becomes `get_pixel`); and make the surface Maestro's, the same on the browser as on the redroid emulator.

This file is the user-facing spec: what each command does on each platform, what it replaces, who calls it, and what is dropped and why. The code shape (the command table, the dispatcher, the `Device` trait, the selector type, the failure and the envelope) is in [contract.md](contract.md); the two agree, and the contract decides how the code is built. The ledger is the Harness API block of [progress.md](../after-device-lab/progress.md); the goal criteria are h0 to h8 in [the after-device-lab goal](../../goals/2026-10-20-after-device-lab.md).

Today the harness has 46 Lua methods, 49 `wf browser` verbs and 31 `wf android` verbs. It has nine wait methods with four timeout conventions (`wait` drops its seconds, `appears` is listed twice in the help, `wait_editor_longer` reads as "a longer wait"), bare nouns used as verbs (`pixel`, `text`, `find`, `trace`), three waits whose result is discarded so they can never fail a run, and two CLIs with different verbs. After this change there are 23 device methods in Lua and the same 22 device verbs on `wf browser` and `wf android`.

Caller counts come from the usage count of 20 Oct over `e2e/*.lua`, CI, runbooks, skills and goal verifiers; `file:line` references are to the tree of that day. The CI-run scripts are app-routes, editor-typing, stubbed-run-local, tamil-writing and voice-edit, plus warm-app, the Android warm-up that `wf test run` adds itself. agent-proof, debugger-probe, metro-smoke, stop-probe and web-goldens are manual or bench only.

## Owner decisions, 2026-10-20

| Question | Answer |
|---|---|
| "A wait for a camera callback" | The point was the callback wait, not a camera: no single-purpose verbs such as audio-taken; every special wait folds into `extended_wait_until{condition=fn}` "in a maximally generic way". No camera feed is built: no screen uses one. Conditions read state through two generic reads, `get_element` for whatever is on screen and `get_device_state` for whatever the device holds. They replace the draft's `get_bounds`, `is_visible`, `get_text`, `get_editor_text`, `is_keyboard_visible`, `get_keyboard_bounds`, `get_screen_size` and `is_microphone_input_consumed`. A new kind of state becomes a field, never a new verb. |
| Default timeouts: Maestro's 17000 ms and 7000 ms, or the repo's budget | The repo's budget: `WF_WAIT_BUDGET_MS` (10000) by default, and `WF_OPTIONAL_LOOKUP_MS` (1000, new) for optional lookups. |
| `assertVisible` and `assertNotVisible`, or a single wait | Both names: `assert_visible(sel)` and `assert_not_visible(sel)` exist as shorthands over `extended_wait_until`'s `visible` and `not_visible`, with the same defaults and the same code path, under their own span names. |
| The interactive debugger | Kept, browser only: `set-breakpoint`, `debug-pause`, `debug-step`, `debug-eval` and `debug-resume`. Each verb's help says why Android has only logpoints. |

The drops under [Dropped](#dropped) were approved with the plan.

## Principles

**Maestro's names, one per action.** A Maestro flow command becomes a Lua method in snake_case (`tapOn` is `device:tap_on`) and a CLI verb in kebab-case (`tap-on`). The verb is the same word on `wf browser` and `wf android`, takes the same parameters and returns the same JSON answer. Parameters keep Maestro's names: `visible`, `not_visible`, `timeout`, `duration`, `direction`, `visibility_percentage`, `threshold_percentage`, `optional` and `label` (snake_case in Lua, kebab-case on the CLI). A Maestro name is borrowed only when our command does what Maestro's does: Maestro's `addMedia` puts a list of files in the gallery and `copyTextFrom` fills a variable, so the microphone feed and the element read get names of their own.
- 16 of Maestro's 48 flow commands are taken: openLink, tapOn, swipe, scrollUntilVisible, inputText, pressKey, back, extendedWaitUntil, assertVisible, assertNotVisible, takeScreenshot, assertScreenshot, startRecording, stopRecording, runFlow and assertTrue.
- The infra verbs take Maestro CLI's names (App.kt:61-78): `test`, `start-device`, `list-devices` and `hierarchy`, and its flags `--device`, `--env`, `--test-output-dir` and `--screen-size` (TestCommand.kt).
- 11 commands are our own and follow Maestro's patterns: `<verb>_on(sel)` for an element action (`draw_strokes_on`); `set_` for fake device input, like setLocation (`set_microphone_input`); `add_`, `clear_`, `start_` and `stop_` for state; `get_` for a read. There is no `is_` read: a boolean is a field of a read (`get_device_state('keyboard').visible`).

**Only what is used or needed.** A command stays when any of these call it: `e2e/*.lua`; CI (`.github/workflows`, `e2e_run.rs`, `agent_proof.rs`, app_bench); a runbook or skill procedure; a goal verifier in `docs/goals/*.md`; or a deployed client (`@lib/browser-client`, the cookie-capture service). It also stays when a CLAUDE.md rule needs it: the debugger, recordings, goldens, and the keyboard moving only by user action. A command's own flow test, help text, `docs/reference` entry and the skill's verb table do not count. Everything else is under [Dropped](#dropped), with its reason. A dropped CLI verb keeps its daemon RPC method when a client calls it.

**Two generic reads.** `get_element(sel)` covers whatever is on screen: it returns `{bounds, text}`, or nil when the element is not visible, and its text is also an input's value and the code editor's buffer (the text of a field, or of a web view hosting one, is its value). `get_device_state([part])` covers whatever the device holds: `screen`, `keyboard` and `microphone`. Four more reads have one job each and no generic home: `get_pixel` (what the accessibility tree cannot see), `get_logs`, and the run-state reads `get_network_stub_hits` and `get_debug_hits`.
- A read returns a value, or nil (on the CLI, a null result with exit 0) when the thing is not there. Absence is an answer, not a failure.
- Reads never wait, never act, take no clear flag and change nothing on the device or in the page. On the browser, selectors resolve in page JS without writing attributes. Today's text lookup tags the DOM with `data-wf-text-target` (`device/browser.rs:152-161`), which fires `waitObserve`'s attribute observer and the app's own observers on every retry of a condition.
- No bare noun is a verb: `pixel` becomes `get_pixel`; `find`, `text` and `editor_text` become `get_element`; `keyboard_shown` becomes `get_device_state('keyboard').visible`; `warnings` becomes `get_logs`; `stub_hits` becomes `get_network_stub_hits`; `debug_hits` becomes `get_debug_hits`; `shell` becomes `run_shell`; `trace` becomes `draw_strokes_on`; `audio_inject` becomes `set_microphone_input`.

**One wait.** `device:extended_wait_until{visible= | not_visible= | condition=, timeout, interval, optional, label}` replaces the nine wait methods, the draft's audio-taken verb and every sleep poll loop. A condition is a read-only Lua callback, like `cy.waitUntil`'s checkFunction. The microphone wait the owner asked for is `extended_wait_until{condition=function() return device:get_device_state('microphone').consumed end}`. `assert_visible` and `assert_not_visible` are Maestro's names for the two selector modes of the same wait. The only other waits are Maestro's implicit ones: `tap_on` and `draw_strokes_on` wait for their element, and `scroll_until_visible` scrolls until its element shows.

**Milliseconds, and no silent drop.** Every duration is in milliseconds, as in Maestro and Cypress: timeout, interval, duration and delay. The migration multiplies today's second-based arguments by 1000. `sleep(seconds)` becomes `sleep_ms(ms)` and the old name raises, so no call changes unit silently. An argument a platform cannot honour is a usage error (exit 2, or a Lua error), never ignored. That ends today's silent drops: `wait`'s secs, the browser swipe's ms (`device/browser.rs:291`), `record_stop`'s out, Android's debug defer, `screenshot(path)`'s opts, the stub's content_type, `--identifier`, type's `--delay`, and `wait_editor_longer`'s capped secs.

**One implementation per platform, in process.** Each command is one Rust function per driver, with typed arguments, returning a result or a failure with evidence and a next step. The CLI and the Lua binder both call it in the same process, so a Lua call and its CLI verb cannot drift apart.
- The browser driver posts to the daemon's `/call` itself instead of spawning `wf browser <verb>`. That removes `WF_BROWSER_CMD`, and the flow tests that used it move to `tests/flow/support/mock_daemon.rs`.
- The Android driver talks to adb and the uiautomator2 server itself instead of spawning child verbs.
- The same layer runs the lease guard before every lease-guarded command, and the guard renews the lease. Today a lease is renewed only when a child `wf android` verb runs (`android_lease.rs:147-151`), so moving the calls in process would otherwise stop the heartbeat.

**One envelope.** Every `wf browser` and `wf android` verb prints exactly one JSON object:
- success: `{ok:true, command, platform, device, ms, result}`, exit 0;
- failure: `{ok:false, command, platform, device, ms, error, evidence:{screenshot, hierarchy, element, matches, keyboard_visible, logs}, verdict, met_after_ms, next}`, exit 1;
- usage error: `{ok:false, error, usage}`, exit 2;
- Android lease conflict: exit 3, naming the holder and the release command.

Each verb's result schema is in its `--help` and in `test --api`. The envelope covers only these two nouns: `wf test`, `wf vm` and the other nouns keep their output, because `e2e-tests.yml:120-150` and the goal verifiers pipe it into jq. Today `wf android --json` spreads the result's fields at the top level (`json_mode.rs:75-82`); its readers move to `.result` in the same commit: `e2e-tests.yml:161`, `docs/goals/2026-10-14-device-lab-fast-green.md:18-19` and `warm-app.lua:41`. warm-app writes only the device entry, so the published `device-features.json` keeps the shape that `docs/goals/2026-10-13-device-test-process.md` a1 and a3 read. Three exceptions are documented:
- `test` streams its log lines and its `report:`, `bundle:`, `recording:` and `goldens:` lines, which `e2e_run.rs`, `agent_proof.rs` and `harness.rs` parse, and ends with one JSON summary line;
- `adb` passes raw output through;
- `tunnel` prints one JSON line and stays in the foreground.

`--json` stays as a no-op flag, before or after the verb, because `@lib/browser-client` and cookie-capture send it.

**Settle.** `tap_on` and `draw_strokes_on` act only once the element's box holds still across two looks 100 ms apart, at most `WF_SETTLE_MS` (1000 ms), as Maestro waits for the screen to settle before a tap. On the browser the lookup, the looks and the re-measure run in the page as one `waitObserve`, while the daemon keeps ticking. In-process calls are fast enough to tap a dialog that is still animating in, which Android ignored (CI run 39519761372, voice-edit chunk 1).

**optional and label.** Every action and every wait takes Maestro's shared attributes. `optional=true` turns a failure into a nil return, logged as an optional miss with no evidence and no diagnosis look; it replaces `pcall` around a device call. As in Maestro, an optional lookup has its own, shorter budget: `WF_OPTIONAL_LOOKUP_MS`, 1000 ms by default, because today's `pcall(device:tap(X))` misses after one look (`android_input.rs:221-237`) and units are sized to stay under a minute. `label` names the step in report.json, in timing.jsonl and in the error.

**The keyboard moves only by user action.** Tapping a field with `tap_on` raises it, and tapping Done lowers it. hideKeyboard is not offered. `back` refuses while the keyboard is up, because on Android Maestro's hideKeyboard is that same key. `input_text` and the editing `press_key` keys refuse while the keyboard is down; they never focus a field themselves, whereas Maestro's inputText types without focus. Each refusal carries evidence and a next step: "tap the field first" or "tap Done". No other verb, option or daemon RPC shows or hides the keyboard. `capture-leetcode-shots.mjs`'s setKeyboard becomes a tap on Done, and the keyboard-toolbar runbook's eval focus and blur steps become a tap on the editor and a tap on Done.

**Maestro's matching rule on both platforms** (Orchestra.kt:1899, Filters.kt:58-131): case-insensitive, a dot matches a newline, a full match or literal equality, also with each newline in the value read as a space. `text` matches the element's text, its accessibility label or an input's value; a web view's buffer is its `get_element` text but is never matched, so the editor is found by id and its text read. [Selector](#selector) has the details, the regex grammar the three engines share and the two deliberate differences from Maestro.

**Visible means on screen.** An element is visible when its on-screen part is not empty, and bounds always mean that on-screen part. Android's node bounds already are. The browser clips `getBoundingClientRect` to the screen and to every ancestor whose overflow clips. A tap lands inside the on-screen part. Today's browser touch tap already clips this way and refuses an off-screen element (cdp `attached.rs:1220-1225`), so a toolbar key at x 380-436 of a 393-wide screen is tapped in its visible 13 px or not at all.

**One platform only where the platform really differs**, and the command's help says why. The other CLI lists the verb too and exits 2 with `{error:'not on <platform>: <why>'}`. The full list:
- `css` selectors: Android has no DOM.
- `run_shell`: a page has no OS shell.
- `back`: a page has no system Back key, and history back has no caller.
- Browser only: the daemon lifecycle verbs `start`, `status` and `stop`; `tunnel`; `tailnet-forward`; `check-taps`; the interactive debugger.
- Android only: `lease` and `adb`.

State that lives for one run (network stubs, logpoint hits) is Lua-only on both platforms, so both CLIs list the same device verbs.

**Targeting with Maestro's `--device <id>`.** On every device verb and on `test`, it picks the browser session name or the Android adb serial. It replaces `--name` (browser), `--serial` and `--identifier`, and defaults to the only device; with several open, a verb exits 2 and lists them.
- When a test opens a browser session, the session is marked with the test's pid through an additive `owner` argument on the daemon's `open` RPC. A verb refuses such a session unless `--device` names it, so a verb typed by hand never drives a running test; the browser has no lease to stop it.
- The Android container is not its serial. `start-device`, `stop-device`, `list-devices` and `lease` name the container with `--name` (default `rdl`, env `WF_ANDROID_NAME`, was `WF_ANDROID_DEVICE`), as `up --name` (`app_bench/android.rs:127`) and the lease (`android_lease.rs:143, 337`) already do.
- The browser's old `--device <preset>` becomes `--screen-size <preset>`. A preset id given to `--device` exits 2 and names `--screen-size`.

**Who may run what does not change.** Android device verbs run only in CI's android-e2e-tests job. Today the lease guard (`android.rs:106`) covers open, tap, swipe, text, install, launch, stroke, trace, record, type and press. It grows to every verb that changes the device: `tap-on`, `swipe`, `scroll-until-visible`, `input-text`, `press-key`, `back`, `open-link`, `draw-strokes-on`, `set-microphone-input`, `run-shell`, `start-recording`, `stop-recording`, `clear-logs` (it runs `logcat -c`) and `adb`. `start-device` and `stop-device` stay unguarded: a crashed unit's stale lease (30 min TTL, `e2e_run.rs:131`) must never make CI's `if: always()` teardown exit 3 and leave the 1.3 GB device up, and nobody holds a lease on a device that is not running. `wf android test` also renews its lease every 60 s while it runs, so a long stretch of reads (stop-probe waits 900 s) cannot let it lapse.

**The browser daemon's protocol stays frozen for its clients**: `wf browser-daemon`, `GET /health`, `GET /status`, `POST /stop` and `POST /call {method, args}`. Its lifecycle verbs `wf browser --json start|status|stop` keep `result.url`, because `@lib/browser-client` and the cookie-capture service call them (`browser_daemon.rs:53-57` in that service). That service runs the VM's shared wf build (its Dockerfile links `/opt/clis/current/wf`), so a rename would break the deployed service as soon as the clis job publishes, before the service itself redeploys. Only additive changes are allowed:
- `tap {x, y}`: today tap requires `ref` (`browser_daemon.rs:435-437`), so a point or a regex match cannot be tapped;
- a swipe `durationMs`;
- `consoleLogs {level?, clear?}`: a buffer per session, filled from CDP `Runtime.consoleAPICalled` and `Runtime.exceptionThrown` from session open, so no client's page is wrapped;
- `recordStart {path}` and `recordStop`;
- `audioInject` with `replace: true`;
- `open` with `owner`.

Screenshots keep today's RPC; the driver copies the daemon's file to the path the caller asks for.

**Every failure carries its evidence and the next step**, per the CLAUDE.md agent-first rule: a screenshot and a hierarchy dump in the test output dir, the element's state, the keyboard state and the last JS warnings. All of it goes into report.json and the run bundle.

## The commands at a glance

Lua names are device methods unless marked as globals. The CLI verb is the same on `wf browser` and `wf android`. An "action" may not run inside a condition; the Android lease guard covers the actions that change the device (see [Principles](#principles)).

| Lua | CLI verb | Maestro | Kind | Where | Replaces |
|---|---|---|---|---|---|
| `open_link(link)` | `open-link` | openLink | action | both | open; navigate |
| `tap_on(sel)` | `tap-on` | tapOn | action, waits for its element | both | tap; wait-then-tap pairs |
| `swipe(start, end_point, {duration})` | `swipe` | swipe | action | both | swipe, drag, scroll |
| `scroll_until_visible{element, direction, from, timeout}` | `scroll-until-visible` | scrollUntilVisible | action, loop | both | nine hand-written scroll loops |
| `input_text(text)` | `input-text` | inputText | action | both | type_text; android type and text |
| `press_key(key)` | `press-key` | pressKey | action | both | press; shell keycombination |
| `back()` | `back` | back | action | Android; the browser exits 2 | press('back') |
| `extended_wait_until{...}` | `extended-wait-until` | extendedWaitUntil, cy.waitUntil | wait | both | the nine waits; sleep polls |
| `assert_visible(sel)` | `assert-visible` | assertVisible | wait | both | wait, wait_slow |
| `assert_not_visible(sel)` | `assert-not-visible` | assertNotVisible | wait | both | wait_gone |
| `take_screenshot(path)` | `take-screenshot` | takeScreenshot | action | both | screenshot(path) |
| `assert_screenshot(path, {threshold_percentage, ignore})` | `assert-screenshot` | assertScreenshot | action, check | both | screenshot(name), golden, `--goldens` |
| `start_recording`, `stop_recording` (CLI only) | `start-recording`, `stop-recording` | startRecording, stopRecording | action | both | record start/stop; open --video |
| `get_element(sel)` | `get-element` | (Cypress cy.get) | read | both | find, text, editor_text |
| `get_device_state([part])` | `get-device-state` | (ours) | read | both | keyboard_shown, shell wm size and dumpsys window, wait_recording |
| `get_pixel(point)` | `get-pixel` | (ours) | read | both | pixel |
| `get_logs({level})` | `get-logs` | (ours) | read | both | warnings, logcat dumps |
| `clear_logs()` | `clear-logs` | (ours) | action | both | clear_warnings, shell logcat -c |
| `set_microphone_input(path)` | `set-microphone-input` | (ours, like setLocation) | action | both | audio_inject |
| `draw_strokes_on(sel, {source, human})` | `draw-strokes-on` | (ours, like tapOn) | action, waits for its element | both | trace |
| `run_shell(command)` | `run-shell` | (ours) | action | Android; the browser exits 2 | shell |
| `add_network_stub(method, url_pattern, opts)` | (Lua only) | (ours) | run state | both | stub |
| `get_network_stub_hits()` | (Lua only) | (ours) | read | both | stub_hits |
| `get_debug_hits()` | (Lua only) | (ours) | read | both | debug_hits |
| `run_flow(label, fn)`, a global | (none) | runFlow | step | both | device:step |
| `assert_true(value, label)`, a global | (none) | assertTrue | check | both | assert_that |

## Selector

In Lua a selector is a table of Maestro selector fields, or a bare string. On the CLI it is the same fields as flags, or one positional string.

### Fields

Every given field must hold: they are ANDed, as in Maestro.

| Field | Kind | Browser | Android |
|---|---|---|---|
| `id` | regex | `data-testid`, in the app page, then in the editor WebView | the node's resource-id, whole or the part after its last `/`, as Maestro's idMatches does (Filters.kt:109-131); this is React Native's testID |
| `text` | regex | the element's text (`innerText` trimmed; for input and textarea their value, because an input's innerText is empty), or its `aria-label` | the node's text (an EditText's text is its value), or its content-desc |
| `css` | string, not a regex | a DOM selector such as `.cm-line:last-child`, like Maestro's web-only css field | exits 2: there is no DOM |
| `point` | `'x,y'` in tap units, or `'x%,y%'` of the screen | a screen point, as in Maestro's tapOn point | the same |

- `point` is taken by `tap_on` only, and never with another field, because no script taps a point inside an element. Anywhere else it is a usage error.
- Maestro's text also covers hintText and its error text. Those are not taken (see the differences below).
- These Maestro fields are not taken, because nothing calls them: index, enabled, checked, focused, selected, above, below, leftOf, rightOf, containsChild, childOf, containsDescendants, traits, width, height and tolerance.

### Matching

Maestro's rule (Orchestra.kt:1899, Filters.kt:58-106).
- Flags: case-insensitive, dot matches newline, multiline. These are Maestro's IGNORE_CASE, DOT_MATCHES_ALL and MULTILINE.
- A field matches in any of three ways:
  - the pattern matches the whole value;
  - the pattern equals the value literally (Maestro's `regex.pattern == value`), so `{text='Run (local)'}` finds the text Run (local) even though its parentheses are regex syntax;
  - either of the above holds after each newline in the value is read as a space.
- When several elements match, the deepest one wins (Maestro's deepestMatchingElement): of the visible matches, the first in document order (browser) or tree order (Android) with no other visible match inside it. Visibility is applied first on both platforms, so a hidden match inside a visible one does not hide it. A failed wait's evidence reports how many matched: on the browser the deepest matches, since an element's `innerText` holds its children's; on Android every matching node, once.
- Two deliberate differences from Maestro:
  - Hint text (a placeholder) is not matched. The uiautomator2 UiSelector has no hint matcher, so on Android it would need a full hierarchy dump per lookup, and no script selects by placeholder.
  - Maestro's clickable-first ordering is not taken, because a tap on the deepest match lands on its clickable parent anyway.

### Regex grammar: one grammar, three engines

A pattern runs in one of three engines:
- Java on Android. Matching happens on the device, through the UiSelector's `textMatches`, `descriptionMatches` and `resourceIdMatches`, which already require a full match. That is today's primary path (`android_u2.rs:183-190`, which today uses `textContains` and `descriptionContains` for text).
- JavaScript on the browser.
- Rust on the `WF_ANDROID_UIA=dump` fallback only, because a full dump per lookup cannot keep a 100 ms poll.

Compiling every pattern with Rust's regex crate alone would not make a bad pattern the same error everywhere: `(?i)`, `[[:alpha:]]`, `\pL`, `(?P<n>..)`, `\A` and `\z` compile in Rust but are a SyntaxError in JavaScript, or mean something else there. So the harness parses each pattern with the regex-syntax crate, and anything outside the shared subset is a usage error on both platforms.

The subset allows:
- literals and `.`;
- classes `[...]` with ranges and `^`;
- `\d \w \s \D \W \S`;
- backslash escapes of the syntax characters `^ $ \ . * + ? ( ) [ ] { } | /`;
- quantifiers `* + ? {n} {n,} {n,m}` and their lazy forms;
- groups `(...)` and `(?:...)`;
- alternation;
- `^` and `$`.

It refuses lookaround, backreferences, named groups, inline flags, POSIX classes, `\p{...}`, `\b`, `\A` and `\z`.

From the parsed pattern P, the harness writes each engine's form, with the flags and the literal-equality alternative built in, and with every literal space also matching a newline:
- Java, for the UiSelector: `(?ism)(?:P)|\QP\E`;
- JavaScript: a RegExp with flags `imsu`, plus an equality check;
- Rust: an `(?ism)` regex.

A flow test runs the same patterns on all three engines.

### Shorthand

A bare string is `{text=<string>}`, as in Maestro: `device:tap_on('Continue')`. A string that starts with one of today's prefixes (`id:`, `text:`, `css:`, `xy:`, `label:`) raises `old selector form: use {id='X'}` instead of being read as text, so a rewrite the migration missed fails loudly.

### Visible

What `visible`, `not_visible`, `assert_visible`, `assert_not_visible`, `get_element` and the implicit waits test: the element exists and its on-screen part is not empty.
- Browser: the bounds are `getBoundingClientRect` clipped to the screen and to every ancestor whose overflow is not visible, and the element must pass `checkVisibility({visibilityProperty:true})`. The search covers the app page first and then every same-origin iframe (the editor WebView), in the same order for every field and every verb. Today `css:` searches only the WebView, and `text:` and `label:` only the app page. Today's `find` also returns a rect for any element in the DOM, hidden or off screen (`device/browser.rs:305-321`), so some browser branches flip; [Verification](#verification) lists them.
- Android: a node in the accessibility tree from the uiautomator2 server, with its visible bounds. Views left out of the tree, such as Expo Go's Reset dialog and the result toast, cannot be found by any selector; that is why `get_pixel` stays.

Tap units are CSS px on the browser and physical px on Android; `get_device_state('screen')` gives the size and dpr.

### CLI

The flags are `--id RE`, `--text RE`, `--css S` and `--point X,Y`; one positional string means `--text`. Where a verb takes two selectors, the flags carry a prefix: `scroll-until-visible --element-id ... --from-id ...`. `extended-wait-until` takes `--visible` or `--not-visible` plus the selector flags. The JSON answer echoes the selector as an object.

### Examples (today, then new)

| Today | New |
|---|---|
| `'id:ProblemEditorRunLocalButton'` | `{id='ProblemEditorRunLocalButton'}` |
| `'text:Continue'` | `{text='.*Continue.*'}`: the migration keeps today's substring match by wrapping the text, with its regex characters escaped (none of the nine literal texts in `e2e/*.lua` has one); tighten to `'Continue'` afterwards |
| `'text:' .. name` (editor-typing.lua:315) | `{text='.*editor[.](focus\|blur\|send_message).*'}` |
| `'css:.cm-line:last-child'` | `{css='.cm-line:last-child'}` |
| `('xy:%d,%d'):format(x, y)` | `{point=('%d,%d'):format(x, y)}` |
| an id element whose text contains `3 / 3` (a polling loop today) | `{id='ProblemEditorRunOutcomeTotals', text='.*3 / 3.*'}` |
| `label:` (browser only today, no caller) | folded into `text` |

## Waiting

### Signature

```lua
local value = device:extended_wait_until{visible=sel}   -- or not_visible=sel, or condition=function() ... end
local value = device:extended_wait_until{visible=sel, timeout=ms, optional=bool, label='...'}
local value = device:extended_wait_until{condition=fn, timeout=ms, interval=ms, optional=bool, label='...'}
local el = device:assert_visible(sel, {timeout=ms, optional=bool, label='...'})   -- the options table is optional
device:assert_not_visible(sel, {timeout=ms, optional=bool, label='...'})          -- returns true
```

CLI, the same on `wf browser` and `wf android`:

```
extended-wait-until (--visible | --not-visible) <selector flags> | --condition '<Lua expression>'
                    [--timeout MS] [--interval MS] [--optional] [--label TEXT]
assert-visible <selector flags> [--timeout MS] [--optional] [--label TEXT]
assert-not-visible <selector flags> [--timeout MS] [--optional] [--label TEXT]
```

`extended_wait_until` takes exactly one of `visible`, `not_visible` and `condition`; anything else is a usage error (Lua raises at the line, the CLI exits 2). This is Maestro's extendedWaitUntil (visible, notVisible, timeout), with `cy.waitUntil(checkFunction, {timeout, interval})` added as `condition`, plus Maestro's shared `optional` and `label`.

`assert_visible(sel, opts)` is `extended_wait_until{visible=sel, ...opts}` and `assert_not_visible(sel, opts)` is `extended_wait_until{not_visible=sel, ...opts}`: the same code path, the same defaults, the same evidence, under their own span names. They take no `interval`, as the selector modes do not.

### Options and defaults

- `timeout` (ms): the default is `WF_WAIT_BUDGET_MS` (10000). With `optional` and no timeout, the default is `WF_OPTIONAL_LOOKUP_MS` (1000). The owner kept the repo's budget (2026-10-20); Maestro's lookupTimeoutMs 17000 and optionalLookupTimeoutMs 7000 (Orchestra.kt:142-143, 556, 1488), Cypress's 4000 and cypress-wait-until's 5000 are not used. The timeout is always honoured: today `wait(sel, secs)` drops secs, and `wait_gone` takes none. 0 means look once. A timeout above the budget is a declared slow wait: its timing row says so, so the budget report still sees it.
- `interval` (ms, condition only): the default is 200, as in cypress-wait-until, and the minimum is 50. It is the longest pause after each check returns, so an Android `get_pixel` check, which takes one screencap of about 300 ms, runs about every 500 ms; the browser checks again as soon as the app's DOM changes within it, but never within 50 ms of the last check. `interval` with `visible` or `not_visible` is a usage error rather than being ignored: selector waits are event-driven on the browser and asked again 100 ms after each answer on Android.
- `optional` (bool, default false): when the timeout passes, the wait returns nil instead of failing. There is no diagnosis look, no screenshot and no hierarchy dump. The step is logged as an optional miss and counted as a lookup, not a timeout.
- `label` (string): names the step in report.json, in timing.jsonl and in the error (Maestro's label).

### How it checks

- `visible`: the selector matches a visible element (see [Visible](#visible)).
  - Browser: one `waitObserve` subscription runs the read-only matcher and resolves on the first DOM, intersection or resize change that makes it match. It uses a MutationObserver per document, the editor WebView included, and no polling. Nothing is written to the page.
  - Android: the UiSelector lookup on the uiautomator2 server, asked again 100 ms after each answer. Today it polls every 500 ms (`android_input.rs:15`), in whole seconds, and `appears` rounds up to at least 1 s.
- `not_visible`: met at the first check where no visible element matches, so it returns at once when the element is already gone.
  - Maestro's notVisible keeps looking for another 500 ms (Orchestra.kt:1124-1130). That is not taken: every `not_visible` in the scripts follows a step that saw the element, and the window would add about 7 s to a full run.
  - Where a script means "stayed absent for a while", it writes that as `visible` with `optional` (rewrite 5).
- `condition`: a Lua function. It is called at once and again `interval` ms after each return until it returns a truthy value (anything except nil and false), like `cy.waitUntil`'s checkFunction. On the browser a DOM mutation in the app's documents within the interval brings the next check forward, no sooner than 50 ms after the last (one `waitObserve` watch per pause, the same timeout). A mutation since the check began counts, so one made while the watch was being set up is not missed; a document watched for the first time counts as changed. The observers go 30 s after the last watch.
  - It must only read. Inside it the `get_*` reads work. Any action raises `a condition must not act: <name>`, because a check that is retried must never change the screen. The actions are `tap_on`, `swipe`, `scroll_until_visible`, `input_text`, `press_key`, `back`, `open_link`, `set_microphone_input`, `draw_strokes_on`, `run_shell`, `clear_logs`, `add_network_stub`, `take_screenshot` and `assert_screenshot`. Cypress forbids commands inside `should(fn)` for the same reason.
  - An error raised inside it fails the wait at once, with that error: it is a broken check, not a slow app. (Cypress's `should(fn)` retries on a throw instead.)
  - On the CLI, `--condition` is one Lua expression run in a fresh read-only Lua state with the same rule, for example `--condition 'not device:get_device_state("keyboard").visible'`.
- The run-stop check runs on every loop turn, so a stopped run never waits out a timeout.

### What it returns

- `visible` and `assert_visible`: the element, as `get_element` returns it: `{bounds={left, top, right, bottom, cx, cy}, text}`.
- `not_visible` and `assert_not_visible`: `true`.
- `condition`: the first truthy value the function returned, as `cy.waitUntil` yields it; for example the pixel table or the buffer.
- optional and not met: nil.

The CLI result is `{met, value, ms, checks}`. An optional miss is `{met:false, ...}` with exit 0.

### What a timeout raises (optional = false)

When `timeout > 0`, the wait keeps checking for `WF_WAIT_DIAGNOSE_MS` more (8000). This is only to tell a slow app from a broken one; it fails either way. A wait with `timeout = 0` skips this look, so `extended-wait-until --condition 'device:get_device_state("keyboard").visible' --timeout 0` answers at once; today a miss of the call it replaces (`keyboard --require`) costs 8 s.

In Lua it raises at the script line, in the form `<file>:<line>: <command>{...} '<label>' <error> -- <verdict>; evidence: <json path>`, for example:

```
e2e/voice-edit.lua:234: assert_visible{id='ProblemEditorVoiceReviewSheet-Send'} 'review sheet up' timed out after 30000 ms -- slow: met 33120 ms after the call; evidence: <test output dir>/wait-timeout-1729430000000.json
```

`pcall` receives the same string. The evidence JSON is written beside the screenshot and listed in report.json.

The CLI exits 1 with:

```
{ok:false, command:'extended-wait-until', platform, device, ms, error:'timed out after 30000 ms',
 verdict:'slow'|'broken', met_after_ms:33120|null,
 evidence:{screenshot:'<test output dir>/wait-timeout-<ts>.png', hierarchy:'<test output dir>/wait-timeout-<ts>.hierarchy.json',
   element:'absent' | 'present, not visible: 12,840 360x48, on-screen part empty, checkVisibility=false, frame=editor' | 'still visible at 0,610 393x242',
   matches:<elements that match, ignoring visibility>, last_value:<the condition's last return as JSON, capped at 2 KB>, checks:<n>,
   keyboard_visible:<bool>, logs:[<the last 3 lines at warn or above>]},
 next:'<one concrete step>'}
```

The same evidence object goes into report.json for the failing step, and into the run bundle. `next` is chosen from the evidence:
- nothing matched: "the selector matched nothing: compare it with `<hierarchy>`";
- present but off screen: "`scroll_until_visible{element=...}` first";
- slow: "raise timeout to `<met_after_ms rounded up>` if this is slow by design, else find what slowed it";
- condition never met: "the last value was `<v>`: check the condition".

### Timing and logs

The span names are `extended_wait_until:visible`, `extended_wait_until:not_visible`, `extended_wait_until:condition`, `assert_visible` and `assert_not_visible`, each with `:optional` appended when it is set. Optional waits count as lookups (hit or miss); the others end as ok or timeout. `WF_WAIT_LOG` lines become `{call, sel or label, ms, ok, timeoutMs, budgetMs, platform}`. The byVerb consumers (`test_results.rs`, `test_publish.rs`) are renamed in the same commit, and the break in the trend lines is logged.

### Implicit waits

Maestro's own, not extra wait verbs. `tap_on` and `draw_strokes_on` wait for their element up to `WF_WAIT_BUDGET_MS`, or up to `WF_OPTIONAL_LOOKUP_MS` with `optional`, before acting. `scroll_until_visible` searches up to its own timeout (Maestro's 20000). Every `get_*` read answers at once.

### Today's waits rewritten

Seconds become ms, and selectors are rewritten as in [Selector](#selector). The rule: an assertion the script needs to hold is `assert_visible` or `assert_not_visible`; a probe whose answer branches the script is `extended_wait_until{visible= or not_visible=, optional=true}`; everything else is a `condition`.

1. `wait(sel)` → `device:assert_visible(sel)`. When the next line taps the same element, the wait is deleted instead: `tap_on` waits by itself.
   - tamil-writing.lua:134 `wait('id:TamilWriteCheck', 30)` → `assert_visible({id='TamilWriteCheck'}, {timeout=30000})`. It now really waits 30 s instead of 10.
2. `wait_slow(sel, n)` → `assert_visible(sel, {timeout=n*1000})`. Examples:
   - voice-edit.lua:231 → `assert_visible({id='ProblemEditorVoiceReviewSheet-TranscriptInput'}, {timeout=60000, label='transcribed'})`;
   - app-routes.lua:22 → `timeout = n == 1 and 180000 or 60000`;
   - stop-probe.lua:12 → `assert_visible({id='StopProbeNeverRendered'}, {timeout=900000})`. The test's 60 s lease renewal keeps the lease through this wait.
3. `wait_gone(sel)` → `assert_not_visible(sel)`. This covers the rails after Done (5 sites), the Reset dialog after Confirm (3), and the old screen after `open_link(base..'/settings')` (4). Each follows a step that saw the element.
4. `wait_gone_slow` inside `pcall` (voice-edit.lua:241,243) →
   ```lua
   if not device:extended_wait_until{not_visible={id='ProblemEditorVoiceReviewSheet-TranscriptInput'}, timeout=60000, optional=true} then outcome = 'timeout' end
   if device:extended_wait_until{visible={id='ProblemEditorVoiceProcessing'}, timeout=4000, optional=true}
     and not device:extended_wait_until{not_visible={id='ProblemEditorVoiceProcessing'}, timeout=120000, optional=true} then outcome = 'timeout' end
   ```
   editor-typing.lua:296 (Android, `wait_gone_slow('text:All tests passed', 20)`) is deleted, not rewritten. The toast is drawn outside the accessibility tree, so the wait passes at once (the script's own comment says so); rewrite 8c does the real work.
5. `appears(sel[, n])` → `extended_wait_until{visible=sel, timeout=(n or 2)*1000, optional=true}`:
   - The route loops (editor-typing.lua:21-24, voice-edit.lua:61-64):
     ```lua
     while os.time() - t0 < secs do
       if device:extended_wait_until{visible=sel, timeout=8000, optional=true} then return end
       if device:get_element({text='.*Continue.*'}) then device:tap_on({text='.*Continue.*'}) end
     end
     device:assert_visible(sel, {timeout=5000, label='route ready'})
     ```
   - warm-app.lua:15, and debugger-probe.lua:32-37 (which polls with `sleep(2)`):
     ```lua
     while not device:extended_wait_until{visible={id='DebugFab'}, timeout=3000, optional=true} do
       -- answer the dialogs with get_element + tap_on, or back
     end
     ```
   - tamil-writing.lua:66 → `{visible=wait_for, timeout=timeout*1000//3, optional=true}`.
   - The branch probes (editor autofocused, toolbar up, Reset dialog in the tree, Done shown, score badge, Thinking strip) keep their timeouts, with `optional=true`.
   - tamil-writing.lua:111 `if not device:appears(score, 1) then break end` means "the score stayed absent for 1 s". It keeps that meaning:
     ```lua
     if not device:extended_wait_until{visible={id='TamilPracticeStrokeScore'}, timeout=1000, optional=true} then break end
     ```
   - stubbed-run-local.lua:57-58 →
     ```lua
     device:assert_visible({text='.*Accepted.*'}, {timeout=5000, label='stubbed run shown as Accepted'})
     assert_true(not device:extended_wait_until{visible={text='.*Unknown error.*'}, timeout=1000, optional=true}, 'the app could not read the stubbed answer')
     ```
   - Waits whose result is ignored today (tamil-writing.lua:91,148) get `optional=true` with the result unused, so behaviour is unchanged.
6. `wait_keyboard(false, 5)` (editor-typing.lua:142, whose result was discarded) →
   ```lua
   device:extended_wait_until{condition=function() return not device:get_device_state('keyboard').visible end, timeout=5000, label='keyboard down after Done'}
   ```
   It now fails when the keyboard stays up. debugger-probe.lua:61 (25 x `sleep(0.2)`) becomes the same wait.
7. `wait_recording(n)` (voice-edit.lua:208,211), the callback wait that replaces audio-taken:
   ```lua
   local function mic_took() return device:get_device_state('microphone').consumed end
   if not device:extended_wait_until{condition=mic_took, timeout=4000, optional=true}
     and android and device:extended_wait_until{visible={text='WHILE USING THE APP'}, timeout=3000, optional=true} then
     device:tap_on({text='WHILE USING THE APP'})
   end
   device:extended_wait_until{condition=mic_took, timeout=10000, label='the recorder took the clip'}
   ```
   No camera feed is built (owner, 2026-10-20). Any future input state is read the same way: a field of `get_device_state`, waited on with a condition.
8. `wait_pixel(x, y, test[, n])`:
   - a. Asserted (editor-typing.lua:223, voice-edit.lua:167):
     ```lua
     local reset = ('%d,%d'):format(across(356), from_middle(578))
     device:extended_wait_until{condition=function() return blue(device:get_pixel(reset)) end, timeout=10000, label='Reset dialog drawn'}
     ```
     debugger-probe.lua:99 (20 x `sleep(0.5)`) becomes the same wait.
   - b. A probe inside the Send loop, whose result is discarded (voice-edit.lua:221) → `{condition=function() return covered(device:get_pixel(pt)) end, timeout=2000, optional=true}`. The loop re-checks the state afterwards, as it does today.
   - c. A wait whose result is discarded (editor-typing.lua:299) →
     ```lua
     device:extended_wait_until{condition=function() local c = device:get_pixel('83%,20.5%') return not (c.r > 200 and c.g > 200 and c.b > 200) end, timeout=20000, label='result toast gone'}
     ```
     It now fails when the toast stays.
9. `wait_editor_longer(before[, s])`. The scripts' `buffer()` helpers become one read:
   ```lua
   local function buffer() local e = device:get_element({id='CodeEditorInput'}) return e and e.text end
   ```
   - Browser (editor-typing.lua:126, debugger-probe.lua:87) → `device:extended_wait_until{condition=function() return #(buffer() or '') > before end, label='toolbar key landed'}`.
   - Android, `pcall(device.wait_editor_longer, device, before, 2)` (editor-typing.lua:124) → the same with `timeout=2000, optional=true`, because the accessibility read misses some keys.
   - debugger-probe.lua:85 (20 x `sleep(0.3)`) → the same with `timeout=6000`.
10. The other sleep polls:
    - stubbed-run-local.lua:44-48 (30 x read the text, `sleep(1)`) →
      ```lua
      local shown = device:extended_wait_until{visible={id='ProblemEditorRunOutcomeTotals', text='.*3 / 3.*'}, timeout=30000, optional=true}
      ```
      then today's Android diagnosis branch when it is not shown, then `assert_true(shown, ...)`.
    - debugger-probe.lua:113, `sleep(1)` before the after-reset screenshot → `{condition=function() return not (buffer() or ''):find(EDIT, 1, true) end, timeout=3000}`.
    - voice-edit.lua:212 `sleep(clip_s)` stays, as `sleep_ms(clip_s * 1000)`. The clip plays in real time, and only the browser's fake microphone could report that the clip has ended. Android's injector swaps a file and cannot, so a read would not be congruent between the platforms.
11. CLI forms in runbooks:
    - `keyboard --require` → `extended-wait-until --condition 'device:get_device_state("keyboard").visible' --timeout 0`;
    - `warnings --fail` → `extended-wait-until --condition '#device:get_logs() == 0' --timeout 0`.

## Commands

Each command lists its Maestro source, its CLI form, what it does, how each platform does it, what it replaces and who calls it today. Every action and wait also takes `optional` and `label` (see [Principles](#principles)); every CLI verb also takes `--device D`.

### `device:open_link(link [, {optional, label}])`

- **Maestro**: openLink (link). Its autoVerify and browser options are not taken: no caller.
- **CLI**: `open-link <link> [--device D] [--optional] [--label L]`
- **Does**: Opens a URL or deep link in the app. It returns once the platform has accepted the link, not once the screen has rendered, so follow it with a wait. Result `{link}`.
- **Browser**: The first call in a test opens the test's session: start-device with the test's `--screen-size` and `--frame`, recording on, and the test's pid as owner. It then arms the queued logpoints and stubs and navigates the app frame (today's navigate RPC). The console buffer starts when the session opens, so messages from before the first navigation are kept.
- **Android**: Sends a VIEW intent for the link to Expo Go with `am start` (today's `wf android open`). Lease-guarded.
- **Replaces**: Lua `device:open`; `wf android open`; `wf browser navigate`, and the session-opening half of `wf browser open`.
- **Callers today**: `device:open` 17 lines (10 in CI-run scripts, 11 files); harness.rs 13. Runbooks: `wf android open` 9, `wf browser navigate` 4, `wf browser open` 4. debug-overlay.md:107,122 and theme-switching.md:287 also become `open-link`, after a `run-shell` force-stop.

### `device:tap_on(sel [, {optional, label}])`

- **Maestro**: tapOn (the selector fields id, text, css and point; optional, label). repeat, delay, retryTapIfNoChange and waitToSettleTimeoutMs are not taken: no caller.
- **CLI**: `tap-on <text> | --id RE | --text RE | --css S | --point X,Y [--optional] [--label L]`
- **Does**: Waits until the element is visible, which is Maestro's implicit lookup: up to `WF_WAIT_BUDGET_MS`, or `WF_OPTIONAL_LOOKUP_MS` (1000) with `optional`. Then it taps inside the element's on-screen part. With `point` it taps that screen point, with no wait. It appends `{t, kind:'tap', x, y, selector}` to the input log, and on the browser it draws the tap dot. Result `{x, y, bounds}`. A miss fails with the same evidence as a timed-out wait; with `optional` it returns nil after the short lookup. Tapping a field is how the keyboard comes up.
- **Browser**: A touch at a point in the middle 60% of the on-screen part (today's tap_touch jitter), in CSS px, sent through the additive `tap {x, y}` RPC. Before that, regex and css selectors are resolved in read-only page JS: the app page first, then the editor WebView.
- **Android**: The uiautomator2 server's click on the matched node (`android_u2::click_target`). If that fails, input tap at the centre of the node's visible bounds, as today (`android_input.rs:221-237`). `point` uses the server's click(x, y), with input tap as the fallback (`android.rs:77-87`). Physical px. A css selector exits 2. Lease-guarded.
- **Replaces**: `device:tap`; `wf browser tap --ref [--outer]`; `wf android tap --id|--text|x y`. Also the "wait(X) then tap(X)" pairs. `pcall(function() device:tap(X) end)` is rewritten by what its fallback does:
  - The fallback scrolled (editor-typing.lua:118,134; stubbed-run-local.lua:31; tamil-writing.lua:145): `scroll_until_visible`, then `tap_on`.
  - A miss is expected and must cost nothing (editor-typing.lua:22; voice-edit.lua:62,227; tamil-writing.lua:109,124,160): `if device:get_element(X) then device:tap_on(X) end`.
  - X may still be arriving: `tap_on(X, {optional=true})`.
- **Callers today**: `device:tap` 44 lines (33 CI-run, 8 files); harness.rs 9. Runbooks: browser tap 5, android tap 2. `frontend/mla-tamagui/e2e` calls the tap RPC 18 times; that stays unchanged, because `ref` stays and `{x, y}` is additive.

### `device:swipe(start, end_point [, {duration=400, optional, label}])`

`start` and `end_point` are `'x,y'` or `'x%,y%'` strings. They are positional because `end` is a Lua keyword.
- **Maestro**: swipe (start, end, duration, default 400). direction, from and waitToSettleTimeoutMs are not taken as swipe options: no caller (`scroll_until_visible` uses `from`).
- **CLI**: `swipe --start X,Y --end X,Y [--duration MS]`
- **Does**: Moves one finger from start to end over `duration` ms; a short duration flings. Appends to the input log. Result `{start, end, duration}`.
- **Browser**: A touch-move sequence in the device frame that now honours `duration`, through the additive `durationMs` RPC argument; today the driver drops ms (`device/browser.rs:291`). Desktop presets drag with the mouse.
- **Android**: The uiautomator2 server's swipe RPC (`android_u2::swipe`), falling back to `input swipe x1 y1 x2 y2 duration`, as today (`android.rs:77-87`). Lease-guarded.
- **Replaces**: `device:swipe(x1, y1, x2, y2[, ms])`; `wf browser swipe --from --dx --dy`, drag and scroll; `wf android swipe x1 y1 x2 y2 --duration`.
- **Callers today**: `device:swipe` 12 lines (9 CI-run, 6 files); about half sit in the hand-written loops that `scroll_until_visible` replaces. Runbooks: browser swipe 4. There is no flow test for it today; one is added.

### `device:scroll_until_visible{element=sel, direction='DOWN', from=sel, timeout=20000, visibility_percentage=100, optional, label}`

- **Maestro**: scrollUntilVisible (element; direction DOWN, UP, LEFT or RIGHT, default DOWN; timeout, default 20000; visibilityPercentage, default 100), plus `from`. Maestro always swipes from the screen centre (Maestro.kt:203-211); we need `from` because the editor toolbar's two rows scroll on their own. speed and centerElement are not taken.
- **CLI**: `scroll-until-visible --element-id RE | --element-text RE | --element-css S [--direction D] [--from-id RE | --from-text RE | --from-css S] [--timeout MS] [--visibility-percentage N]`
- **Does**: `direction` says where the wanted content lies, as in Maestro: DOWN brings content from below into view, so the finger moves up. While less than `visibility_percentage` of the element is on screen, it swipes once and checks again. Each swipe is a 600 ms drag, slow enough to move a row by its own length without a fling. It starts at the centre of `from` (default: the screen) and moves toward the edge of `from` opposite `direction`, stopping 10% inside that edge. The two toolbar rows are named separately: `from={id='EditorActionToolbar'}` for Done, `from={id='EditorSyntaxToolbar'}` for the syntax keys; the rail's centre is the border between the rows, so a swipe started there would move neither. Result: the element as `get_element` returns it, plus the number of swipes, `{bounds, text, swipes}`. On timeout it fails with the wait's evidence plus the swipes made.
- **Browser**: The same loop over swipe and the element read, in CSS px. The visible percentage is the clipped area over the full `getBoundingClientRect` area. Each check reads the element and, while it is not shown enough, `from` in one page evaluation.
- **Android**: The same loop, in physical px. The visible percentage is the node's visible bounds over its bounds. A key scrolled off screen is absent from the accessibility tree, so the direction comes from the caller (editor-typing's ROW1 order), never from the key's last position. Lease-guarded.
- **Replaces**: Nine hand-written loops:
  - `bring_into_view`: voice-edit.lua:136-144 and editor-typing.lua:97-114;
  - bringing Done into view: voice-edit.lua:149-155, editor-typing.lua:134-140, debugger-probe.lua:52-58, agent-proof.lua:11-14 and stubbed-run-local.lua:31-37;
  - the Two Sum list: debugger-probe.lua:42-46;
  - the debug log sheet: editor-typing.lua:319-323, where `element` becomes `{text='.*editor[.](focus|blur|send_message).*'}` with `from={id='DebugLogSheetList'}`.
  Also tamil-writing.lua:145's scroll-down-then-tap fallback.
- **Callers today**: none directly. It replaces the loops above in 7 scripts, including the G1 voice-edit and G3 editor-typing gates.

### `device:input_text(text [, {optional, label}])`

- **Maestro**: inputText (text, label). The inputRandom family is not taken.
- **CLI**: `input-text <text>`
- **Does**: Types the text at the caret of the focused field, one key at a time with human pacing, as a thumb on the soft keyboard would. It refuses while the keyboard is down, with evidence and the next step "tap the field first". It never types without focus, as Maestro's inputText can. It appends one input-log line per run of keys. Result `{typed, ms}`.
- **Browser**: Today's `type --ref :focus --at-caret --human`, which keeps G3's human typing. Fill mode, `--submit` and `--delay` are dropped.
- **Android**: `input text`, in runs of printable characters. As today, it refuses `%`, backslash and non-ASCII; Maestro's inputText has no Unicode on Android either. Lease-guarded.
- **Replaces**: `device:type_text`; `wf browser type`; `wf android type` and its duplicate, `wf android text`.
- **Callers today**: `device:type_text` 2 lines (1 CI-run, 2 files): editor-typing.lua's `typ()` helper, called for every chunk of typed text, and debugger-probe.lua:83. harness.rs 3. Runbooks: browser type 3, android type 1.

### `device:press_key(key [, {optional, label}])`

- **Maestro**: pressKey (one key, by Maestro's KeyCode names: Enter, Backspace, Tab, and Remote Dpad Left, Right, Up and Down for the arrow keys). Back is its own command, `back`. Ctrl+End is ours.
- **CLI**: `press-key <key>`
- **Does**: Presses one named key, case-insensitive: Enter, Backspace and Tab; the arrows, under Maestro's names Remote Dpad Left, Right, Up and Down, which move the caret in a text field; and Ctrl+End, which moves the caret to the end of the buffer. Any other name exits 2 and lists these; letters go through `input_text`. The keys need the keyboard up, as a user's do. Result `{key}`.
- **Browser**: DOM key events at the focused element: ArrowLeft and the other arrows, and Control+End.
- **Android**: `input keyevent` 66, 67 or 61; the arrows are keyevents 21, 22, 19 and 20 (DPAD). Ctrl+End is `input keycombination 113 123`, moved here out of the scripts. Lease-guarded.
- **Replaces**: `device:press(key, ...)` varargs; `wf browser press <DOM key> [--repeat]`; `wf android press <keys...>`; `device:shell('input keycombination 113 123')`.
- **Callers today**: `device:press` 10 lines (7 CI-run, 5 files); six press Enter, Backspace or Ctrl+End, and the four Back presses move to `back`. shell keycombination 2 (editor-typing.lua:241, debugger-probe.lua:74). Runbooks: press 3, and device-lab-codemirror-typing.md:24 names the arrows.

### `device:back([{optional, label}])`

- **Maestro**: back.
- **CLI**: `back` (`wf android`; `wf browser` exits 2)
- **Does**: Presses the system Back key; scripts use it to close Expo Go's developer menu. It refuses while the keyboard is up, with evidence and the next step "tap Done", so Back can never lower the keyboard. That is also why hideKeyboard is not offered: on Android, Maestro's hideKeyboard is this same key. Result `{}`.
- **Browser**: Exits 2: a page has no system Back key. Maestro's web back is history back, but every back call in the scripts sits behind the Expo Go developer menu (text Reload or Toggle performance monitor), which never appears on the web.
- **Android**: `input keyevent 4`. Lease-guarded.
- **Replaces**: `device:press('back')`; `wf android press back`.
- **Callers today**: 4 lines: metro-smoke.lua:18, warm-app.lua:17 (the CI warm-up), tamil-writing.lua:40 (G2), debugger-probe.lua:34.

### `device:extended_wait_until{visible=sel | not_visible=sel | condition=fn, timeout, interval, optional, label}`

- **Maestro**: extendedWaitUntil (visible, notVisible, timeout), extended with cy.waitUntil's checkFunction and interval, and with Maestro's optional and label.
- **CLI**: `extended-wait-until (--visible | --not-visible) <selector flags> | --condition '<Lua expression over device reads>' [--timeout MS] [--interval MS] [--optional] [--label L]`
- **Does**: Waits until the element is visible, until no visible element matches, or until the condition returns a truthy value. Returns that value, or nil for an optional miss; otherwise it raises with evidence. The full semantics are in [Waiting](#waiting).
- **Browser**: Selector waits: one `waitObserve` subscription running the read-only matcher, with no polling. Conditions: the callback runs again on the app's next DOM mutation, at least 50 ms and at most `interval` after it returned.
- **Android**: Selector waits: the UiSelector lookup on the uiautomator2 server, asked again 100 ms after each answer; today it polls every 500 ms (`android_input.rs:15`), in whole seconds. Conditions: the callback runs every `interval`.
- **Replaces**: `wait_gone_slow`, `appears`, `wait_keyboard`, `wait_recording`, `wait_pixel` and `wait_editor_longer` (with `assert_visible` and `assert_not_visible`, which take `wait`, `wait_slow` and `wait_gone`); the sleep poll loops; `wf browser wait-observe`; `wf android wait`; `wf android keyboard --require`; `wf android warnings --fail`.
- **Callers today**: 81 wait call lines, in every CI-run script: wait 19, wait_slow 12, wait_gone 12, wait_gone_slow 3, appears 25, wait_keyboard 1, wait_recording 2, wait_pixel 4, wait_editor_longer 3. Also 6 of the 7 sleep calls (debugger-probe.lua:36,61,85,99,113 and stubbed-run-local.lua:47). harness.rs about 15. agents.rs:242 checks that the agent prompt names the wait. Runbooks: `keyboard --require` 2 (device-lab-codemirror-typing-android.md:17,29) and `warnings --fail` 1 (device-lab-tamil-writing-android.md:54).

### `device:assert_visible(sel [, {timeout, optional, label}])` → the element, or nil

- **Maestro**: assertVisible, its most-used wait (owner, 2026-10-20: both names exist).
- **CLI**: `assert-visible <text> | --id RE | --text RE | --css S [--timeout MS] [--optional] [--label L]`
- **Does**: `extended_wait_until{visible=sel, ...}`: the same code path, defaults (`WF_WAIT_BUDGET_MS`, or `WF_OPTIONAL_LOOKUP_MS` with `optional`), return value and evidence. Its span is `assert_visible` (`assert_visible:optional` with `optional`). It takes no `interval`.
- **Browser** and **Android**: as `extended_wait_until`'s `visible`.
- **Replaces**: `device:wait(sel[, secs])` and `device:wait_slow(sel, n)` (wait rewrites 1 and 2).
- **Callers today**: wait 19 and wait_slow 12 lines, less the waits deleted because the next line taps the same element.

### `device:assert_not_visible(sel [, {timeout, optional, label}])` → true, or nil

- **Maestro**: assertNotVisible (owner, 2026-10-20: both names exist). Maestro's 500 ms "stays gone" window is not taken (see [Waiting](#how-it-checks)).
- **CLI**: `assert-not-visible <text> | --id RE | --text RE | --css S [--timeout MS] [--optional] [--label L]`
- **Does**: `extended_wait_until{not_visible=sel, ...}`: met at the first check where no visible element matches. The same code path, defaults and evidence. Its span is `assert_not_visible` (with `:optional` when set). It takes no `interval`.
- **Browser** and **Android**: as `extended_wait_until`'s `not_visible`.
- **Replaces**: `device:wait_gone(sel)` (wait rewrite 3).
- **Callers today**: wait_gone 12 lines (9 CI-run, 6 files).

### `device:take_screenshot(path [, {label}])`

- **Maestro**: takeScreenshot (path, label). cropOn is not taken.
- **CLI**: `take-screenshot <path>`
- **Does**: Saves the screen as a PNG at `path`, adding `.png` when it is missing, as Maestro does, and lists it in the report's screenshots. Result `{path, width, height}`.
- **Browser**: The device frame's screen in device px. The daemon's screenshot RPC is unchanged; the driver copies its file to `path`, as today.
- **Android**: `adb screencap` to the path.
- **Replaces**: the path form of `device:screenshot(path)`; `wf browser screenshot`; `wf android screenshot --out`.
- **Callers today**: 22 of the 27 `device:screenshot` lines use the path form (9 files). Runbooks: browser screenshot 6, android screenshot 9. The frontend e2e scripts call the screenshot RPC, which is unchanged.

### `device:assert_screenshot(path [, {threshold_percentage=95, ignore={{left, top, right, bottom}, ...}, optional, label}])` → `{path, verdict, match_percentage}`

- **Maestro**: assertScreenshot (path; thresholdPercentage, default 95). It compares on every run and fails when the reference is missing, as Maestro's does. `ignore` is ours; Maestro has only cropOn, which is not taken.
- **CLI**: `assert-screenshot <path> [--threshold-percentage N] [--ignore L,T,R,B]...` (`test --update-goldens` writes instead of comparing)
- **Does**: `path` is the golden's path relative to the platform's goldens dir (`docs/runbooks/goldens/<platform>/`), with `.png` added when it is missing. The command scales the screen to a 360 px PNG and compares it with the golden. It fails below `threshold_percentage`, or when no golden exists, with the diff image and `match_percentage` as evidence. Under `test --update-goldens` it writes the golden and its sidecar instead, with verdict `updated`; otherwise the verdict is `match`. Ignore rects are in tap units and are scaled by dpr on the browser. This changes CI: today the comparison happens only under `--goldens`, which CI never passes (deploy.yml gives e2e-tests.yml no flags), so no golden has ever been asserted in CI.
- **Browser**: The same golden mechanics over the browser screenshot.
- **Android**: The same golden mechanics over `adb screencap`.
- **Replaces**: `device:screenshot(name, opts)` and `device:golden`; `test --goldens` (comparing is now the default). `threshold_percentage` is 100 x (1 - today's max_ratio). The result's ratio becomes `match_percentage` (web-goldens.lua:11 logs it). tolerance and max_width are not taken: no caller.
- **Callers today**: 5 lines: editor-typing.lua:300, tamil-writing.lua:80, stubbed-run-local.lua:27 and :59, web-goldens.lua:10. golden: harness.rs only. Committed goldens: android editor-two-sum-result, editor-valid-palindrome-result, tamil-uyir-1-open, tamil-uyirmei-k-open; browser web-home. two-sum-before-run and two-sum-stubbed-result have none, so step 7 creates them.

### `start_recording`: `start-recording <path>` (CLI only)

Not bound in Lua: a test records itself from its first `open_link` to its end (`--no-record` turns that off), and no script calls record_start.
- **Maestro**: startRecording (path, required; `.mp4` is added).
- **Does**: Starts a screen recording at `path` (adding `.mp4` when it is missing, as Maestro does), with the input log beside it. Result `{path}`.
- **Browser**: A new additive RPC, `recordStart`, over `cdp::record`, so a recording can start in the middle of a session. Today the only way to record is `open --video`, which records from the moment the session opens.
- **Android**: Switches adbd to root first, so that a later `set_microphone_input` cannot kill screenrecord, then records in 170 s segments. Lease-guarded.
- **Replaces**: `wf android record start`; `wf browser open --video/--video-path`; Lua record_start (no caller).
- **Callers today**: Android runbooks: record start and stop at device-lab-tamil-writing-android.md:37,55, plus 3 prose mentions. Browser runbooks open sessions with `--video` (3), which becomes `start-device`, then `start-recording`.

### `stop_recording`: `stop-recording` (CLI only)

- **Maestro**: stopRecording.
- **Does**: Ends the recording and returns its files. The device stays usable; today the browser's close ends the whole session. Like Maestro's, it does not fail when nothing is recording. Result `{files}`.
- **Browser**: A new additive RPC, `recordStop`, which finishes the video without closing the session.
- **Android**: Stops screenrecord and pulls the segments. Lease-guarded.
- **Replaces**: `wf android record stop --out`; `wf browser close` as a way to end the video; Lua record_stop (no caller).
- **Callers today**: the same runbooks as `start-recording`.

### `run_flow(label, function() ... end)` → the function's results (a global)

- **Maestro**: runFlow (label, commands). `when` is Lua's `if`.
- **CLI**: none: the flow file itself is `test <script>`.
- **Does**: Runs a named part of the script as its own row in the report's timing (span kind step), so a backend's time shows apart from the taps around it. It is a global because it is not a device action. The same on both platforms.
- **Replaces**: `device:step(name, fn)`.
- **Callers today**: `device:step` 12 lines (12 CI-run, 6 files).

### `assert_true(value [, label])` (a global)

- **Maestro**: assertTrue (condition, label).
- **CLI**: none.
- **Does**: Fails the test at this line, with `label` as the message, when `value` is nil or false. The same on both platforms.
- **Replaces**: `assert_that`.
- **Callers today**: `assert_that` 43 lines.

### `device:get_element(sel)` → `{bounds, text}` or nil

- **Maestro**: none; the nearest is Cypress's `cy.get`. Maestro reads an element only through copyTextFrom and selectors.
- **CLI**: `get-element <text> | --id RE | --text RE | --css S`. The result is the element, or null with exit 0.
- **Does**: One look, no wait. When the element is visible it returns `{bounds={left, top, right, bottom, cx, cy}, text}`; nil when it is not. `bounds` is the on-screen part in tap units. `text` is:
  - for an input, a textarea, an Android EditText, a contenteditable, or a web view hosting one: the field's value, with the code editor's buffer lines joined by `\n`;
  - otherwise the visible text;
  - otherwise the accessibility label.

  `if device:get_element(X) then` is the cheap look that `appears(sel, 1)` pretended to be; appears cost a full second on every miss.
- **The editor buffer**: `get_element({id='CodeEditorInput'}).text` is the buffer on both platforms: on the browser, the `.cm-line` elements in the WebView's iframe; on Android, the EditText inside the WebView node. A flow test pins this. If either platform cannot satisfy it in step 3, `get_editor_text()` stays as the one documented exception, and this spec records why.
- **Browser**: Read-only page JS with the selector visibility rule, in the app page and then in the same-origin iframes; bounds are `getBoundingClientRect` clipped to the screen and to clipping ancestors, offset to the screen. Text: `value` for input and textarea, the joined `.cm-line`s for the editor, otherwise `innerText` trimmed, else `aria-label`. Today `device:text` returns innerText, which is empty for an input, so voice-edit.lua:232's read of the transcript input (`ProblemEditorVoiceReviewSheet-TranscriptInput`) records an empty `heard` on the browser.
- **Android**: The uiautomator2 lookup on the accessibility tree; bounds are the node's visible bounds; text is the node's text (an EditText's text is its value), else its content-desc. Today an empty text returns nil. The accessibility read of the editor's EditText misses some toolbar keys (a known gap), so Android conditions over the buffer are optional.
- **Replaces**: `device:find`, as a boolean and for coordinates; `device:text`; `device:editor_text`; tamil-writing's `present()`; `wf android find`, which exited 1 when the element was absent and which the driver no longer used; the hidden `wf android edit-text`, missing from help; the driver's outer-eval querySelector and innerText reads.
- **Callers today**:
  - `device:find` 30 lines (23 CI-run, 7 files). About 18 use the result as a boolean: voice-edit.lua:120,204,223,227,245,261; editor-typing.lua:198,315; tamil-writing.lua:40,42,59; stubbed-run-local.lua:17; warm-app.lua:16,17; debugger-probe.lua:32,33,34. About 12 read coordinates (`r.left`, `r.right`, `r.cx`, `r.cy`, `rail.top`, `rail.bottom`): voice-edit.lua:80,139,150; editor-typing.lua:40,101,135,254; debugger-probe.lua:43,53,55; agent-proof.lua:9; stubbed-run-local.lua:32; most sit in the loops `scroll_until_visible` replaces.
  - `device:text` 5 lines (4 CI-run, 5 files): agent-proof.lua:21, stubbed-run-local.lua:45, and the `text_of()` helpers in voice-edit, editor-typing and tamil-writing, which call it 7 more times.
  - `device:editor_text` 3 lines (2 CI-run, 3 files), in the `buffer()` helpers of voice-edit, editor-typing and debugger-probe, which call it about 15 times.
  - The `get_element`-then-`tap_on` rewrites of 7 `pcall(tap)` lines.
  - harness.rs: find 6, text 2, editor_text 1.

### `device:get_device_state([part])` → `{screen, keyboard, microphone}`, or the one part asked for

- **Maestro**: none (ours). Maestro can only hide the keyboard, and offers percentages instead of a screen size (point, start and end accept those here too).
- **CLI**: `get-device-state [screen|keyboard|microphone]`
- **Does**: What the device holds right now. With no argument it returns all three parts; with a part name it returns that part's table itself (`get_device_state('keyboard').visible`), and the driver reads only that part. It is read-only: it never moves the keyboard or touches the clip. A new kind of device state becomes a field here, never a new verb.
  - `screen`: `{width, height, dpr}`: the screen in tap units, and dpr, the screenshot pixels per tap unit.
  - `keyboard`: `{visible, bounds}`: whether the soft keyboard is drawn, and where, as `{left, top, right, bottom}` in tap units; `bounds` is null when the keyboard is down.
  - `microphone`: `{input, consumed}`: the clip set last by `set_microphone_input` (null when none was set), and whether a recorder has started on it (false when no clip was set). The read is stateless, so the CLI gives the same answer as the test.
- **Browser**:
  - screen: the preset's CSS viewport and devicePixelRatio, for example 393x852 at 3 on iphone-15-pro.
  - keyboard: the device frame's keyboard model, `__shell.getStatus().keyboardVisible` in the outer page. That is what the frame draws after its blur debounce (`browser_launch.rs:368-378`), just as Android's mInputShown reports what is drawn. Today the read uses focus (`device/browser.rs:412-415`), which disagrees with the drawn keyboard during the debounce. `bounds` is the rectangle the frame draws its keyboard model in (270 px tall).
  - microphone: the fake microphone's state in the app frame (`__mcpFakeMicState()`); `consumed` is whether its consumed list is non-empty, which `set_microphone_input` empties. Today the baseline is kept in the driver process (`clips_taken`, `device/browser.rs:20,406`), where a CLI call cannot see it.
- **Android**:
  - screen: `wm size`, with dpr 1, because input and screencap use the same pixels.
  - keyboard: `dumpsys input_method` (mInputShown) for `visible`; the IME window's frame from `dumpsys window` (`type=ime frame=[l,t][r,b]`) for `bounds`, which editor-typing.lua:258 parses by hand today.
  - microphone: `input` is the pushed clip while `inj.marker` exists; `consumed` is whether a `*.swapped` file newer than `inj.marker` exists in Expo Go's or the app's cache: one `adb shell find`.
- **Replaces**: `device:keyboard_shown`; `wf android keyboard [--require]`; `device:shell('wm size')` and the literal 393 widths; `device:shell('dumpsys window')`; the browser branch's `toolbar_above_keyboard = rail ~= nil` (editor-typing.lua:263), which is true whenever the rail is in the DOM, so the G3 gate checks nothing on the browser today; `device:wait_recording` and the trait's `recording_started`; the earlier proposal's audio-taken verb and `get_media_state`.
- **Callers today**:
  - `device:keyboard_shown` 5 lines (4 CI-run, 3 files); harness.rs 2; the runbook's `keyboard --require` 2 (device-lab-codemirror-typing-android.md:17,29). The keyboard waits are in [Waiting](#todays-waits-rewritten).
  - The `wm size` reads at editor-typing.lua:31, voice-edit.lua:71, tamil-writing.lua:22 and debugger-probe.lua:21, and the literal 393 at editor-typing.lua:137, voice-edit.lua:151-153, debugger-probe.lua:54-57 and stubbed-run-local.lua:34.
  - The G3 gate's toolbar-above-keyboard check, now one line on both platforms:
    ```lua
    local kb = device:get_device_state('keyboard').bounds
    r.toolbar_above_keyboard = rail ~= nil and kb ~= nil and rail.bottom <= kb.top + 2
    ```
  - `wait_recording` 2 lines (voice-edit.lua:208,211, CI-run, G1), which become conditions over the microphone part (wait rewrite 7).

### `device:get_pixel(point)` → `{r, g, b}`

`point` is `'x,y'` in tap units or `'x%,y%'`.
- **Maestro**: none (ours). Maestro's nearest command is assertScreenshot.
- **CLI**: `get-pixel <X,Y>`
- **Does**: The colour of one screen point, read from a fresh screenshot. It sees what the accessibility tree cannot: Android's Reset dialog and its result toast.
- **Browser**: The screenshot is in device px, so the point is multiplied by dpr. Today it is not scaled, but every current call is in an Android branch.
- **Android**: One `adb screencap` per call.
- **Replaces**: `device:pixel(x, y)`; the screenshot taken on every poll inside `wait_pixel`.
- **Callers today**: `device:pixel` 4 lines (3 CI-run, 3 files), plus the 4 `wait_pixel` sites, which become conditions over it. harness.rs 3.

### `device:set_microphone_input(path [, {optional, label}])`

- **Maestro**: none (ours). Named like setLocation, which feeds a fake sensor. Maestro's addMedia puts a list of files in the gallery, which is a different action.
- **CLI**: `set-microphone-input <path>`
- **Does**: Sets the clip the app's microphone hears at its next recording. There is one pending clip on both platforms, and a new call replaces it. Today the browser queues clips (`fake_mic.js` is a FIFO) while Android overwrites one file, so two calls behave differently. Result `{path, bytes}`. Whether a recorder took it is `get_device_state('microphone').consumed`.
- **Browser**: The daemon's `audioInject` with the additive `replace:true`, which empties the fake microphone's queue and its consumed list before pushing the clip. Formats: wav, mp3, m4a, mp4, aac, ogg, opus, webm, flac.
- **Android**: Switches adbd to root, pushes the clip over `/data/local/tmp/clip.wav`, touches `/data/local/tmp/inj.marker` and starts the injector loop that swaps the clip into the recorder's file. It refuses while a non-root recording runs, because the root switch would kill the recorder. Lease-guarded.
- **Replaces**: `device:audio_inject`; `wf browser audio-inject`; the driver's adb push and injector sequence.
- **Callers today**: `audio_inject` 1 line (voice-edit.lua:203, CI-run, G1). Runbook audio-inject 2 (device-lab-voice-edit.md:21,67).

### `device:draw_strokes_on(sel, {source='guide'|'template', human=true, optional, label})`

- **Maestro**: none (ours). Named like tapOn and longPressOn: a verb, then On, then a selector.
- **CLI**: `draw-strokes-on <selector flags> --source guide|template [--human=false]` (`human` defaults to true, as every caller passes it)
- **Does**: Waits for the drawing pad like `tap_on`, reads the pad's own guide or template strokes, and draws each one by touch: finger down, moves, finger up. `human` adds the shared timing and path jitter. Result `{strokes, points}`.
- **Browser**: Today's trace RPC in the device frame. A `human` stroke keeps its planned pace on a page slower than a frame per move: the moves already due go as the latest one, as a touchscreen coalesces them, with the path's first and last points and every finger down and up. The trace result and the input log carry the moves sent and coalesced and the longest one move's dispatch blocked (`maxMoveMs`), the pad's per-move cost.
- **Android**: Today's `wf android trace` (`input motionevent`). Lease-guarded.
- **Replaces**: `device:trace(sel, source[, human])`; `wf browser trace`; `wf android trace` ("trace" also read like a debugger trace). `pcall(device:trace(...))` at tamil-writing.lua:90,141 becomes `optional=true`.
- **Callers today**: `device:trace` 3 lines (tamil-writing.lua:90,141,143, CI-run, G2). Runbooks: browser trace 6, android trace 2.

### `device:run_shell(command)` → `stdout, exit_code`

- **Maestro**: none (ours). Maestro's runScript runs JavaScript, not a device shell.
- **CLI**: `run-shell <command>` (`wf android`; `wf browser` exits 2)
- **Does**: Runs a shell command on the device. Result `{stdout, exit_code}`; a non-zero exit is an answer, not a failure.
- **Browser**: Exits 2: a page has no OS shell.
- **Android**: `adb shell`, as the current adbd user. It is now lease-guarded; today it is not.
- **Replaces**: `device:shell`; `wf android adb shell` as scripts use it (`adb` itself stays as plumbing); `wf android status <bundleId>` and `status --url` (cold restart, cold deep link), which become `run-shell 'am force-stop host.exp.exponent'` followed by `open-link`.
- **Callers today**: `device:shell` 14 lines (12 CI-run, 6 files). 11 move to typed commands: `wm size` x4 to `get_device_state('screen')`, keycombination x2 to `press_key('Ctrl+End')`, `dumpsys window` to `get_device_state('keyboard').bounds`, logcat x4 to `get_logs` and `clear_logs`. 3 Android-only diagnostics remain: warm-app.lua:25 (`logcat -v epoch` over the main, system and events buffers for the start-up timing marks), tamil-writing.lua:68 (`dumpsys activity`) and stubbed-run-local.lua:54 (`/proc/net/tcp`). Runbooks: debug-overlay.md:107,122 and theme-switching.md:287.

### `device:get_logs([{level='warn'}])` → a list of strings, each `'<level>: <message> (at <frames>)'`

- **Maestro**: none (ours).
- **CLI**: `get-logs [--level debug|info|warn|error]`
- **Does**: The app's JS console lines and uncaught errors since the session opened or since the last `clear_logs`, at the given level or above. `Logger.*` echoes to the console (`lib/logger.ts` ConsoleEchoTransport), so the app's own log lines are included. The default level `warn` gives today's warnings. Lines at warn and above also go to report.json `warnings[]` and to the test_runs metric.
- **Browser**: The daemon's additive `consoleLogs` RPC: a buffer per session, filled in every frame from session open by CDP `Runtime.consoleAPICalled` and `Runtime.exceptionThrown`. No console wrapper is injected into the page, so `open` does not change for other clients. Messages logged before the first navigation are no longer missed; today the hook is installed after each navigation.
- **Android**: `logcat -d ReactNativeJS` at the matching priority (D, I, W or E).
- **Replaces**: `device:warnings`; `wf android warnings`. Also `device:shell('logcat -d -s ReactNativeJS | tail -40')` at editor-typing.lua:179 and tamil-writing.lua:154, and `logcat ... ReactNativeJS:V` at stubbed-run-local.lua:53, which now work on the browser too. Also the `__liveTail` read through `wf browser outer-eval` that `lib/logger.ts:95` documents.
- **Callers today**: `device:warnings` 10 lines (7 CI-run, 8 files); the 3 logcat dumps above; runbook device-lab-tamil-writing-android.md:54, where `warnings --fail` becomes `extended-wait-until --condition '#device:get_logs() == 0' --timeout 0`; harness.rs 3; the diagnosis step that `lib/logger.ts:95` describes.

### `device:clear_logs()`

- **Maestro**: none (ours). Named like clearState and clearKeychain.
- **CLI**: `clear-logs`
- **Does**: Discards the buffered log lines without reporting them. Scripts use it to drop known noise, such as React #419 after a cold web deep link (stubbed-run-local.lua:25-26), and to start fresh timing marks. Result `{cleared}`.
- **Browser**: `consoleLogs` with `clear:true`.
- **Android**: `logcat -c`, which clears every buffer. Lease-guarded.
- **Replaces**: `device:clear_warnings`; `wf android warnings --clear`; `device:shell('logcat -c')` at warm-app.lua:9.
- **Callers today**: `clear_warnings` 11 lines (8 CI-run, 8 files), plus warm-app.lua:9.

### `device:add_network_stub(method, url_pattern [, {status=200, body= | json_body= | body_file=, headers={K=V}, delay=ms}])`

- **Maestro**: none (ours). Named like addMedia: stubs accumulate for the run.
- **CLI**: none on either CLI: stubs live only for the run (see [Principles](#principles)).
- **Does**: Answers requests whose URL matches the glob, and whose method matches (`'*'` is any), with the canned response for the rest of the run. Unmatched requests still reach the network. An unknown option fails. Result `{pattern, method, status, bytes}`.
- **Browser**: CDP request interception through the daemon's `networkStub` RPC, queued until the session opens.
- **Android**: The in-run stub server plus iptables DNAT for `WF_ANDROID_STUB_PORTS` (8787 and 8789), as root. The rules are removed at shutdown.
- **Replaces**: `device:stub` (`file` becomes `body_file`, `json` becomes `json_body`, `delay_ms` becomes `delay`); `wf browser network-stub`.
- **Callers today**: `device:stub` 1 line (stubbed-run-local.lua:14, CI-run); harness.rs 3.

### `device:get_network_stub_hits()` → a list of `{method, url, status, pattern, at}`

- **Maestro**: none (ours).
- **CLI**: none on either CLI: stubs live only for the run.
- **Does**: The stubbed requests answered so far.
- **Browser**: The daemon's `networkStubHits` RPC.
- **Android**: The in-run stub server's log.
- **Replaces**: `device:stub_hits`; `wf browser network-stub-hits`.
- **Callers today**: 1 line (stubbed-run-local.lua:60); harness.rs 3.

### `device:get_debug_hits()` → a list of `{file, line, function, frames, locals}`

- **Maestro**: none (ours).
- **CLI**: none on either CLI: hits live only for the run. Points are armed with `test --logpoint file:line[:cond]`.
- **Does**: The logpoint hits so far, each with frames at original source lines and the locals at the stop. It never clears the list: a read must not change state, and no caller passes clear. Captures are not taken: only harness.rs sets them, through Lua `debug{capture=}` (harness.rs:486), which is dropped, and the locals carry the values.
- **Browser**: The CDP debugger with source maps, through the daemon.
- **Android**: The Hermes inspector through the dev Metro, attached when the test has `--logpoint`.
- **Replaces**: `device:debug_hits`; `wf browser debug-hits`.
- **Callers today**: 1 line (debugger-probe.lua:119, manual); harness.rs 1. Callers of today's `run --debug` flag, which becomes `--logpoint`: debugger-probe.lua:10-11, device-lab-debugger-probe-android.md, wf__test.txt:38 and the e2e-tests.yml dispatch flags.

## Dropped

| Dropped | Why |
|---|---|
| Lua `wait`, `wait_slow`, `wait_gone`, `wait_gone_slow`, `appears`, `wait_keyboard`, `wait_recording`, `wait_pixel`, `wait_editor_longer` | Nine names for one action, with four timeout conventions and two failure styles: `wait` drops its secs, `wait_gone` takes no timeout, the `_slow` pair is uncapped, and the probes default to 2 s and never fail. Some names mislead: `wait_editor_longer` reads as a longer wait, and `wait_recording` reads as the screen recording. `extended_wait_until`, `assert_visible` and `assert_not_visible` replace all of them ([Waiting](#todays-waits-rewritten)). Each old name works as a thin wrapper over the new wait until step 7 rewrites the scripts, then raises `renamed: use device:<new>` until step 8 removes it. |
| The draft's single-purpose reads: `get_bounds`, `is_visible`, `get_text`, `get_editor_text`, `is_keyboard_visible`, `get_keyboard_bounds`, `get_screen_size`, `is_microphone_input_consumed` | Owner, 2026-10-20: no single-purpose verbs. `get_element` covers whatever is on screen (bounds, text, an input's value, the editor buffer) and `get_device_state` whatever the device holds (screen, keyboard, microphone). `get_editor_text()` comes back only if a platform cannot give the buffer through `get_element` in step 3. |
| audio-taken and `get_media_state` (the earlier proposal) | The owner asked for a callback wait instead of a special verb. `extended_wait_until{condition=...}` over `get_device_state('microphone').consumed` does that job. |
| `wf browser camera-inject`; a camera target for media input | Owner, 2026-10-20: no camera feed. No app screen uses a camera (no expo-camera, ImagePicker or video getUserMedia in `frontend/mla-tamagui`), the verb has no caller beyond the browser-cli skill's verb table, and redroid has no camera HAL. The `cameraInject` RPC stays under the additive-only rule. |
| Lua `golden` | The old name of `screenshot(name)`, called only from harness.rs. `assert_screenshot` replaces both. |
| `test --goldens` (was `run --goldens`) | `assert_screenshot` compares on every run, as Maestro's assertScreenshot does. Only `--update-goldens` remains. |
| `stroke` (Lua, and the CLI verb on both platforms) | No script calls it, and the browser does not even bind it. `draw_strokes_on` carries the Tamil gate. The browser runbook line (device-lab-tamil-writing.md:20) moves to `draw-strokes-on`. The stroke RPC stays for `verify-tamil.mjs`, which calls it through `@lib/browser-client`. |
| Lua `unstub`; `network-stub --clear` | No caller. Stubs end with the run, and Android's shutdown already removes its DNAT rules. |
| Lua `record_start`, `record_stop` | No caller: every test records itself. The CLI keeps `start-recording` and `stop-recording` for the runbooks. |
| Lua `debug{file, line, condition, capture}`; captures in debug hits; `get_debug_hits{clear}` | Only harness.rs arms points from inside a script (harness.rs:486). `test --logpoint` arms them before the first page load, which is what debugger-probe needs. The hits' locals carry the values captures computed. No caller passes clear, and a read that clears would empty the list on every retry of a condition. |
| `json.decode` | No caller. |
| `sleep(seconds)` | Maestro has no sleep and calls fixed waits an anti-pattern. Six of the seven calls are poll loops that become conditions. The one real-time need, voice-edit's clip playback, uses `sleep_ms(ms)`. The old name raises, so no call changes unit silently. |
| hideKeyboard (Maestro) | It breaks the keyboard rule: on Android it presses Back, and on iOS it swipes. A user taps Done: `tap_on({id='EditorAction-Done'})`. |
| addMedia, copyTextFrom (Maestro, as names) | Not borrowed, because ours do something else. addMedia puts a list of files in the device gallery; `set_microphone_input` feeds one clip to the microphone. copyTextFrom stores text in `maestro.copiedText`; `get_element(sel).text` returns it. |
| launchApp, stopApp, killApp, clearState, clearKeychain (Maestro); `wf android launch`, `install` | Scripts open screens by deep link (`open_link`), and `test --cold-start` kills the app for the warm-up unit. launch and install have no caller besides their own flow test and one prose line in a runbook, which `adb install` covers. The cold restarts in debug-overlay.md:107,122 and theme-switching.md:287 become `run-shell 'am force-stop host.exp.exponent'` followed by `open-link`. |
| eraseText, pasteText, setClipboard (Maestro); `wf browser clipboard` | `press_key('Backspace')` covers the two backspace uses, `get_element(sel).text` returns the text directly, and nothing pastes. The browser verb has only its own test and the skill's verb table. |
| doubleTapOn, longPressOn, scroll (Maestro); `wf browser drag`, `scroll` | No caller. `swipe` and `scroll_until_visible` cover every gesture the scripts make. |
| repeat, retry, runScript, evalScript, runFlow from a file, inputRandom (Maestro) | Lua is the flow language: loops, functions, pcall and require cover these. `run_flow` keeps the label. |
| setPermissions (Maestro); `wf browser permission` | The voice gate taps the system permission dialog as a user does. The browser verb has only its own test and the skill's verb table; its RPC stays. |
| setLocation, travel, setAirplaneMode, toggleAirplaneMode, setOrientation, setDarkMode, toggleDarkMode, assertDarkMode, assertLightMode, waitForAnimationToEnd (Maestro); `wf browser set-device` | No caller. |
| assertWithAI, assertNoDefectsWithAI, extractTextWithAI (Maestro) | `agent.run` covers the one open-ended step, voice-edit's agent mode. |
| Maestro matching details: hintText and error-text matching, clickable-first ordering, notVisible's 500 ms window | Hint text has no UiSelector matcher, so Android would need a full hierarchy dump per lookup, and no script selects by placeholder. A tap on the deepest match lands on its clickable parent anyway. Every `not_visible` follows a step that saw the element, and the window would add about 7 s per full run. Each can be added later under Maestro's semantics without changing the rest. |
| Unused Maestro parameters | No call site needs them: tapOn's repeat, delay, retryTapIfNoChange, waitToSettleTimeoutMs and a point inside an element; swipe's direction, from and waitToSettleTimeoutMs; scrollUntilVisible's speed and centerElement; cropOn on takeScreenshot and assertScreenshot; openLink's autoVerify and browser; the selector's index, state, relational, traits and dimension fields; pressKey names other than Enter, Backspace, Tab and the four Dpad arrows. Each can be added later under its Maestro name without changing anything else. |
| `back` on the browser (history back) | No caller: every back call sits behind Expo Go's developer menu, which never appears on the web. It would need a new RPC to stay in both verb lists, so `back` is Android-only with the usual exit 2. |
| `wf browser eval`, `outer-eval`, `shell-eval`, `wait-observe` (CLI) | They were the Lua driver's transport; the driver now calls the daemon in process. Their reads became typed verbs: `lib/logger.ts:95`'s `__liveTail` read becomes `get-logs --level info`, which also works on Android; device-lab-codemirror-typing.md:24's "eval may read the page" becomes `get-element` and `hierarchy`; keyboard-toolbar-browser-testing.md used eval to focus and blur the editor, which the keyboard rule forbids, so those steps become "tap the editor", then "tap Done". The RPC methods stay for `@lib/browser-client` and cookie-capture, which calls eval, open, navigate, cookies and close. |
| `wf browser open`, `close`, `navigate`, `list` | `open` and `close` become `start-device` and `stop-device --device`, `navigate` becomes `open-link`, and `list` is folded into `list-devices` together with today's `list-devices` presets. |
| `wf browser network-stub`, `network-stub-hits`, `debug-hits`, and `debug-set` as a logpoint verb | Their state now lives only for the run, on both platforms: `add_network_stub`, `get_network_stub_hits`, `get_debug_hits` and `test --logpoint`, so the two CLIs list the same verbs. Their only CLI caller was the Lua driver. `set-breakpoint` keeps debug-set's pausing form. |
| `wf browser network-proxy` | Its only runbook mention (device-lab-voice-edit.md:58) says not to use it. |
| `wf browser cookies`, `memory`, `coverage`, `check-clipping`, `upload`, `opened-urls`, `preflight`, `debug-clear` (CLI) | Only their own flow tests or the skill's verb table call these CLI verbs. The RPC methods stay: cookies for the deployed cookie-capture service, memory and coverage for wf bench. |
| `wf browser audio-inject`, `snapshot`, `video-check`, `goldens` | Renamed, not removed: `set-microphone-input`, `hierarchy`, `check-taps` and `wf flow goldens`. |
| `get_warnings`, `clear_warnings` (the earlier proposal's names) | Renamed to `get_logs` and `clear_logs`. One read covers today's warnings (level warn, the default), the logcat dumps the scripts take with `run_shell`, and logger.ts's live-tail read, the same on both platforms. |
| `wf android text`, `find`, `wait`, `stroke`, `edit-text`, `keyboard`, `warnings --fail`, `help`, and the start, stop and status names that are listed but never dispatched | `text` duplicates `type` without the keyboard check. The driver does not use `find` or `wait`; `get-element` and `extended-wait-until` cover them. `edit-text` and `keyboard` are fields of `get-element` and `get-device-state`. `keyboard --require` and `warnings --fail` become `extended-wait-until` conditions with `--timeout 0`. `help` is replaced by `--help`. `features`, `up` and `down` become `list-devices`, `start-device` and `stop-device`. |
| Options: `--identifier`, `--log`, `--mouse`, `--indicator`/`--no-indicator`, type's fill mode, `--submit` and `--delay`, swipe `--ref` and `--fling`, stroke `--units`, `--step-delay` and the transforms, start `--foreground` and `--port` (browser), navigate `--dev-bridge*`, snapshot `--outer`, tailnet-forward `--middleware`, open `--orientation`, `--cdp-endpoint` and `--http-credentials` | Each is ignored, always refused, or has no caller. The RPC keeps the ones a client passes. Android `start-device` keeps `--screen`, `--name`, `--port`, `--with`, `--slim`, `--stock` and `--expo-go`, which have callers (`app_bench/android.rs:127`, `e2e-tests.yml:154`, `android_features.rs:100`, device-lab-metro-android.md). |
| The `label:` selector kind | Folded into `text`, which matches the accessibility label as Maestro's text does. |
| `WF_BROWSER_CMD` | It was the test seam for the spawned `wf browser <verb>` calls. The driver now posts to the daemon itself, so `tests/flow/harness.rs`'s browser cases double the daemon with `tests/flow/support/mock_daemon.rs`, which records `/call` requests. |

## Infra

### `test` (both platforms; was `run`)

Maestro's `maestro test`.

```
wf browser|android test <script.lua> [--env K=V]... [--publish] [--test-output-dir D] [--no-record]
    [--update-goldens] [--logpoint file:line[:cond]]... [--device D] [--screen-size P] [--frame F] [--cold-start]
wf browser|android test --api [--json]
```

- `--env K=V`: Maestro's `-e/--env` (was `--arg`). The Lua table is `env` (was `args`).
- `--test-output-dir`: Maestro's name (was `--record-dir`).
- `--logpoint` (was `--debug`): it arms logpoints; it is not a debug mode.
- `--screen-size` and `--frame`: browser only; they replace `WF_BROWSER_DEVICE` and `WF_BROWSER_FRAME`. `--screen-size` takes a preset id (iphone-15-pro) where Maestro's takes WxH, because a preset also carries dpr, user agent and bezel; `list-devices` lists the presets. On Android the screen is set by `start-device`.
- `--cold-start`: Android only, because a new browser session is already cold.
- `--api` prints this API from the command table; `--api --json` prints the table itself, which the parity flow test reads.

Callers: `wf test run` (`e2e_run.rs:41-59`), the warm-up unit (`e2e_run.rs:225`), `agent_proof.rs:164`, the test-report skill, the runbooks, and the goal verifiers that grep `run --api` (`docs/goals/2026-10-14-device-lab-fast-green.md:89,94`).

Output: the script's log lines and its `report:`, `bundle:`, `recording:` and `goldens:` lines, then one JSON summary line.

### `start-device`, `stop-device`, `list-devices` (both platforms; Android in CI only)

`start-device` and `list-devices` are Maestro CLI commands (StartDeviceCommand, ListDevicesCommand); `stop-device` is ours, for symmetry. The names say what they start, which bare start and stop next to start-recording did not.
- Browser:
  - `start-device --device N [--screen-size P] [--frame F] [--headless]` starts the daemon if needed and opens session N.
  - `stop-device --device N` closes the session; `--all` closes every session.
  - `list-devices` returns `{daemon:{running, url}, devices:[sessions with owner], screens:[presets]}`. It replaces `wf browser list` and `list-devices`.
- Android:
  - `start-device` (was `up`) `[--name N] [--screen WxH@dpi] [--port P] [--with ...] [--slim] [--stock] [--expo-go] [--dry-run]` (`--print` becomes `--dry-run`). It keeps every option with a caller.
  - `stop-device` (was `down`) `[--name N] [--dry-run]`.
  - `list-devices` (was `features`, plus lease status) `[--name N]` returns `{devices:[{name, serial, boot, restartProven, disabledTotal, lease}]}`.
  - Neither `start-device` nor `stop-device` is lease-guarded.

Callers: `e2e-tests.yml:154,160-161,176`; `app_bench/android.rs:127-134`; `warm-app.lua:41`, which writes `wf android list-devices --json | jq '.result.devices[0]'` so device-features.json keeps its shape for the goal verifiers; root.md:61; fast-green.md:13,18-19,50-52.

### `wf browser start`, `status`, `stop` (browser only, frozen)

The daemon's lifecycle, not a device's. `start [--headless] [--screenshot-dir D]` starts the daemon, `status` returns `{running, url, sessions}`, and `stop` stops it. `@lib/browser-client` and the cookie-capture service (`backend/leetcode-auth-cookie-capture/rs/src/infrastructure/browser_daemon.rs:53-57`) read `result.url` from `wf browser --json status|start`. That service runs the VM's shared wf (its Dockerfile links `/opt/clis/current/wf`), so a rename would break the deployed service before it redeploys. `e2e-tests.yml:111,180` call `wf browser stop`.

### `lease` (Android only)

`wf android lease [acquire|heartbeat|release|status] [--id ID] [--name N] [--wait s] [--ttl ms] [--who W]`: today's form, with `--device` renamed to `--name`, because `--device` now means the serial. `heartbeat` and `--id` are unchanged, as device-lab-metro-android.md:41-44 uses them. One redroid serves every CI job, so jobs queue for it. `wf test run` takes the lease in process (`e2e_run.rs:128-137`), and `wf android test` renews it every 60 s.

### `hierarchy` (both platforms)

Maestro CLI's `maestro hierarchy`. It prints the screen's elements as JSON `{id, text, label, value, bounds, visible, frame}`, so an agent can write a selector. Every failure also writes it as evidence. It replaces `wf browser snapshot` (used by the skill and docs/solutions) and the unused `wf android find`.

### `adb` (Android only)

Raw passthrough for the adb transport (driver internals, runbooks). It is now lease-guarded, and its output is not wrapped in the envelope.

### `tunnel` and `tailnet-forward` (browser only)

They let the agent's machine reach tailnet services, in two different ways.
- `tunnel [services...]` publishes the deployed VM services on localhost ports (remote port + 10000) through tailscaled's userspace SOCKS5 listener. It stays in the foreground, because a backgrounded tunnel serves a stale mapping after the VM is recreated (`browser_tunnel.rs:1-4`).
- `tailnet-forward --host H --port P [--local-port L]` forwards one arbitrary host:port through the daemon's tailnet access, for a page loaded in the device frame (`browser.rs:953`).

Callers: the codemirror-typing, tamil-writing and voice-edit runbooks, and the skill. The Android device already sits inside the tailnet.

### `check-taps <video> [--inputs inputs.jsonl]` (browser only; was `video-check`)

A post-run check that every tap in the input log shows its dot in the recording. Three device-lab runbooks call it. Android videos show no touches.

### Interactive debugger (browser only)

`set-breakpoint <file:line[:cond]>` (was `debug-set --pause`), `debug-pause`, `debug-step [over|into|out]`, `debug-eval <expr>` and `debug-resume`. Kept for the CLAUDE.md rule: breakpoints, stacks at original lines, and locals at the stop (owner, 2026-10-20). `debug-eval` keeps its name, because the fast-green p9 verifier greps it and it is not ambiguous. Each verb's help says why Android has only logpoints: Android's Hermes session lives inside the test, where `test --logpoint` and `get_debug_hits` give the same evidence.

### `wf flow goldens` (moved from `wf browser goldens`)

Renders the flow tests' DOM snapshots and compares them with golden PNGs, with no device. Callers: `clis/harness-flow/src/flow_run.rs` and `frontend/libraries/flow-test-harness`.

### `wf browser-daemon` and its HTTP API

`GET /health`, `GET /status`, `POST /stop`, `POST /call {method, args}`: frozen for `@lib/browser-client`, the cookie-capture service, wf bench and the frontend e2e scripts. Only additive changes are allowed: `tap {x, y}`, a swipe duration, `consoleLogs`, `recordStart`, `recordStop`, `audioInject replace:true`, and `open`'s `owner`.

### `--json`

On every `wf browser` and `wf android` verb: kept as a no-op, before or after the verb, because the output is always the envelope and the two daemon clients send it.

### Lua globals

- `log(...)`
- `env` (was `args`): the `--env` values
- `json.encode`
- `now_ms()`
- `sleep_ms(ms)`: a last resort, for real-time media
- `add_timing_span(label, from_ms, to_ms)`: was `device:span`; splits warm-app's cold start
- `run_flow` and `assert_true` (see [Commands](#commands))
- `agent.run{prompt, model, timeout, json}`: voice-edit's agent mode
- `pcall` and `xpcall`, wrapped so that a caught error still honours a run stop
- `device.platform` (`'android'` or `'browser'`): Maestro's MAESTRO_PLATFORM
- the standard `io` and `os` libraries: voice-edit opens its wav files, and warm-app writes device-features.json

### Environment variables

- `WF_WAIT_BUDGET_MS`: the default wait and tap timeout (10000)
- `WF_OPTIONAL_LOOKUP_MS` (new, 1000): the default for optional lookups
- `WF_WAIT_DIAGNOSE_MS` (new, 8000): the look that tells slow from broken
- `WF_WAIT_LOG` and `WF_RUN_TIMING`
- `WF_BROWSER_SCREEN_SIZE`: was `WF_BROWSER_DEVICE`; the old name exited 2 naming the new one until step 8 and is ignored since
- `WF_BROWSER_FRAME` and `WF_BROWSER_TAILNET`
- `WF_BROWSER_NO_START`: attach to a running daemon and never start one (the flow tests' mock daemon)
- `WF_CHROMIUM`: `@lib/browser-client` passes it to start
- `WF_ANDROID_NAME`: the container and lease name, default `rdl`; was `WF_ANDROID_DEVICE`, which exited 2 naming the new one until step 8 and is ignored since
- `ANDROID_SERIAL` and `WF_ANDROID_SERIAL`: the defaults for `--device`
- `WF_ANDROID_LEASE_ID`, `WF_ANDROID_LEASE=none` (no guard, for a sandbox or a test), `WF_LEASE_WHO` and `WF_TEST_NO_LEASE`
- `WF_ANDROID_UIA=dump`: the hierarchy-dump fallback, which the flow tests use
- `WF_ANDROID_STUB_PORTS` and `WF_ANDROID_INPUT_LOG`
- `WF_METRO_URL`: the Hermes inspector for `--logpoint`
- `WF_TEST_PUBLISH` and `WF_AGENT_CMD`
- `WF_BROWSER_CMD` is removed (see [Dropped](#dropped))

## Migration

One commit per step, each through lefthook, each landed on `dev` and `claude/device-lab`. Steps H0 to H8 in the ledger are these steps. Until step 7 rewrites the scripts, every old Lua name keeps working as a thin wrapper over the new command, so the e2e scripts run unchanged; renamed CLI verbs exit 2 with `{error:'renamed', use:'<new verb>'}` from their rename step until step 8.

### 0. The spec

This file and [contract.md](contract.md), the Harness API block of the ledger, and goal criteria h0, h1, h7 and h8.

### 1. Contract first, no behaviour change

- One help file per verb under `clis/wf/src/help/`. Browser verbs have none today and exit 2 on `--help`.
- `test --api` prints this API from the command table, and `test --api --json` the table itself (`run --api` keeps working until step 7). Regenerate `docs/reference/wf-cli.md`.
- A parity flow test in `clis/wf/tests/flow/surface.rs`: `wf browser <verb> --help`, `wf android <verb> --help` and `test --api --json` list the same verbs, parameters and result schemas, and every Lua method maps to a CLI verb or has a documented reason to be Lua-only or CLI-only.
- `wf check --dead-refs` checks `wf <noun> <verb>` against each noun's verb table and `device:<m>` against the binder; today it checks only the first word (`dead_refs.rs:307-309`). It reads docs/, .agents/, .github/, e2e/ and `docs/goals/*.md`, because goal verifiers are callers. On today's tree it should flag the broken `wf android status <x,y>` runbook lines, `press --key`, `install-expo`, `shell-screenshot` and `wf browser app-flows` (`verify.rs:506`).

### 2. The shared command layer, the envelope and the lease

No renames yet.
- One typed Rust function per command per driver (`clis/wf/src/device/{mod,browser,android}.rs`), called in process by both the Lua binder (`lua.rs`) and the CLI. The browser function posts to the daemon's `/call`; the Android function talks to adb and the uiautomator2 server.
- `WF_BROWSER_CMD` is removed. `tests/flow/harness.rs`'s browser cases move to `tests/flow/support/mock_daemon.rs`, with `WF_BROWSER_NO_START` so the test attaches to the mock.
- The layer runs `android_lease::guard()` before every guarded command, and the guard renews the lease; `wf android run` renews it every 60 s. A flow test: a run with a short TTL, held across a read-only wait longer than the TTL, still holds its lease.
- `json_mode.rs` writes the envelope for `wf browser` and `wf android` only, and no longer spreads the result into the top level. In the same commit, every reader: `e2e-tests.yml:161` reads `.result.*`; `warm-app.lua:41` writes the device entry, so device-features.json keeps its shape; `docs/goals/2026-10-14-device-lab-fast-green.md:18-19` reads `.result`; vm_exec's lease relay reads `.result`; `tests/flow/support/browser_session.rs`.
- Browser `start` and `status` keep `result.url`. Verified with `pnpm --filter @lib/browser-client run test:unit` and cookie-capture's flow test (`backend/leetcode-auth-cookie-capture/rs/tests/flow.rs`).

### 3. Selectors, the reads and the wait

- `Sel` becomes `{id, text, css, point}`. Every pattern is parsed with regex-syntax and checked against the shared grammar. Each engine gets its own form: Android's UiSelector `textMatches`, `descriptionMatches` and `resourceIdMatches` (replacing `textContains` and `descriptionContains`), a JS RegExp on the browser, a Rust regex on the dump fallback.
- Matching follows Maestro's rule: case-insensitive, literal equality, a newline also read as a space; `text` also matches the label and an input's value; the deepest match wins.
- Bounds are clipped to the screen and to clipping ancestors. The browser resolves selectors in read-only page JS; the `data-wf-text-target` tagging goes. Old prefixed strings raise and name the table form.
- `get_element` and `get_device_state` land, with the flow test that `get_element({id='CodeEditorInput'}).text` is the buffer on both drivers.
- A flow test runs the same patterns on all three engines, including one that Rust accepts and JavaScript rejects, and the literal fallback.
- `extended_wait_until`, `assert_visible` and `assert_not_visible` land beside the old waits, which become thin wrappers over them. They bring the evidence object (a screenshot plus a hierarchy dump), `WF_WAIT_DIAGNOSE_MS` (skipped when the timeout is 0 and for optional waits), `WF_OPTIONAL_LOOKUP_MS`, the span names, `call_outcome` counting optional waits as lookups, and the new `WF_WAIT_LOG` labels. Flow tests: a condition that acts raises; an optional miss returns nil with no evidence.
- The byVerb consumers in `test_results.rs` and `test_publish.rs` are renamed, and the break in the trend lines is logged.

### 4. Additive daemon RPCs

`tap {x, y}` beside `ref`; a swipe `durationMs`; `consoleLogs` (CDP `Runtime.consoleAPICalled` and `exceptionThrown`, buffered per session from open); `recordStart` and `recordStop` over `cdp::record`; `audioInject replace:true`, which empties `fake_mic.js`'s queue and consumed list; an `owner` argument on `open`. Every other RPC method and argument stays unchanged, including the open, navigate, eval, cookies and close calls that cookie-capture makes.

### 5. Browser CLI renames, with every in-repo caller

- Renames: tap → `tap-on`; type → `input-text`; press → `press-key`; navigate → `open-link`; trace → `draw-strokes-on`; screenshot → `take-screenshot` or `assert-screenshot`; audio-inject → `set-microphone-input`; snapshot → `hierarchy`; warnings reads → `get-logs`; video-check → `check-taps`; goldens → `wf flow goldens`; open, close and list → `start-device`, `stop-device` and `list-devices` (which also lists the presets); `--name` → `--device`; `--device <preset>` → `--screen-size`; `WF_BROWSER_DEVICE` → `WF_BROWSER_SCREEN_SIZE`; `debug-set --pause` → `set-breakpoint`. `start`, `status` and `stop` stay as they are.
- New verbs: `get-element`, `get-device-state`, `get-pixel`, `clear-logs`, `scroll-until-visible`, `extended-wait-until`, `assert-visible`, `assert-not-visible`, `start-recording`, `stop-recording`; `back` and `run-shell` listed, exiting 2.
- Drop the verbs listed under [Dropped](#dropped).
- Callers: `device/browser.rs`, `clis/harness-flow/src/flow_run.rs`, `frontend/libraries/flow-test-harness` (flowReporter.ts and its README), `tests/flow/browser.rs`; `.agents/skills/browser-cli` and `test-report`, `libraries/hook-runtime/src/deny.json:37`; `frontend/mla-tamagui/lib/logger.ts:95` (the outer-eval `__liveTail` read becomes `wf browser get-logs --level info`); the browser runbooks (codemirror-typing: arrows by Maestro's Dpad names, and "eval may read the page" becomes `get-element` and `hierarchy`; tamil-writing and voice-edit; keyboard-toolbar: eval focus and blur become "tap the editor", then "tap Done"); 12 docs/gaps files, 2 docs/solutions files, README.md; `capture-leetcode-shots.mjs:96,121,213` (setKeyboard becomes a tap on Done).
- Then `pnpm run sync:agent-config`.

### 6. Android CLI renames, with every caller

- Renames: tap → `tap-on`; type and text → `input-text`; press → `press-key` or `back`; open → `open-link`; trace → `draw-strokes-on`; screenshot → `take-screenshot`; keyboard → `get-device-state keyboard`; edit-text and find → `get-element`; warnings → `get-logs` and `clear-logs`; record start and stop → `start-recording` and `stop-recording`; adb shell → `run-shell`; up, down and features → `start-device`, `stop-device` and `list-devices` (`--print` → `--dry-run`; every other option kept); `--serial` and `--identifier` → `--device`; lease `--device` → `--name`; `WF_ANDROID_DEVICE` → `WF_ANDROID_NAME`.
- New verbs: `get-element`, `get-device-state`, `get-pixel`, `set-microphone-input`, `scroll-until-visible`, `extended-wait-until`, `assert-visible`, `assert-not-visible`, `hierarchy`.
- The lease guard grows to the list in [Principles](#principles); `start-device` and `stop-device` stay unguarded.
- Callers: `e2e-tests.yml:154,160-161,176` and its comments; `app_bench/android.rs:127-134`, `e2e_run.rs:51`, `warm-app.lua:41`, `help/wf__ship.txt`; `.agents/instructions/root.md:61`, which becomes "never `wf android start-device|stop-device|lease`"; `.agents/skills/runbook-automate`; the Android runbooks (device-lab-*-android.md: `warnings --fail` and `keyboard --require` become `extended-wait-until --timeout 0`, record start and stop become `start-recording` and `stop-recording`; metro-android: run becomes test, its lease lines unchanged; agentation; debug-overlay.md:107-122 and theme-switching.md:238-287: `status <x,y>` becomes `tap-on --point`, `status --url <u>` and `status <bundleId>` become `run-shell 'am force-stop host.exp.exponent'`, then `open-link`); `tests/flow/android.rs`, `docs/known-gaps.md`, README.md:28; the goal verifiers at fast-green.md:13 (`down --print` becomes `stop-device --dry-run`) and :50-52 (grep `start-device` and `stop-device`).
- Then `pnpm run sync:agent-config`.

### 7. The Lua binder, `run` → `test`, and all 11 e2e scripts

The binder exposes exactly the command table, and the old names raise `renamed: use device:<new>`. The script rewrite is one `wf offload ask --batch e2e/*.lua` unit with this mapping, then reviewed:
- `open` → `open_link`
- `tap(s)` → `tap_on(sel)`; delete a preceding wait on the same selector; `pcall(tap)` by its three rules (see `tap_on`)
- `type_text` → `input_text`
- `press('back')` → `back()`; `press(k)` → `press_key(Name)`
- `shell('input keycombination 113 123')` → `press_key('Ctrl+End')`
- `find` used as a boolean → `get_element(X)` in the condition; otherwise → `get_element(X).bounds`
- `text` → `get_element(X).text`; the `buffer()` helpers → `get_element({id='CodeEditorInput'}).text`; `keyboard_shown` → `get_device_state('keyboard').visible`
- `shell('dumpsys window')` and the browser's `rail ~= nil` → `get_device_state('keyboard').bounds`
- `pixel(x, y)` → `get_pixel(('%d,%d'):format(x, y))`
- `shell('wm size')` → `get_device_state('screen')`; the literal 393 → `get_device_state('screen').width`
- `trace(s, src, h)` → `draw_strokes_on(sel, {source=src, human=h})`; `pcall(trace)` → `optional=true`
- `audio_inject(w)` → `set_microphone_input(w)`; `wait_recording` → the condition in wait rewrite 7
- `screenshot(path)` → `take_screenshot(path)`; `screenshot(name[, o])` → `assert_screenshot(name[, o])`; web-goldens.lua:11's ratio → `match_percentage`
- `stub(m, p, {file=, headers=})` → `add_network_stub(m, p, {body_file=, headers=})`; `stub_hits` → `get_network_stub_hits`
- `debug_hits` → `get_debug_hits`; `warnings` → `get_logs`; `clear_warnings` and `shell('logcat -c')` → `clear_logs`; the logcat ReactNativeJS dumps → `get_logs{level='debug'}`; other `shell` → `run_shell`
- `device:step` → `run_flow`; `device:span` → `add_timing_span`; `assert_that` → `assert_true`; `args` → `env`
- `wait` and `wait_slow` → `assert_visible`; `wait_gone` → `assert_not_visible`; the other waits and the sleep poll loops → the rewrites in [Waiting](#todays-waits-rewritten); `sleep(clip_s)` → `sleep_ms(clip_s*1000)`
- `swipe(x1, y1, x2, y2, ms)` → `swipe(('%d,%d'):format(x1, y1), ('%d,%d'):format(x2, y2), {duration=ms})`; the hand-written scroll loops → `scroll_until_visible` with the toolbar row as `from`
- every selector → the [Selector](#selector) form, with text escaped and wrapped in `.*`

In the same commit:
- `run` → `test`, `--arg` → `--env`, `--record-dir` → `--test-output-dir`, `--debug` → `--logpoint`; `--goldens` is dropped. Update `e2e_run.rs:41-59,225`, `agent_proof.rs:164` and its TASK prose, debugger-probe.lua:10-11, device-lab-debugger-probe-android.md, wf__test.txt:38, the e2e-tests.yml dispatch description, and the error text at `device/android.rs:159`.
- `tests/flow/harness.rs`; `agents.rs:242` (the prompt now names `extended_wait_until`); the test-report skill's notes.
- The goal verifiers at fast-green.md:87-94: `wf android test --api | grep device:assert_screenshot`, Hermes and debugStatus.

Before landing, the goldens, because `assert_screenshot` now compares in every CI run. Today CI never compares (deploy.yml passes e2e-tests.yml no flags, and only `--goldens` compares); two-sum-before-run and two-sum-stubbed-result have no golden on either platform; and the Android goldens may predate the 480x1040 screen. So: dispatch e2e-tests.yml on the branch for both platforms with `--update-goldens`, pull the goldens with `wf test goldens`, review every image, commit them, and only then land.

### 8. Remove the renamed stubs

Once `wf check --dead-refs` is clean and both CI e2e jobs are green on `dev`: the CLI's renamed exit 2 and the Lua `renamed:` errors go. The browser daemon's `start`, `status` and `stop` are not stubs and stay.

## Verification

Reported in the repo's order (runbook, browser and Android e2e, deployment verification, integration, flow, unit).
- Flow tests: `cargo test -p wf` plus the new ones (parity, swipe, lease renewal over a long read-only wait, the regex grammar on three engines with the literal fallback, a test-owned session refused, a clipped tap point, a condition that acts, an optional miss, the editor buffer through `get_element`); `@lib/browser-client` test:unit; cookie-capture's flow test; `wf check --dead-refs` at 0.
- Browser e2e: every `e2e/*.lua` with `wf browser test --publish` against a local `expo export --platform web`. Each run publishes its report and a recording; screenshots go to the owner. `pnpm --filter mla-tamagui run test:e2e` for the RPC clients.
- Android e2e: only CI's android-e2e-tests bundles, with no device verb run by hand. G1 to G3 green on both platforms on `dev`, after confirming that the remote head equals the pushed commit.
- Read the bundles for the known behaviour flips. Until step 7 the old Lua names keep today's answers where a command would answer differently (the review of steps 1 to 4): on the browser `find`, `text` and `editor_text` read the DOM whatever shows, with the whole box; `warnings` returns the warn and error lines logged since the last `open`; `press`, `press('back')` and `type_text` add no keyboard refusal. So the flips below land with the script rewrite, except the selector one, which the old names already take:
  - text selectors: substring to full match, now case-insensitive, and now matching aria-label, content-desc and input values (live through the old names since step 3; the old `text:X` reads as `.*X.*`);
  - `find` → `get_element` on the browser, which now needs the element on screen: voice-edit.lua:120,204 (rails), :223,227 (Send); editor-typing.lua:254-263 (the rail, now a real keyboard check), :315 (the debug log lines now scroll into view); stubbed-run-local.lua:17; and the coordinate reads of an off-screen element: voice-edit.lua:53-56, editor-typing.lua:135-136 and stubbed-run-local.lua:32-33 read `r.cy` of a Done key past the screen's right edge (nil now, so the swipe falls back to its fixed row), and stubbed-run-local.lua:45 and agent-proof.lua:21 read the run totals below the fold;
  - `warnings` → `get_logs`: lines logged while the bundle loads and in an earlier document now count, at app-routes.lua:25, web-goldens.lua:13, metro-smoke.lua:22, debugger-probe.lua:125 and tamil-writing.lua:51, which assert none;
  - `press('back')` → `back()` on Android, refused while the keyboard is up: warm-app.lua:17, debugger-probe.lua:34, metro-smoke.lua:18, tamil-writing.lua:40;
  - waits that now fail where they used to pass silently: editor-typing.lua:142 (keyboard) and :299 (toast pixel);
  - tamil-writing.lua:134, which now waits 30 s;
  - voice-edit's `heard`, which is no longer empty on the browser;
  - the first CI golden comparisons.
- Each unit's wall time against its last green bundle (n=1).
- Benchmark per CLAUDE.md: `wf bench agent-proof` on the parent commit and the new one, n=1 on the Sonnet arm, then the opencode arm, with the scripted arm as the control; n=3 only where the two disagree. The numbers, the decision and the break in the byVerb and wait-label trends go in `docs/audits/2026-09-02-token-diet-decision-log.md`.
