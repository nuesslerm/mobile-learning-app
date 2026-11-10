---
paths: frontend/**, clis/harness-browser/src/*.rs, clis/cdp/**
name: browser-cli
description: Use when interacting with a local or remote web app through this repo's `wf browser` CLI, especially for Expo routes, device-frame verification, fake-keyboard checks, screenshots, and browser-driven debugging.
license: MIT
metadata:
  origin: custom
  category: browser-automation
  opencode: supported
  claude_code: supported
  compatibility: OpenCode, Claude Code
---

# browser-cli

> Driving this CLI to find out whether a feature *works* — rather than to run a
> known check — is its own method: delegate it to a subagent with a goal statement
> and promote the findings into tests. See the `browser-exploration` skill.

Use the `wf browser` CLI in this repo for browser-driven verification. Its device verbs are Maestro's commands in
kebab-case, the same verbs and JSON answers as `wf android` (`wf browser <verb> --help` for one, `wf browser test --api`
for the Lua side, docs/plans/harness-api/spec.md for the design).

Default mental model:
- The daemon persists across CLI calls; a session (a device) stays open until `wf browser stop-device` or `wf browser stop`.
- The default experience is mobile-first: device frame on, fake keyboard on.
- A verb drives the only open session, or the one `--device <name>` names. A session a running test opened is driven only
  when `--device` names it, so a verb typed by hand never races a test.
- Every verb prints one JSON line: `{ok, command, platform, device, ms, result}` (exit 0); a failure
  `{ok:false, error, evidence, next}` (exit 1); a usage error (exit 2). `--json` is accepted and ignored.
- Elements are found by selector, never by a ref: `--id <testID regex>`, `--text <regex>` (also the accessibility label
  and an input's value), `--css <selector>`, or one bare string meaning `--text`. A pattern is a case-insensitive full
  match or literal equality (`Run (local)` finds that text). `wf browser hierarchy` lists what is on screen to write one.

## Look at BOTH colour schemes

`--color-scheme` sets what the page is told the OS prefers, emulated at the
browser context so it is in force before the app's first paint.

**The default, `system`, is LIGHT** — that is what headless Chromium answers, and
it does not depend on the host's own appearance. So a dark-theme app is rendered
light by default, and a whole class of defect is invisible: a component that
never sets its own colour looks correct on white and disappears on black. Four
such defects in `mla-tamagui` passed seven green suites and were found only by a
human looking at a phone.

```bash
wf browser start-device --device primary --color-scheme dark
```

Treat the two schemes as two renderings of the same screen, both worth a
screenshot — not as a light default with a dark variant.

## Recording a session

```bash
wf browser start-recording /tmp/run.mp4
# ... drive it ...
wf browser stop-recording
# -> {"ok":true,"command":"stop-recording",...,"result":{"files":["/tmp/run.mp4"]}}
wf browser check-taps /tmp/run.mp4
```

Output is H.264 `.mp4`, which plays inline in a chat client on a phone. Beside it lands `<video>.inputs.jsonl` (every
tap, swipe, stroke and microphone clip, with its time), and `check-taps` fails when a logged tap shows no dot. A Lua
test (`wf browser test`) records itself from its first `device:open_link` to its end.

**Record when the question is about a sequence, screenshot when it is about a
frame.** A still cannot show which interaction raced, what flashed and went, or
what order things happened in — and those are exactly the questions an
exploration is usually asking.

## Commands

| Need | Verb |
| --- | --- |
| a device | `wf browser start-device --device <name> [--screen-size <preset>] [--color-scheme light\|dark\|system] [--frame screen\|bezel\|document] [--tailnet] [--plain]`; `wf browser list-devices` lists the sessions and presets; `wf browser stop-device [--device <name> \| --all]` |
| open a page | `wf browser open-link <url>` — a full URL: a local export (`http://127.0.0.1:8800/mla-tamagui/leetcode/two-sum`; `wf test prepare --web` builds it) |
| see the screen | `wf browser hierarchy` (elements with id, label, text, bounds); `wf browser take-screenshot <path>`; `wf browser assert-screenshot <golden name>` compares with `docs/runbooks/goldens/browser/` |
| read one element | `wf browser get-element --id <re>` → `{bounds, text}` or null (text is a field's value; the code editor's buffer for `--id CodeEditorInput`) |
| read the device | `wf browser get-device-state [screen\|keyboard\|microphone]`; `wf browser get-pixel <x,y>` |
| tap | `wf browser tap-on --id <re>` (waits for it, then taps inside its on-screen part); `--point <x,y>` taps a point |
| type | `wf browser input-text <text>` at the caret of the focused field (refused while the keyboard is down: tap the field first) |
| keys | `wf browser press-key Enter\|Backspace\|Tab\|"Remote Dpad Left"\|"Remote Dpad Right"\|"Remote Dpad Up"\|"Remote Dpad Down"\|Ctrl+End` |
| gestures | `wf browser swipe <x,y> <x,y> [--duration 400]`; `wf browser scroll-until-visible --element-id <re> [--from-id <re>] [--direction DOWN\|UP\|LEFT\|RIGHT]` |
| wait | `wf browser assert-visible --id <re> [--timeout ms]`, `wf browser assert-not-visible ...`, or `wf browser extended-wait-until --condition '<Lua expression over device:get_element or device:get_device_state>'` |
| logs | `wf browser get-logs [--level debug\|info\|warn\|error]` (the app's console since the session opened); `wf browser clear-logs` |
| microphone | `wf browser set-microphone-input <clip>` — see "Injecting audio on demand" |
| writing pad | `wf browser draw-strokes-on --id <pad> --source guide\|template [--human=false]` |
| debugger | `wf browser set-breakpoint <file:line[:cond]>`, `wf browser debug-pause`, `wf browser debug-step [over\|into\|out]`, `wf browser debug-eval <expr>`, `wf browser debug-resume` |
| tailnet | `wf browser tailnet-forward --host <h> --port <p> [--local-port <n>]`; `wf browser tunnel [services...]` |
| daemon | `wf browser start`, `wf browser status`, `wf browser stop` |

The verbs from before the rename (tap, type, press, navigate, open, close, list, screenshot, snapshot, trace,
audio-inject, video-check, debug-set, goldens) are unknown verbs now: exit 2 with `{error:"unknown verb '<verb>'", verbs}`.
Those dropped (eval and its siblings, network stubs and proxy, cookies, memory, coverage, permission, clipboard, upload,
drag, scroll, set-device, stroke, ...) exit 2 with `{error:"dropped", why, use}`; their daemon RPC methods stay for
@lib/browser-client, the cookie-capture service and `wf bench`, and a test stubs the network with
`device:add_network_stub`.

## The phone input model

A phone preset (`--screen-size iphone-15-pro`, `pixel-8`, ...) is a phone, so the driver acts like a finger and a thumb:

- **Taps are touches.** `tap-on` is a touch tap with a dot, inside the middle of the part of the target that is on the
  screen; a target off the screen is not visible, so `scroll-until-visible` brings it in first.
- **The keyboard moves only by user action.** Tapping a field raises it; tapping Done lowers it
  (`wf browser tap-on --id EditorAction-Done`). No verb shows or hides it otherwise, and `input-text` and the editing
  keys refuse while it is down. The toolbar above it is wider than the screen: `scroll-until-visible` with
  `--from-id EditorActionToolbar --direction RIGHT` reaches a key.
- **Typing is keys.** `input-text` types key by key at a thumb's pace through the preset's keyboard.
- **Writing is strokes.** `draw-strokes-on` draws a pad's guide or template as minimum-jerk touch strokes with tremor.
- **Everything is logged.** A recording writes `<video>.inputs.jsonl`, and `check-taps` proves every tap shows as a dot.
  A gate that claims "the agent used only input" shows that log.
- **The device is the device.** Metrics, user agent and client hints, safe-area insets, phone fonts, the status bar and
  home indicator (`--frame bezel` for the hardware), a live network with no CORS, and `__deviceLab` (recorded alerts,
  vibrate, share) come with the preset on a lab export (`EXPO_PUBLIC_DEVICE_LAB=1`). The reads (`get-element`,
  `get-device-state`, `hierarchy`, `get-logs`) never change the page.

## Start here

1. Build the export with `wf test prepare --web` and serve `frontend/mla-tamagui/dist` under `/mla-tamagui` on loopback (the Setup of `docs/runbooks/device-lab-codemirror-typing.md` has the commands); no Metro runs on the VM.
2. Start a device (the daemon starts with it): `wf browser start-device --device primary`.
3. Open a page: `wf browser open-link http://127.0.0.1:8800/mla-tamagui/leetcode/two-sum`.
4. Wait for it: `wf browser assert-visible --id ProblemEditorRunLocalButton --timeout 30000`.
5. Look: `wf browser hierarchy`, then `wf browser take-screenshot /tmp/two-sum.png`.

## Interaction rules

- Find the element in `wf browser hierarchy`, then act on it by id (`--id`), falling back to text.
- **Always drive text entry through `wf browser input-text` after a tap on the field.** It types key by key at the caret,
  the same path a user's input takes, so what a CM6-class editor holds is what was typed. Nothing in the CLI sets a
  field's value directly.
- A wait is a condition, not a sleep: `assert-visible`, or `extended-wait-until --condition` over a read
  (`device:get_device_state('keyboard').visible`, `#device:get_element({id='CodeEditorInput'}).text > 10`).

## Injecting audio on demand

`wf browser set-microphone-input <clip>` sets the clip the app's microphone hears at its *next* recording, with no
relaunch; a new call replaces a clip not yet taken:

```bash
wf browser set-microphone-input backend/remote-chat-agent/tests/fixtures/voice/rename-loop-variable.wav
# ... tap the record button, stop it, read the transcript ...
wf browser extended-wait-until --condition "device:get_device_state('microphone').consumed" --timeout 5000
```

`wav`, `mp3`, `m4a`, `mp4`, `aac`, `ogg`, `opus`, `webm` and `flac` all work — the clip is decoded by an `<audio>`
element, not by Chromium's WAV-only fake capturer.

How it works, and the two things that follow from it:

- It replaces `navigator.mediaDevices.getUserMedia` (installed as a context
  init script when the session opens, inert while nothing is queued) and hands back an
  `HTMLAudioElement.captureStream()`. The app's real pipeline runs unchanged —
  expo-audio hangs its metering `AnalyserNode` off that stream and passes the
  same stream to `MediaRecorder`, and both work on captured samples, so the
  waveform moves and the upload is a real encode of the clip.
- **The init script lands on navigation.** Set the clip after the page has
  opened at least once; against a page opened before the script was
  installed, it fails loudly with "no frame with the fake
  microphone installed" rather than queueing into nothing.
- A clip is consumed by `MediaRecorder.start()`, never by `getUserMedia`, and
  playback starts at that same moment. This is deliberate: expo-audio's
  `requestRecordingPermissionsAsync()` probes permission by calling
  `getUserMedia` and immediately stopping the tracks, so consuming earlier
  would feed the whole clip to a throwaway probe and leave the real take
  silent — and starting playback at `getUserMedia` would lose everything said
  between `prepareToRecordAsync()` and `record()`.

Chromium only, like the fake-device flags.

## Tailnet port-forward

`wf browser tailnet-forward` opens a plain TCP listener on `127.0.0.1` and tunnels it to any `host:port` through tailscaled's own SOCKS5 proxy (the same mechanism `wf browser tunnel` uses for its fixed list of deployed services, generalized here to any tailnet target). Useful for pointing a browser (or `curl`) at a tailnet-only service via plain `localhost`, since Chromium can't be configured to route only some origins through a SOCKS5 proxy.

```bash
wf browser tailnet-forward --host vm-ops-api-main --port 8080
# -> {"ok":true,...,"result":{"url":"http://127.0.0.1:<port>","localPort":<port>,"reused":false}}
```

- Daemon-global, not tied to any session — the local port stays up for the daemon's lifetime.
- Repeating the same `--host`/`--port` returns the existing forward (`reused: true`) instead of starting a second listener.
- Omit `--local-port` to let the OS assign a free port; pass it to pin a stable one.
- A debugging proxy that logs or rewrites the raw bytes is the daemon's `tailnetForward` RPC with `middlewarePath` (a JS
  module whose default export is `(chunk, direction, ctx) => chunk`); the CLI verb no longer takes it, as nothing in
  the repo called it.

## Device frame and keyboard

- In default mode, the app renders inside a device-frame iframe; the fake keyboard renders inside it.
- `wf browser get-device-state keyboard` says whether the keyboard is up and where (`bounds`, null while it is down).

## Recovery

- `wf browser stop-device --device <name>` closes one session; `--all` closes every one.
- `wf browser stop` stops the daemon and closes everything.

## Common workflows

Smoke an Expo route:

```bash
wf browser start-device --device primary
wf browser open-link http://127.0.0.1:8800/mla-tamagui/leetcode/two-sum
wf browser assert-visible --id ProblemEditorRunLocalButton --timeout 30000
wf browser hierarchy
wf browser take-screenshot /tmp/two-sum.png
```

Verify keyboard ownership:

```bash
wf browser tap-on --id CodeEditorInput
wf browser get-device-state keyboard
wf browser tap-on --id EditorAction-Done
wf browser extended-wait-until --condition "not device:get_device_state('keyboard').visible" --timeout 2000
```

## Pitfalls

- Do not use stale `multi-browser_*` MCP tool names, nor the old verbs (`tap --ref`, `eval`): they exit 2 (an unknown verb lists the verbs there are; a dropped one names what covers it).
- Stop the daemon with `wf browser stop` when done.
- A selector matches an element whose on-screen part is not empty: an element below the fold or under a clipping parent is
  `null` to `get-element` and never tapped. Scroll it in with `scroll-until-visible`, then act.
- **A raw JS-dispatched click is not a reliable substitute for a real tap on a React-driven page** — it can silently
  no-op (no error, no state change). The CLI has no eval for that reason; tap with `tap-on`.
