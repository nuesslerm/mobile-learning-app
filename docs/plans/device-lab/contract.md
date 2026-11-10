# device-lab: contract

Read first: [`spec.md`](spec.md) (§2 tiers, §3–§4 breakdown, §7 acceptance gates) and [`progress.md`](progress.md). This contract is binding on every unit in [`units.jsonl`](units.jsonl). It pins the seams; how a seam is built is the unit's choice, as long as its gate passes.

## Driver commands (`wf browser …`, `clis/multi-browser`)

Rules for every command:

- **One command, four places.** A command is:
  - a daemon method;
  - a CLI subcommand in `src/entrypoints/cli.ts`;
  - a zod schema in `src/data/commandSchemas.ts`;
  - a method on the SDK container (`src/di/container.ts`).
- **Output.** It prints JSON with `--json`, and a one-line summary otherwise.
- **No false success.** A command whose action had no effect says so, and never reports success. (`scroll` reported "Scrolled 0,300" while nothing moved; that is the bug this rule prevents.)
- **Indicators.** Input commands draw their indicator unless `--no-indicator` is passed: a dot for a tap, a trail for a stroke or swipe, a key tint on the mock keyboard for a key.
- **Input log.** While recording, input commands append to the session's input log.
- **Touch.** On a preset with `hasTouch`, taps, strokes and swipes are touch input, sent through CDP `Input.dispatchTouchEvent`. `--mouse` forces the old mouse path. Each touch is stamped with its planned time, so the app reads the planned hold and cadence however late an event arrives. A phone preset keeps every frame in one process, as a WebView does: in an out-of-process frame Chromium moves a tap's click by the frame's offset.
- **Delivered.** A press must reach the app frame. Until a cross-site app frame is composited, the shell's `<iframe>` element takes its input. A mouse press starts with a move, and a touch with its touch-down, and either repeats after the next paint for up to 5 s (a kept touch is cancelled first), then the command fails (`pointer-route.ts`). A shell element over the point, such as the mock keyboard, takes the press, as it would on the phone.
- **Coordinates.** Results give points in the device screen's CSS pixels, 0,0 at its top-left, and `swipe --from` takes them so. The input log uses the page's pixels, which are the recording's.

| Command | Arguments | Result (JSON) | Row |
|---|---|---|---|
| `open` (changed) | unchanged | unchanged | D1, D4, I6, S1 |
| `permission` (new) | `--name`, `--kind microphone\|camera\|clipboard-read\|clipboard-write\|geolocation\|notifications`, `--state granted\|denied\|prompt` | `{ kind, state }` | I5 |
| `camera-inject` (new) | `--name`, `--path <png\|jpg\|webp\|webm\|mp4>` | `{ path, width, height, frames }` | I7 |
| `upload` (new) | `--name`, `--path <file>` (repeatable) or `--clear` | `{ armed: string[] }` | I9 |
| `clipboard` (new) | `--name`, `[--set <text>]` | `{ text }` | S2 |
| `opened-urls` (new) | `--name`, `[--clear]` | `{ urls: { url, at, via }[] }`, `via` one of `linking`, `window.open`, `anchor` | S3 |
| `tap` (changed) | existing, plus `[--mouse]` | `{ tapped, x, y, holdMs, pointer: 'touch' \| 'mouse' }` | I13 |
| `stroke` (changed) | existing, plus `[--human]`, `[--mouse]` | `{ stroked, points, samples, ms, pointer, curve }` | I13 |
| `swipe` (new) | `--name`, `--ref <sel>` or `--from <x,y>`, `--dx <px>`, `--dy <px>`, `[--fling]` | `{ from, to, ms, scrolled: { dx, dy }, pointer }`, with `scrolled` measured on the scroll container under the start point | I13 |
| `trace` (new) | `--name`, `--ref <pad>`, `--source guide\|template`, `[--human]` | `{ strokes, samples }`; `guide` is one stroke | I13 |
| `type` (changed) | existing, plus `--at-caret`, `[--human]` | `{ typed, mode: 'caret', keys, interKeyP50Ms }` with `--at-caret`, which fails unless focus is already in `--ref`; unchanged without | I14 |
| `press` (new) | `--name`, `<key>` (`Enter`, `Backspace`, `Tab`, `Escape`, `ArrowLeft`, `ArrowRight`, `ArrowUp`, `ArrowDown`), `[--repeat <n>]` | `{ key, repeat }` | I14 |
| `close` (changed) | unchanged | `{ closed, video?, inputLog? }` | I13 |
| `video-check` (new) | `<video>`, `[--inputs <jsonl>]` (default: the `.inputs.jsonl` beside the video, same stem) | `{ taps, dotsFound, missing: number[], pass }`, `missing` the `t` of each tap with no dot; exits 1 when `pass` is false | I13 |

Behaviour each command must keep:

- **`open`**
  - Applies the preset's device metrics: `deviceScaleFactor`, `isMobile`, `hasTouch`, and a user agent with `Sec-CH-UA-*` client hints from `DeviceSpec` (D1).
  - Loads Roboto for Android presets and Inter for iOS (D4).
  - Launches with `--autoplay-policy=no-user-gesture-required` (I6).
  - On a phone preset, sends every cross-origin fetch and XHR through Node, without the headers only a browser adds, and hands back the real response with CORS allowing it (S1). Responses are never mocked.
  - Sizes the recording to the shell page.
- **`permission`** grants through `context.grantPermissions`; `denied` and `prompt` go through CDP `Browser.setPermission`, on the app frame's origin.
- **`camera-inject`** replaces the video track that `getUserMedia({ video })` returns, for the current stream and every later one.
- **`upload`** arms the files. The next file chooser a tap opens receives them, and the use is written to the input log.
- **`trace`**
  - `--source guide` replays the path the pad publishes in `data-active-guide`, in pad pixels.
  - `--source template` screenshots the pad before any ink, thins the faint glyph to one-pixel centre lines and splits them at junctions and ends. Strokes are ordered by letter, left to right, then top to bottom within a letter.
  - Neither source may read course data, glyph tables or app source.
- **`--human`**
  - On `stroke` and `trace`: a minimum-jerk speed profile at hand speed, samples at 60 Hz in real time, and 1–1.5 px tremor.
  - On `type`: an inter-key p50 of 150–300 ms with jitter, and longer pauses after punctuation and newlines.
- **`tap`** with a finger lands at a jittered point in the middle 60 % of the part of the target on the screen and holds 70–140 ms; `--mouse` clicks that part's centre. A target with no part on the screen is refused ("off the screen -- swipe it into view first"): a finger cannot touch past the glass. `swipe --ref` starts on the centre of the part on the screen.
- **`type --at-caret`**
  - Types into the focused element at its caret, one key per event. It never selects or clears.
  - iOS presets send `keydown` with the real key, then `Input.insertText`.
  - Android presets send `keydown` with keyCode 229 and key `Unidentified`, then `Input.insertText`. Enter and Backspace are a 229 key down carrying the editing command, so they arrive as `insertParagraph` and `deleteContentBackward`.
  - The desktop preset presses hardware keys.
- **Input log** (one JSON object per line, in `<video stem>.inputs.jsonl` beside the recording): `{ t, kind, x?, y?, ref?, key?, chars?, samples?, files? }`.
  - `t` is milliseconds since the recording started.
  - `kind` is one of `tap`, `stroke`, `swipe`, `trace`, `type`, `press`, `mic`, `upload`. A `trace` entry is one stroke; an `upload` entry carries `files`.
  - A `type` entry carries `chars: 1`. A multi-character insert is a contract violation.
- **`video-check`** passes when, for every logged tap, a frame within 300 ms after `t` shows the tap-indicator colour within 20 px of `(x, y)`.
- **Existing and unchanged:** `keyboard` (I1; focus already raises the mock keyboard) and `audio-inject` (I3).

## Lab module (T3)

The in-app mocks for APIs that neither the browser nor Chromium emulation can supply.

- **Location:** `frontend/mla-tamagui/lib/device-lab/index.ts`, imported as `@/lib/device-lab`. It imports nothing from `features/`.
- **Gate.** The only way in is this block at the top level of `frontend/mla-tamagui/app/_layout.tsx`:

  ```ts
  if (process.env.EXPO_PUBLIC_DEVICE_LAB === '1') {
    require('@/lib/device-lab').installDeviceLab();
  }
  ```

  - Nothing imports the module statically.
  - babel-preset-expo inlines `EXPO_PUBLIC_*` in production builds, so a flagless export drops the branch and the module.
- **Flag.**
  - `frontend/mla-tamagui/package.json`'s `export:web` sets `EXPO_PUBLIC_DEVICE_LAB="${EXPO_PUBLIC_DEVICE_LAB-1}"`: on unless set, and an explicitly empty value turns it off.
  - EAS Update and native builds never set it.
  - `frontend/mla-tamagui/metro.config.js` keys `cacheVersion` on the flag. Metro's transform cache does not key on inlined `EXPO_PUBLIC_*` values, so without it a flagged export that follows a flagless one reuses the folded branch and ships without the lab (seen 2026-10-05).
- **Sentinel.** The module contains the literal `__device_lab_v1__`. A flagged export carries it and a flagless export does not (goal criteria `p2.prod-clean` and `p2.lab-sentinel`).
- **API:**

  ```ts
  export const DEVICE_LAB_SENTINEL = '__device_lab_v1__';
  export type LabCall = { api: string; args: unknown[]; at: number };
  export type LabAlert = { title: string; message?: string; buttons: string[]; answered?: number };
  export interface DeviceLab {
    version: typeof DEVICE_LAB_SENTINEL;
    /** Recorded calls to APIs that are a no-op on web: vibrate (I10), speech (I12), share (S6). */
    calls: LabCall[];
    /** Every alert shown (S5), with the index of the button that answered it. */
    alerts: LabAlert[];
  }
  /** Idempotent. Sets and returns globalThis.__deviceLab. */
  export function installDeviceLab(): DeviceLab;
  ```

- **Alert (S5).**
  - `installDeviceLab` replaces react-native-web's no-op `Alert.alert` with a native-styled dialog rendered in a portal.
  - testIDs: `DeviceLabAlert`, `DeviceLabAlert-Title`, `DeviceLabAlert-Message`, `DeviceLabAlert-Button-<index>`.
  - A tap on a button calls that button's `onPress` and records `answered`.
- **Reading it.** The driver reads `window.__deviceLab` through `wf browser eval`, which only reads.

## Static guards (T4)

What the browser cannot show is guarded by lint rules and tests.

**Lint rules** live in `libraries/eslint-plugin-mobile-learn` under the `mobile-learn/` prefix. Each has tests in that package's `tests/` and is enabled in `frontend/mla-tamagui`'s ESLint config.

| Rule | Row | Fails on |
|---|---|---|
| `no-hermes-unsupported-api` (existing, grown) | J1 | `structuredClone`, `Intl.Segmenter`, streaming reads of `response.body`, `TextDecoderStream`, `crypto.subtle`, on top of today's list. Each entry cites its source; an entry not yet verified against the shipped Hermes says `spike` in its message. |
| `no-web-globals-in-shared` (new) | J2 | `window`, `document`, `localStorage`, `sessionStorage`, `visualViewport`, `matchMedia` or `navigator` outside `*.web.ts(x)` files, unless guarded by `typeof X !== 'undefined'` or `Platform.OS === 'web'`. |
| `no-dynamic-import` (new) | J4 | `import()` in app code (`frontend/mla-tamagui`, `features`, `libraries`), unless the line above it reads `// device-lab: native-verified <runbook>`. |
| `no-alert-outside-lab` (new) | S5 | `Alert` imported from `react-native` outside `frontend/mla-tamagui/lib/device-lab/`. |
| `keyboard-avoiding-behavior` (new) | U6 | A `KeyboardAvoidingView` without a `behavior` chosen by `Platform.OS`, and a `RefreshControl` in a file without a matching native-branch test. |

**Tests** (each gives a native-only path a test the browser cannot provide):

| Path | Row | Asserts |
|---|---|---|
| `frontend/mla-tamagui/features/leetcode/data/__tests__/voiceEditTransport.native.test.ts` | J3 | The native branch appends `{ uri, name, type }` to the `FormData`, not a `Blob`. |
| `docs/plans/device-lab/forks.md` with `frontend/mla-tamagui/tests/unit/platformForks.test.ts` | J3 | Every `Platform.OS`/`Platform.select` use and every `.web`/`.native` file is in the register as `inherent`, `removable` or `instrumentation`, and each `inherent` fork names a native-branch test that exists. |
| `backend/remote-chat-agent/tests/integration/transcribeM4a.test.ts` | I4 | An m4a/AAC fixture posted to `/transcribe` comes back as text. |
| `frontend/mla-tamagui/features/leetcode/code-editor/core/__tests__/editorMessages.contract.test.ts` | U1 | The iframe shim and `react-native-webview` exchange the same `EditorMessages` schema. |
| `frontend/mla-tamagui/tests/unit/nativeConfig.test.ts` | J5 | `runtimeVersion` matches the installed `expo` SDK major, and every dependency that ships a config plugin (`expo-audio` first) is in `plugins`. |
| `frontend/mla-tamagui/lib/__tests__/devRestart.native.test.ts` | S13 | The native branch calls `Updates.reloadAsync`, after `DevSettings.reload` in dev. |
| `frontend/mla-tamagui/tests/unit/hostComponents.test.ts` | U9 | Every native module in `package.json` is on the New Architecture list in `docs/plans/device-lab/host-components.json`, sourced from React Native Directory. |

## Flow-test reuse

- **Safe areas.** The `react-native-safe-area-context` stub in `frontend/libraries/flow-test-harness` seeds `initialWindowMetrics` with the iPhone 15 Pro insets (top 59, bottom 34) by default; a test can override them.
- **Permissions.** `createFakeAudioRecorder({ permission })` accepts `'granted'`, `'denied'` or `'undetermined'`.

## Acceptance gates

The gates are the runbooks named in spec §7: `docs/runbooks/device-lab-voice-edit.md`, `device-lab-tamil-writing.md` and `device-lab-codemirror-typing.md`.

- A gate passes when its result record in `docs/runbooks/.results/` says `pass` and its sha is on this history.
- The runbook must be unchanged since that run.
- `progress.md` ticks the gate with the published recording.
