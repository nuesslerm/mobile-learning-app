# multi-browser and the vnc-auth listener: plan for the Rust replacement

Status: step 1 is merged: `clis/cdp` (a std-only DevTools client plus the attached-session engine, 14 tests against real Chromium) and the opt-in Rust daemon (`WF_BROWSER_DAEMON=rust`, `wf browser-daemon`), with the 13-test `browser-attach` suite green on the Node daemon and on the Rust one (daemon 184.5 MB to 4.8 MB, flow 1657 ms to 370 ms). Steps 2 to 5 are not built.

## What has to be replaced

- Daemon protocol (`clis/multi-browser/src/data/daemon.ts`): HTTP on loopback with `GET /health`, `GET /status`, `POST /stop`,
  `POST /call {method,args}` answering `{ok,result}` or `{ok:false,error}` with HTTP 200, and the device-frame SPA under
  `/_mcp/device-frame` (static files built from `harness-shell`, which is browser-side React and stays TypeScript source that
  is built to static assets, not CLI code).
- The Rust client for this protocol already exists (`clis/wf/src/browser.rs`, 53 black-box tests), so a Rust daemon is
  testable by the same suite once a browser is real instead of stubbed.
- 40 commands in `src/entrypoints/cli.ts`; the engine behind them is `src/data/browser/session.ts` (3.3k lines of Playwright:
  locators, snapshots, touch, tracing, video, coverage, memory).

## Strangler order (each a PR with its own black-box suite, TS first then Rust)

1. Attach-only sessions: `open --cdp-endpoint`, `navigate`, `eval`, `screenshot`, `cookies`, `list`, `close`. This is exactly
   what `vnc-auth-capture`'s desktop launcher uses (attach to the kiosk Chromium, apply mobile emulation, navigate, dismiss
   the cookie banner, read cookies), and it needs no Playwright feature beyond raw CDP.
2. Input: `tap`, `type`, `press`, `scroll`, `swipe`, `stroke`, `drag` (CDP `Input.dispatch*`).
3. Launch: Chromium launch flags and device presets (`browser_devices.json` is already embedded in the Rust client).
4. Observation: `snapshot` (accessibility tree), `check-clipping`, `memory`, `coverage`, `trace`, `video-check`, `goldens`
   (image diff replaces pixelmatch and pngjs).
5. Then the vnc-auth listener (HTTP routes, token gate, KasmVNC/tailscale-serve driving) on top of the Rust attach layer.

## Routing during the migration

A browser session lives in one process, so commands cannot be split across a Rust and a Node daemon for the same session.
Route by session: a session opened with `--cdp-endpoint` belongs to the Rust daemon, any other to the Node daemon, and a
command the Rust daemon does not implement yet answers an explicit error naming the command (never a silent fallback).

## Why this is not started in this change

Each step needs a real Chromium (available here) and, for step 5, KasmVNC, X11 and tailscale (not available here and not
stub-faithful). Steps 1 to 4 can be done and tested in this sandbox; step 5 and the image builds that carry the result
cannot be verified here.

## Revision after step 1: who needs which step

A search of the repo for consumers shows the input commands (`tap`, `type`, `press`, `scroll`, `swipe`, `stroke`, `drag`) are
used on *launched* device-frame sessions (flow tests, agents driving the app), not on attached ones: the only attached
consumer, the vnc desktop launcher, uses `open`, `navigate`, `eval`, `cookies`, `screenshot`, `emulateMobile`,
`fullscreenWindow` and `checkLaunchInvariants`, all but the last now in Rust. So the order that removes TypeScript fastest is:

1. Port `checkLaunchInvariants` (CDP and X11 measurement) into the attached engine, then the vnc listener itself
   (HTTP routes, link tokens, the token-gated proxy, tailscale serve binding, the agent-shell relay) on top of it. This
   deletes `clis/vnc-auth-capture`. Stub-testable (tailscale, X11 tools) plus a real Chromium; the KasmVNC desktop and the
   Docker image stay unverifiable here.
2. Launch and the device frame (step 3): needs Playwright's context emulation reproduced in CDP, the mock keyboard, and the
   `harness-shell` page. The harness is browser-side React: it can be built to static files and shipped as assets, but its
   TypeScript source stays in the repo, so "no TS under clis/" needs it moved out of `clis/` (decision for the owner).
3. Input and observation (steps 2 and 4) on launched sessions, with the page's own event log as the oracle (a test page
   records clicks, keys and scrolls; both daemons must produce the same log).

## Finding on `snapshot` (step 4) and the ref-based input commands

Measured on real pages through the Node daemon: `snapshot` prints Playwright's AI aria snapshot, not Chrome's accessibility
tree. Its rules are Playwright's own and are visible in the output: roles Chrome does not name (`generic`, `paragraph`,
`text:` nodes, `rowgroup`), names computed by Playwright's accname implementation (a `<label>` wrapping an `<input>` yields a
`generic` with a `text:` child and a `textbox "Email"`), `[cursor=pointer]` and `/url:` children for links, states
(`[checked]`, `[selected]`, `[disabled]`, `[active]`), frame-prefixed refs (`f1e3`) that its `aria-ref=` selector engine
resolves, and `tap`/`type`/`stroke`/`drag` all resolve through that engine. Two honest ways forward, to be chosen by the owner
because both have a cost:

1. Vendor Playwright's injected aria-snapshot script (Apache-2.0, bundled as a string inside `playwright-core`'s
   `coreBundle.js`) into the Rust crate and run it over CDP: exact output, refs usable by the input commands, but a vendored
   JavaScript blob that must be refreshed with each Playwright upgrade.
2. Write a native snapshot (DOM walk) with a documented subset of roles and names and its own `eN` refs, consumed by the Rust
   `tap`/`type`: smaller, no vendored script, but output that differs from Playwright's outside the fixtures it is tested on,
   which agents reading snapshots would notice.

Until one is chosen the Rust daemon answers these commands with "not implemented" and `WF_BROWSER_DAEMON=rust` stays opt-in.

## Status after the attached-session work, and the sizing of what is left

Merged and tested on both daemons (`browser-attach`, 24 tests): `open --cdp-endpoint`, `close`, `list`, `status`, `stop`,
`navigate`, `eval`, `cookies`, `screenshot`, `scroll`, `snapshot` (documented subset), `tap`, `type`, `press`, `drag`,
`stroke` (deterministic cases) and the `/_mcp/device-frame` static route.

Left in `multi-browser`, with the work each needs (the CLI always opens device-frame sessions, so none of it is optional for
replacing the Node daemon):

| Piece | Size in TS | What the Rust side needs first |
| --- | --- | --- |
| Launch (`launch.ts`, `fonts.ts`, device presets, emulation flags, context options) | about 500 lines | Chromium launch with the pixel-ratio and site-isolation flags, `Target`/`Emulation` per page, the platform-font config |
| Device frame (`device-frame.ts`, shell bridge, `getAppTarget`) | about 300 lines plus the React shell | Frame and execution-context tracking over CDP (`Page.getFrameTree`, `Runtime.executionContextCreated`, isolated worlds), because the app runs in nested cross-origin iframes that Playwright resolves for every `eval`, `tap` and `type` |
| Touch input (`touch-input.ts`, `swipe`, `trace`, tap indicators) | about 900 lines | `Input.dispatchTouchEvent`, the human-motion model (random tremor, so only counts are comparable), indicator DOM injection |
| Observation (`memory`, `coverage`, `video`, `check-clipping`, `goldens`) | about 1500 lines | `Performance`/`Profiler` domains, screencast and ffmpeg, an image diff replacing pixelmatch and pngjs |
| Mock keyboard and input injection (`soft-keyboard.ts`, fake mic and camera, clipboard, upload, permissions) | about 700 lines | Init scripts via `Page.addScriptToEvaluateOnNewDocument`, `Browser.setPermission`, file chooser interception |

The frame tracking is the long pole: every ref-based command in device-frame mode goes through it. It is also the part the
attached daemon does not need, which is why attached sessions were done first. After it, the order above is by dependency.
`harness-shell` (React) remains browser-side TypeScript to be built to static files and moved out of `clis/`.

## Launched sessions: what reading the TypeScript showed

`open` without `--cdp-endpoint` is a chain of nine things, each observable by a flow test, which is why it cannot be landed
half: (1) Chromium launch with the device-scale and site-isolation flags and the fake media flags; (2) the context options
(viewport, device scale factor, mobile and touch, the identity user agent, colour scheme, clipboard permissions, ignore HTTPS
errors, optional video); (3) closing every other page and context; (4) page bindings (`__mcpOpenedUrl`,
`__mcpSetColorScheme`, `__mcpFocusChange`) that call back into the daemon; (5) init scripts (fake microphone, fake camera,
opened-URL capture, focus-driven auto keyboard); (6) native-network routing; (7) the harness navigation, the wait for
`window.__shell`, the fit of the window to the device box, and `emulateDevice` (safe-area and identity overrides on the page
and on the app frame); (8) key polling that mirrors the mock keyboard; (9) video finalisation on close.

Landed as groundwork: the device registry the Rust side needs (`clis/wf/src/browser_device_specs.json`, 11 presets with
viewport, dpr, safe area, platform and identity), guarded against drift by `tests/unit/rustDeviceSpecs.test.ts` in
`multi-browser`; the frame layer, the device-frame helpers and the static route (see the decision log). Not started: items 1
to 9. The oracle for them is both daemons opening the same preset and reporting, from inside the app frame, `innerWidth`,
`innerHeight`, `devicePixelRatio`, `navigator.userAgent`, `navigator.maxTouchPoints`, the safe-area insets and
`prefers-color-scheme`.
