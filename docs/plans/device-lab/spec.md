# device-lab: a browser render you can trust for native

Status: phase 0 (this spec) written 2026-10-04; acceptance gates (§7) added the same day. Driven to completion by the goal [`docs/goals/2026-10-04-device-lab.md`](../../goals/2026-10-04-device-lab.md).
Owner: Markus. Branch: `claude/device-lab`, rebased onto `dev`, without the greenfield commits. Tracking PR: [#306](https://github.com/nuesslerm/expo-hello-world-app-2/pull/306).

## 1. Intent

Clarified in the 2026-10-04 Q&A; this replaces the first draft, which read as remote control of devices.

- **One stand-in for the phone.** The web export, rendered by headless Chromium at a real phone's size, stands in for iOS and Android. If a screen works there, it should work in the native app.
- **Native-like mocks where web differs.** Where react-native-web (RNW) or the browser behaves differently from native, or has no equivalent, the browser gets a mock that behaves like native.
- **Headless-agent drivable.** An agent drives every capability with no human and no UI. That covers microphone input, camera and video, file uploads, permissions, the keyboard, and so on. There are no knobs, because nothing is changed by hand.
- **Proven by journeys a person would make.** Three journeys run the way a person runs them, driven by an agent and recorded with every tap marked: a voice edit, Tamil letters and words drawn on the canvas, and code typed into the editor (§7).
- **Browser only.** No Expo Go control, no relay, no native overlay. Screenshots come from the browser.
- **The network is never mocked.** The export talks to the live backend.
- **Normal web build plus native mocks.** The app keeps its web code paths; its own platform forks are parity gaps to list and shrink (§5).
- **Surfaces:**
  - the web export in a device-sized Chromium;
  - flow tests (jsdom);
  - the agent CLI/SDK.

Dropped from the first draft:

| Dropped | Why |
|---|---|
| Expo Go controller, WebSocket relay, dev-menu overlay | Browser only. The relay research is kept in §9 in case native control returns. |
| Workbench with knobs and a component catalog | Nothing changes interactively; agents drive state. |
| Network offline/slow emulation | The backend is live. Network work is about making native-like requests succeed (§4, S1). |
| Build-vs-adopt (Storybook RN, React Cosmos, expo-dev-client) | No catalog, so the question is moot. The numbers are kept in §9. |

## 2. Four tiers: how a native API reaches the browser

Use the lowest tier that is faithful.

| Tier | Use when | How | Ships in production? |
|---|---|---|---|
| **T1 web-native** | The web implementation behaves like native | Nothing; render and drive as is | — |
| **T2 Chromium emulation** | The browser has the API, but its state must be set: permission, metrics, media input, file chooser, sensors | Playwright and CDP, from the driver (details below) | No: it lives in the browser session |
| **T3 in-app lab mock** | No web equivalent, or RNW diverges from native (`Alert.alert` is an empty function on web) | A lab-only module behind `if (__DEV__ \|\| process.env.EXPO_PUBLIC_DEVICE_LAB === '1') require(…)` | No: folded away (evidence below) |
| **T4 static guard** | A browser can never reproduce the failure: Hermes API gaps, native view configs, Metro native bundling, native config | A lint rule or config check, or a written residual risk with its guard | — |

T2's Playwright and CDP tools:

- context options;
- `grantPermissions`;
- `Browser.setPermission`;
- `Emulation.*`;
- init scripts;
- Chromium launch flags.

Evidence for the tiers:

- **T3 folds out of production.** babel-preset-expo 57.0.10 replaces `process.env.EXPO_PUBLIC_*` with the literal value in production builds; an unset variable becomes `undefined` (`plugins/inline-env-vars.js:50-51`). Metro then constant-folds the dead branch before it collects dependencies, so the lab module never enters the graph.
- **No deployed web build to protect.** mla-tamagui has no deployed web build. Web exports exist only for tests; production is the EAS Update. So `export:web` turns the lab flag on, and the guard is "a flagless export contains no `device-lab` sentinel".
- **T2 spikes.** Both were run in this sandbox's Chromium 141:
  - `Emulation.setSafeAreaInsetsOverride({insets:{top:59,bottom:34,left:0,right:0}})` makes `env(safe-area-inset-top)` compute to `59px`, with or without `viewport-fit=cover`.
  - `Browser.setPermission({permission:{name:'microphone'}, setting:'denied'})` makes `navigator.permissions.query({name:'microphone'})` report `denied`.

The agent drives everything through `wf browser …`, the multi-browser daemon. Today's commands:

- `open --device --orientation --color-scheme --audio-file`
- `navigate`, `tap`, `drag`, `stroke`, `type`, `scroll`
- `eval`, `audio-inject`, `network-proxy`, `set-device`
- `snapshot`, `screenshot`, `check-clipping`, `cookies`, `goldens`

The commands this spec adds are listed per row.

## 3. Summary

**Priority:**
- **P1:** used today with a real parity risk, or explicitly asked for (audio, camera/video, file uploads).
- **P2:** used today with a smaller risk, or a common trap.
- **P3:** not used today.

**Status:**
- **`done`:** meets §1 today, agent-drivable.
- **`residual`:** accepted risk with a written guard.
- **`todo`.**

The goal's final check fails while any P1 or P2 row is `todo`.

<!-- breakdown:start -->
| ID | API | Used today | Web today | Tier | P | Status |
|---|---|---|---|---|---|---|
| D1 | Viewport, DPR, touch, user agent | yes | partial | T2 | P1 | done |
| D2 | Safe-area insets | yes | 0 in the frame | T2 | P1 | done |
| D3 | Colour scheme | yes | yes | T2 | P1 | done |
| D4 | Fonts and text metrics | yes | differs | T2 | P1 | done |
| D5 | Status bar and home indicator chrome | yes | none | T2 | P2 | done |
| D6 | Orientation | no (portrait lock) | viewport swap | T2 | P3 | todo |
| D7 | Text scale (Dynamic Type) | implicit | none | T3 | P3 | todo |
| D8 | Reduced motion, locale, timezone | partial | yes | T2 | P3 | todo |
| I1 | Soft keyboard frame | yes | none | T3 | P1 | done |
| I2 | TextInput traits | yes | partial | T4 | P3 | residual |
| I3 | Microphone recording | yes | yes | T2 | P1 | done |
| I4 | Recording container/codec | yes | differs | T4 | P1 | done |
| I5 | Permission states | yes (mic) | grant only | T2 | P1 | done |
| I6 | Audio playback | yes | yes | T2 | P2 | done |
| I7 | Camera / video capture | no | yes | T2 | P1 | done |
| I8 | Video playback | no | codec-limited | T1 | P3 | residual |
| I9 | File upload (image/document picker) | no | yes | T2 | P1 | done |
| I10 | Haptics / vibration | no | no-op | T2 | P3 | todo |
| I11 | Sensors and device motion | no | partial | T2 | P3 | todo |
| I12 | Speech (TTS) | no | yes | T2 | P3 | todo |
| I13 | Touch input (taps and strokes) | yes | mouse events | T2 | P1 | done |
| I14 | Soft-keyboard text entry | yes | bulk insert, key events | T2 | P1 | done |
| S1 | Live network (fetch, WebSocket) | yes | CORS applies | T2 | P1 | done |
| S2 | Clipboard | yes | permission-gated | T2 | P1 | done |
| S3 | Opening external URLs | yes | new tab | T2 | P2 | done |
| S4 | Deep links in / initial route | yes | yes | T1 | P1 | done |
| S5 | `Alert.alert` dialogs | no | no-op | T3 | P2 | done |
| S6 | Share sheet | no | Linux: absent | T3 | P3 | todo |
| S7 | Android back button | no | console error | T3 | P3 | todo |
| S8 | App state (foreground/background) | no | visibilitychange | T2 | P3 | todo |
| S9 | AsyncStorage | yes | localStorage | T1 | P2 | done |
| S10 | Secure store | no | none | T3 | P3 | todo |
| S11 | Notifications | no | none | T3 | P3 | todo |
| S12 | Location | no | yes | T2 | P3 | todo |
| S13 | OTA reload (expo-updates) | yes | none | T4 | P2 | done |
| S14 | Constants, device info, launch args | yes | partial | T1 | P3 | done |
| S15 | File system | no | none | T3 | P3 | todo |
| U1 | WebView | yes | none (own iframe shim) | T4 | P1 | done |
| U2 | Gesture handler | yes | pointer events | T4 | P2 | done |
| U3 | Reanimated | yes | JS-driven | T4 | P2 | done |
| U4 | Skia | yes | CanvasKit | T4 | P2 | done |
| U5 | Bottom sheet | yes | partial | T4 | P2 | done |
| U6 | Scroll physics, RefreshControl, KeyboardAvoidingView | yes (ScrollView) | differs | T4 | P2 | done |
| U7 | Native stack transitions | yes | JS stack | T4 | P3 | residual |
| U8 | JS-only UI libs (svg, markdown, toasts, wheel picker) | yes | yes | T1 | P3 | done |
| U9 | Native view configs (New Architecture) | yes | none | T4 | P1 | done |
| J1 | Hermes API gaps | yes | V8 has them | T4 | P1 | done |
| J2 | DOM / web globals in shared code | yes | present | T4 | P2 | done |
| J3 | Platform forks (`Platform.OS`, `.web`/`.native` files) | yes | web branch only | T4 | P1 | done |
| J4 | Metro native bundling (no code splitting) | yes | splits | T4 | P2 | done |
| J5 | Native config (runtimeVersion, usage strings) | yes | n/a | T4 | P2 | done |
<!-- breakdown:end -->

## 4. Breakdown

Each entry uses the same fields:

- **Used** — where the app uses it.
- **Native** — what iOS and Android do.
- **Web** — what RNW, Expo's web implementation or the browser does.
- **Gap** — where they differ.
- **Mock** — how the browser is made to behave like native (the tier).
- **Drive** — how an agent controls it.
- **Flow** — the jsdom flow-test equivalent.

**Sources**

- Expo platform support: the `platforms` frontmatter of each [Expo SDK 57 page](https://docs.expo.dev/versions/v57.0.0/). The full catalogue is in §9.
- RNW behaviour: its 0.21 source, `react-native-web/dist/exports/<API>/index.js` (on [GitHub](https://github.com/necolas/react-native-web/tree/master/packages/react-native-web/src/exports)).
- Playwright: [BrowserContext](https://playwright.dev/docs/api/class-browsercontext), [emulation](https://playwright.dev/docs/emulation), [FileChooser](https://playwright.dev/docs/api/class-filechooser).
- CDP: the [Emulation](https://chromedevtools.github.io/devtools-protocol/tot/Emulation/) and [Browser](https://chromedevtools.github.io/devtools-protocol/tot/Browser/) domains.

### Device and display

**D1 Viewport, DPR, touch, user agent**
- Used: everywhere; `useWindowDimensions` in `app/(app)/_layout.tsx`, `app-shell/DebugFab.tsx`, `features/leetcode/ui/ReorderableList.tsx`, and `rn-agentation`.
- Native: logical points × scale; touch only; the platform user agent.
- Web: the CSS viewport is the iframe or page size, DPR is the context's `deviceScaleFactor`, and touch comes from `hasTouch`.
- Gap: the device frame sizes the iframe from `DEVICE_REGISTRY` (`clis/multi-browser/harness-shell/contracts/src/index.ts`, 11 devices). Context DPR, `isMobile`, `hasTouch` and the user agent are not set per preset; `emulateMobile` sets only the user agent.
- Mock (T2): create the context from the registry entry, with viewport, `deviceScaleFactor`, `isMobile`, `hasTouch`, a mobile user agent, and UA client hints via `Emulation.setUserAgentOverride`.
- Drive: `wf browser open --device <preset>` applies the full metrics in the device frame. Status `done`.
  - The context gets `deviceScaleFactor`, `isMobile` and `hasTouch`, and the user agent of the device's WebView (`DeviceSpec.identity`).
  - CDP sets `navigator.platform`, client hints and five touch points on the page and on the app frame's own target.
  - Plain pages, the goldens among them, keep a desktop context.
- Findings while building it:
  - The context's emulated pixel ratio never reaches a cross-site app frame (it reported 1 inside a 3× page), and a frame target refuses a metrics override of its own. The browser therefore also launches with `--force-device-scale-factor`.
  - A phone lays out a page with no viewport meta at 980 px and zooms out to fit it. Test pages now carry the app export's exact viewport meta.
  - `navigator.vendor` stays Chromium's, so WebKit-only branches keyed on it (CodeMirror's iOS path) still take the Blink path. Accepted residual.
- Evidence:
  - e2e `deviceLab` D1 4/4: iPhone same-site and cross-site frames, a Pixel cross-site frame with Android WebView client hints, and desktop unchanged.
  - The existing e2e suites pass 18/18.
  - multi-browser unit tests pass, including three for `resolveDeviceIdentity`.
- Flow: features stub `Dimensions` at 320×640. jsdom has no device parity, by design.

**D2 Safe-area insets**
- Used: `react-native-safe-area-context` in:
  - `app/_layout.tsx` (provider);
  - `app-shell/AppDrawerContent.tsx`;
  - `features/leetcode/ui/ProblemListScreen.tsx`;
  - `features/tamil-writing/ui/TamilCourseScreen.tsx`;
  - `frontend/libraries/tamagui-design-system/composites/screen-header.tsx`.
- Native: real insets per device and orientation (notch, Dynamic Island, home indicator).
- Web: `NativeSafeAreaProvider.web.js` measures `env(safe-area-inset-*)`, which is 0 in desktop Chromium.
- Gap: every device frame renders the app with zero insets, so notch overlap is invisible. `DEVICE_REGISTRY[].safeArea` exists but only its test reads it.
- Mock (T2): `Emulation.setSafeAreaInsetsOverride` with the registry's oriented insets (`resolveOrientedSafeArea`), sent on the page and on the app frame's own CDP target. A cross-site app iframe is a separate target and does not inherit the page's override (spike: 0px until its own session sets it).
- Drive: automatic on `open`, `navigate` and `set-device` (`clis/multi-browser/src/data/browser/safe-area.ts`).
- Evidence: `clis/multi-browser/tests/e2e/device-frame-safe-area.e2e.test.ts` passes 4/4 (same-site frame, cross-site frame, iPhone SE, landscape rotation). The screenshots show the app header below the Dynamic Island.
- Flow: the `react-native-safe-area-context` harness stub returns 0. Make it accept seeded insets.

**D3 Colour scheme**
- Used: `useColorScheme` in `app/_layout.tsx`, `features/settings-impl/src/ui/AppearanceSettingsScreen.tsx`, `features/leetcode/ui/MermaidDiagram.tsx`.
- Native: OS appearance.
- Web: RNW `Appearance` reads `matchMedia('(prefers-color-scheme: dark)')`.
- Gap: none once emulated.
- Mock (T2): `page.emulateMedia({ colorScheme })`, which already exists.
- Drive: `wf browser open --color-scheme`. Status `done`.
- Flow: `FLOW_THEME` light/dark goldens.

**D4 Fonts and text metrics**
- Used: the system font stack (`-apple-system, BlinkMacSystemFont, …` via RNW/Tamagui) plus `assets/fonts/NotoSansTamil-Regular.ttf`.
- Native: SF Pro (iOS) or Roboto (Android).
- Web: Linux Chromium has neither, so it substitutes fallback fonts with different metrics.
- Gap: wrapping, truncation and line height differ invisibly. A label that fits in the browser can wrap on a phone.
- Mock (T2): launch the lab browser with metric-compatible fonts installed and mapped onto the system stack:
  - Android: Roboto (Apache-2.0), which is exact.
  - iOS: Inter (OFL) as the closest open proxy. SF Pro may not be used off Apple platforms.
- Drive: automatic per device platform. Status `done`.
  - On the first open, `src/data/browser/fonts.ts` fetches the platform's TrueType faces into `~/.cache/wf-device-lab/fonts/<platform>/` (override the location with `WF_DEVICE_LAB_FONTS_DIR`): Inter 400–700 for iOS, Roboto 400/500/700 for Android.
  - The browser launches with a `FONTCONFIG_FILE` that answers every system family with that font: `sans-serif`, `system-ui`, `-apple-system`, `BlinkMacSystemFont`, Roboto, Helvetica, Arial, the SF names, DejaVu and Liberation.
  - `wf browser open` reports the stand-in as `systemFont`.
  - Offline, the browser still opens, on Linux's own fonts.
- Evidence: e2e `deviceLab` D4 2/2. In the app frame, text set in `-apple-system`, `system-ui` and `sans-serif` measures exactly as Inter on the iPhone and as Roboto on the Pixel, and differs from `monospace`. Four unit tests cover the parsing, the config and the cache.
- Flow: n/a (jsdom has no layout).

**D5 Status bar and home indicator chrome**
- Used: implicitly; the app draws under the system bars (`edgeToEdgeEnabled`).
- Native: the OS draws the bars inside the safe areas.
- Web: RNW `StatusBar` renders `null`.
- Gap: screenshots lack the bars, so content under the status bar looks fine when it would collide with the clock.
- Mock (T2, harness side): the render container (§6) draws a status bar (time, signal, Wi-Fi, battery, and the Dynamic Island where the device has one) and a home indicator inside the emulated insets. Glyphs follow the colour scheme.

**D6 Orientation**
- Used: no; the app is locked to `orientation: 'portrait'`.
- Web: rotation is a viewport swap; `resolveOrientedViewport` exists.
- Mock (T2): `--orientation landscape` exists. Add `Emulation.setDeviceMetricsOverride` `screenOrientation` when needed.

**D7 Text scale**
- Used: implicitly; RN `Text` scales with the OS font size (`allowFontScaling` defaults to true).
- Web: no OS font scale.
- Mock (T3): a lab-only root font-scale multiplier. P3 until accessibility sizes are in scope.

**D8 Reduced motion, locale, timezone**
- Used: one hard-coded `Intl.NumberFormat('en-US')` in `DispatchRunScreen.tsx`; dates use the device timezone.
- Mock (T2): `emulateMedia({ reducedMotion })`, plus context `locale` and `timezoneId`.

### Input and media

**I1 Soft keyboard frame**
- Used: `frontend/libraries/tamagui-design-system/lib/keyboard/keyboardSource.ts` and `.web.ts` → `useKeyboardStore`, which feeds:
  - `KeyboardToolbar`;
  - `PlanTab.tsx:91`;
  - `ActionToolbar.tsx` (`Keyboard.dismiss`);
  - `ui/sheet.tsx`.
- Native: a real keyboard. iOS overlays content; Android resizes it.
- Web: RNW `Keyboard.addListener` is a no-op and `isVisible()` returns false, so keyboard events never fire on web. The DS web source reads `visualViewport` instead, plus the harness globals `__MCP_KEYBOARD_VISIBLE__`, `__MCP_KEYBOARD_MODE__` and `__MCP_KEYBOARD_HEIGHT__`.
- Gap: covered by the harness `MockKeyboard`. Overlay vs resize comes from `resolveKeyboardStrategy`, and the height is `MOCK_KEYBOARD_HEIGHT`.
- Mock (T3): exists.
- Removed: there is no verb to raise or lower the keyboard. It follows input focus, as on a device. Status `done`.

**I2 TextInput traits**
- Used: `TextInput` in 3 files.
- Native: keyboard type, return key, autocorrect bar, secure entry.
- Web: RNW maps these to HTML attributes (`inputMode`, `enterKeyHint`, `autocorrect`).
- Gap: native suggestion bars and keyboard layouts don't exist on web.
- Residual: the mock keyboard draws one layout. Guard: none. P3.

**I3 Microphone recording**
- Used: `frontend/mla-tamagui/features/leetcode/data/ExpoAudioRecorderAdapter.tsx`, behind `AudioRecordingPort`.
- Native: AVAudioRecorder or MediaRecorder.
- Web: `getUserMedia` + `MediaRecorder`; `expo-audio` lists web.
- Mock (T2): exists. A `getUserMedia` shim (`clis/multi-browser/src/data/browser/fake-mic.ts`) plays pushed clips through a WebAudio destination. A launch-time WAV is also supported via the `--use-file-for-fake-audio-capture` flags in `launch.ts`.
- Drive: `wf browser audio-inject --path <wav>` and `open --audio-file`. Status `done`.
- Flow: `createFakeAudioRecorder` (harness double).

**I4 Recording container and codec**
- Used: same adapter, lines 120-124. Web records `audio/webm` (Opus); native records `audio/m4a` (iOS) or `audio/mp4` (Android), both AAC.
- Gap: the backend transcribes a different container in the browser than on a phone.
- Guard (T4): a backend contract test that feeds an m4a fixture through the same `/transcribe` path. The browser can't produce AAC.
- Status `done` (T4). `backend/remote-chat-agent/tests/unit/voiceTranscribeContainers.test.ts` posts WebM/Opus, m4a and mp4-labelled AAC fixtures through the real `/api/voice-transcribe` route to an engine that records the bytes, and (with `STT_CONTRACT_URL`) through the live faster-whisper sidecar, which heard the same words from the AAC and Opus files as from the WAV.

**I5 Permission states**
- Used: the microphone, via `requestRecordingPermissionsAsync`.
- Native: granted, denied or undetermined, plus "don't ask again".
- Web: the Permissions API.
- Gap: the daemon only grants (`grantPermissions(['microphone'])`), so a denial path never renders.
- Mock (T2): grant with `context.grantPermissions`; deny with CDP `Browser.setPermission(setting:'denied')` (spike ✓); reset to prompt.
- Drive: `wf browser permission --name <b> --kind <kind> --state granted|denied|prompt`. Status `done`.
  - It sets the permission through `Browser.setPermission`, scoped to the context.
  - A fresh device frame starts with the microphone and camera at `prompt`, as a freshly installed app does, and a prompt is answered Allow.
  - The fake microphone refuses with `NotAllowedError` when the microphone is denied, even with a clip queued. `audio-inject` no longer re-grants a refusal.
- Findings while building it:
  - Chromium resets every permission override in a context when the CDP session that set one detaches, Playwright's own grants included. The browser session is therefore kept.
  - CDP's grant (Playwright's `grantPermissions`) denies every permission it does not list. Granting the microphone for `audio-inject` silently revoked the clipboard, and the context's clipboard grant left the microphone denied from the start.
- Evidence: e2e `deviceLab` I5. In a cross-site app frame, the microphone starts at `prompt`; once denied, a queued clip still fails with `NotAllowedError`; once granted, it is a stream; reset, it is `prompt` again.
- Flow: `createFakeAudioRecorder({ permission: false })`.

**I6 Audio playback**
- Used: `features/leetcode/hooks/useVoicePlayback.ts` (`useAudioPlayer`).
- Native: plays through the speaker; iOS honours `playsInSilentMode`.
- Web: `HTMLAudioElement`. Headless Chromium enforces the autoplay policy.
- Mock (T2): `--autoplay-policy=no-user-gesture-required` in the lab browser. It already ships in every launch (`launch.ts` `chromiumBaseArgs`).
- Drive: an `eval` probe for the playing state. The silent switch can't be emulated (residual note). Status `done`: e2e `deviceLab` I6 shows a sound played before any tap reaches `playing`.

**I7 Camera and video capture**
- Used: no. `expo-camera` and `expo-image-picker` both list web.
- Native: camera preview and capture.
- Web: `getUserMedia({ video })`.
- Mock (T2): extend the fake-mic shim to video. Draw a clip into a canvas and use `canvas.captureStream()` per take. The launch-time `--use-file-for-fake-video-capture=<y4m|mjpeg>` covers a fixed feed.
- Drive: `wf browser camera-inject --name <b> --path <png|jpg|webp|webm|mp4>`. Status `done`.
  - `fake-camera.ts` draws the source into one canvas and captures every video stream from it, so a new injection changes streams the app already holds.
  - A refused camera answers `NotAllowedError`. A fresh device also has Chromium's fake devices, so a camera opens even before anything is injected.
  - Evidence: e2e `deviceLab` I7. An open stream shows the injected red picture, then blue after a second injection, at the picture's own 64×48.

**I8 Video playback**
- Used: no. `expo-video` lists web.
- Gap: Playwright's Chromium ships without H.264/AAC, and no Google Chrome build exists for linux-arm64 (the VM).
- Residual: use VP9/Opus fixtures, and record the codec gap here.

**I9 File upload (image and document picker)**
- Used: no. `expo-image-picker` and `expo-document-picker` list web.
- Native: system picker → `file://` URI.
- Web: a hidden `<input type="file">` → `File`/blob URI.
- Gap: upload code must handle both shapes (see J3, the `voiceEditTransport.ts` fork).
- Mock (T2): Playwright `filechooser` event plus `setFiles`, armed before the tap.
- Drive: `wf browser upload --name <b> --path <file> [--path …] | --clear`. Status `done`.
  - The next file chooser a tap opens receives the files. A chooser already open receives them at once.
  - File choosers are intercepted from the moment the browser opens: Playwright turns interception on without waiting, so a listener added just before the tap raced it, and the upload test failed when run with the rest of the suite.
  - Evidence: e2e `deviceLab` I9. A tap on an `<input type=file>` in a cross-site app frame hands the app the armed file's name and content.

**I10 Haptics and vibration**
- Used: no. `expo-haptics` lists web; RNW `Vibration` calls `navigator.vibrate`.
- Mock (T2): an init script records `navigator.vibrate` calls on `window.__deviceLab.calls`.
- Drive: `eval`.

**I11 Sensors and device motion**
- Used: no.
- Mock (T2): CDP `Emulation.setSensorOverrideEnabled`/`setSensorOverrideReadings` and `DeviceOrientation.setDeviceOrientationOverride`.

**I12 Speech (TTS)**
- Used: no. `expo-speech` lists web (`speechSynthesis`).
- Headless Chromium has no voices.
- Mock (T2): an init script stub that records utterances.

**I13 Touch input (taps and strokes)**
- Used: every screen. Strokes matter most on the Tamil pads (`TracingCanvasSvg.tsx`, a gesture-handler pan) and drags in the bottom sheets.
- Native: touches with contact time and finger-sized jitter. Gesture handler classifies them as touch pointers.
- Web: the driver clicks and drags with `page.mouse`. Pointers report `pointerType: 'mouse'`, a tap has no hold time, and a stroke moves at a constant step.
- Gap: recognisers that read pointer type or velocity see a mouse, not a finger, and no recording shows where a touch landed unless the driver marks it. The G2 dry run (2026-10-05) showed two more:
  - Mouse strokes on the pad select the text of the card above it. The selection highlight is in every screenshot; a finger selects nothing.
  - Nothing scrolls the app like a person. `wf browser scroll` calls `window.scrollBy` and reports success while the app's ScrollView stays put (the pad's top stayed at 559 px); `wf browser drag` moves nothing either. A finger swipe scrolls.
- Mock (T2): on presets with `hasTouch` (D1), taps and strokes go through CDP `Input.dispatchTouchEvent`.
  - A tap lands at a jittered point inside the target and is held 70–140 ms.
  - With `--human`, a stroke follows a minimum-jerk speed profile, is sampled at 60 Hz in real time and carries 1–1.5 px of tremor.
  - Every tap draws its dot and every stroke its trail (`tap-indicator.ts`), so a `--video` recording shows each one.
  - A swipe is a touch drag with a fling, so it scrolls the app's own ScrollViews.
- Evidence (spike, 2026-10-05): standalone Playwright with `hasTouch`, CDP `Input.dispatchTouchEvent`, minimum-jerk strokes at 60 Hz with 1–1.5 px tremor, on the iPhone 15 Pro viewport with its insets.
  - The pad saw `pointerType: 'touch'`.
  - All seven G2 letters were accepted on the first attempt at 97–100 % ON THE LINE, and no card text was selected.
  - A swipe on the cards scrolled the lesson (the pad's top moved from 441 to 291 px). A swipe that starts on the pad is a stroke, not a scroll.
  - A touch tap on "Got it" moved on to the next letter each time.
  - [Screenshots, video and log](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g2-touch-2026-10-05/index.html).
- Drive: `wf browser tap` and `wf browser stroke [--human]` (existing, switched to touch), and new `wf browser swipe`. New `wf browser trace --ref <pad> --source guide|template` replays either source as human strokes (G2): `guide` is the path the pad draws its active guide from (`data-active-guide`, in pad pixels); `template` is the centre lines of the faint glyph on the pad, thinned from its pixels. `wf browser close` writes the recording's input log (time, kind, position) next to the video, and new `wf browser video-check <video>` confirms a dot is visible at every logged tap. The recording is sized to the shell page: the dry run's video was 532×1072 around a 417×916 page, with grey margins.
- Done:
  - On a phone preset, `tap`, `stroke` and `swipe` are a finger (`touch-input.ts`). `--mouse` keeps the old mouse path; the desktop preset swipes with the wheel.
  - Each touch is stamped with its planned time (CDP `timestamp`), so the app reads a 70–140 ms hold and a 60 Hz cadence even when an event arrives late. A fling's speed is read from these stamps.
  - `trace` (`trace.ts`) samples the guide's SVG path. For `template` it captures the pad, takes the glyph by Otsu's threshold, thins it (Zhang–Suen), and splits the skeleton at junctions and ends. Strokes are written letter by letter, left to right, then top to bottom.
  - `swipe` reports what the scroll container under its start point moved. `close` returns the input log (`<video stem>.inputs.jsonl`). `video-check` reads frames with ffmpeg.
  - A session that records opens at its fitted size and is not resized under the recording.
- Found while building it:
  - A tap into a cross-site app frame clicked 13,24 px up and left of the finger: the app frame's offset. Chromium applied it twice to the tap gesture of an out-of-process frame. Touch events and pointer events were right; the click was not. A probe on full-width targets read the click at (−13, −24) or (−13, −25) from the pointerdown in all 18 taps. With small buttons, touch adjustment snapped that click onto the button above, so S3 and I9 lost taps. A phone's WebView keeps every frame in one process. A phone preset now does too (`--disable-site-isolation-trials`): the same probe read within 1 px.
  - The app measured a tap's hold 60–135 ms longer than planned. The CDP round trip and the route check sat between touch-down and lift, which is why touches carry their planned times.
  - A template capture of a pad under the status bar read the Dynamic Island as its darkest stroke. Captures hide the shell's status bar, home indicator and driver marks.
- Evidence: e2e `deviceLab` I13 touch, I13 trace and I13 video-check, 18/18 in 8 runs in a row. The multi-browser suite is 190/190 with Metro up, including `appFlows`, whose leetcode test taps and types in the real app.
  - Touch: a tap gives `pointerType: 'touch'` and a click within 1 px of the finger, held as planned (±40 ms), inside the target's middle 60 %. A `--human` stroke is unbroken, with no `pointercancel`, at hand pace. A swipe up scrolled the list by exactly what the list reports. The input log holds tap, stroke and swipe, in order.
  - Trace: following the guide puts every ink point within 4 px of the curve, from its start to its end, and the pad publishes the next stroke. Over the template, every ink point is within 4 px of a centre line and the ink covers them; the T is written before the ring.
  - Video-check: three taps show three dots, and the video is exactly the page (418×916). A tap logged where no dot was drawn is reported missing.

**I14 Soft-keyboard text entry**
- Used: the CodeMirror editor (`code-editor/core/editorHtml.ts`, in a WebView on native and a srcdoc iframe on web), the voice review sheet and the plan tab.
- Native: tapping a field raises the keyboard. Keys arrive one at a time at the caret through the platform input method:
  - iOS sends `keydown` with the real key, then `beforeinput`/`input` `insertText`;
  - Android (Gboard) sends `keydown` with `key: 'Unidentified'` (keyCode 229), then `beforeinput` `insertText`, and Backspace as `deleteContentBackward`.
- Web: `wf browser type` selects everything and replaces it with `insertText` (or `keyboard.type` with a fixed delay). The harness already raises the mock keyboard when an editable element takes focus (the `focusin` bridge in `session.ts`). In the dry run, a tap on the editor's last line put the caret at its end (offset 8) and raised the keyboard.
- Gap: CodeMirror handles Android input differently from desktop key events, and a bulk replace never exercises auto-indent, bracket closing or Backspace at the caret.
- Mock (T2):
  - Focus raises the mock keyboard and blur lowers it. A tap on a non-editable area keeps focus (as on Android); only the app blurring the editor (Done, Run) or focusing another element lowers it.
  - Keys go in at the caret, one at a time, through the preset's platform model, using CDP `Input.dispatchKeyEvent` and `Input.insertText`. Each key tints on the mock keyboard.
  - With `--human`, the cadence is a thumb's: inter-key p50 150–300 ms with jitter, and longer pauses after punctuation and newlines.
- Drive: `wf browser type --at-caret [--human]`, which never clears the buffer, and new `wf browser press <key>` (Enter, Backspace, arrows).
- Done (`soft-keyboard.ts`):
  - `type --at-caret` requires focus already in `--ref` (a tap puts the caret there) and fails otherwise. It types one key per event and tints each on the mock keyboard.
  - iOS: `keydown` with the real key, then `Input.insertText`.
  - Android: `keydown` 229 `Unidentified`, then `Input.insertText`. Enter and Backspace are a 229 key down carrying the editing command, so they arrive as `insertParagraph` and `deleteContentBackward`.
  - A desktop preset presses hardware keys.
  - `--human` pauses 150–270 ms between keys, longer after punctuation and new lines. The result reports the median.
  - `press <key> [--repeat n]` goes through the same model. It replaced an unused `press` that sent desktop key presses.
- Found: on the Android preset, the app's CodeMirror takes typed text through an EditContext. It does so only on Android Chrome 126 and later, a real phone's WebView included. So no DOM `insertText` reaches it, and it runs its own Android Enter and Backspace paths from the `insertParagraph` and `deleteContentBackward` it is sent. Desktop key events never exercised either path.
- Evidence: e2e `deviceLab` I14 caret and I14 press, on iPhone 15 Pro and Pixel 8, against the app's own CodeMirror bundle in a srcdoc frame. `deviceLab` is 22/22 in 3 runs in a row, and the multi-browser suite is 197/197 with Metro up.
  - Caret: a tap on the empty last line put the caret at the end (offset 23). Typing `if x:` and Enter gave `def f(x):\n    return x\nif x:\n\t`, with the caret after the tab. The key downs were `i f space x : Enter` with their key codes on iOS, and six 229s on Android, where the editor dispatched its own `Enter`.
  - Press: after `abc` at a thumb's pace (median 150–300 ms), Backspace twice and ArrowLeft left `a` with the caret before it. Android's Backspaces arrived as `deleteContentBackward`.

### System integration

**S1 Live network (fetch, WebSocket)**
- Used: every backend client:
  - `voiceEditTransport.ts`;
  - `webBridgeClient.ts`;
  - the coding-agent WebSocket gateway;
  - the dashboards' reporting gateway.
- Native: no CORS, no mixed-content blocking, no third-party cookie rules.
- Web: all three apply. Sandboxed hosts also block a page's cross-host requests (see `networkProxy` in `clis/multi-browser/src/data/browser/session.ts`).
- Gap: requests that succeed on a phone fail in the browser. `export:web` defaults the backend URLs to localhost mocks (`EXPO_PUBLIC_*_BASE_URL=http://127.0.0.1:…`).
- Mock (T2): never mock responses. Route the app's backend hosts through Playwright → Node (`context.route` fulfilled by Node `fetch`, as `network-proxy` already does) so the browser behaves like native. Build the lab export with live URLs and tokens from the real env (`wf secret sync-env`), not the localhost defaults.
- Drive: automatic in the lab session. `wf browser network-proxy` exists per pattern.
- Done: `open` on a phone preset routes every cross-origin fetch and XHR from any frame through Node (`clis/multi-browser/src/data/browser/network-routing.ts`, `route.fetch` with the browser's proxy and cookies). The request leaves without what only a browser sends (`Origin`, `Referer`, `Sec-Fetch-*`, `Sec-CH-UA*`). The real status, headers and decoded body come back with CORS allowing them, every header exposed. Playwright answers the preflight itself, so no OPTIONS reaches the backend, as on the phone. A host Node cannot reach fails the request; nothing is mocked. The desktop preset keeps the browser's own network.
- Evidence: e2e `deviceLab` S1. A POST with JSON and `authorization` reaches an API on a second origin that sends no CORS headers and refuses preflights. The app reads `200`, the gzip-decoded body and `x-request-id`. The API logs one POST with no `origin` or `sec-fetch-mode`. On the desktop preset the same request fails with `TypeError: Failed to fetch` after the refused OPTIONS.
- Differences that stay: an event stream (`EventSource`, or `Accept: text/event-stream`) stays on the browser's network, CORS included, because it never ends and Node fetches whole. A streamed fetch body arrives whole. A WebSocket handshake carries the page's `Origin` where React Native sends the socket's own; no backend here checks it. With routing on, Playwright turns off the HTTP cache.

**S2 Clipboard**
- Used: `ActionToolbar.tsx` (`Clipboard.setStringAsync` / `getStringAsync`); `rn-agentation`.
- Native: reads and writes freely (iOS shows a paste banner).
- Web: `navigator.clipboard` needs the `clipboard-read`/`clipboard-write` permissions and a focused document.
- Mock (T2): grant both permissions in the lab context. Done: the context grants them at creation, and `permission` no longer revokes them (I5).
- Drive: `wf browser clipboard --name <b> [--set <text>]` returns `{ text }`. Status `done`. Evidence: e2e `deviceLab` S2. The app frame pastes what was set from outside, and the command reads what the app copied.

**S3 Opening external URLs**
- Used: `Linking.openURL` in `features/coding-agent-impl/src/ui/DispatchRunScreen.tsx` and `features/settings-impl/src/ui/AboutSettingsScreen.tsx`.
- Native: leaves the app (browser or another app).
- Web: RNW `Linking.openURL` opens a new tab.
- Mock (T2): an init script in every frame (`opened-urls.ts`) records the URL through a context binding instead of opening it.
  - `window.open(url, target, 'noopener')` is RNW's `Linking.openURL`, reported as `linking`.
  - Any other `window.open` is `window.open`, and a `target=_blank` click is `anchor`.
  - Nothing opens, which beats closing a tab after the fact.
- Drive: `wf browser opened-urls --name <b> [--clear]`. Status `done`.
  - Evidence: e2e `deviceLab` S3. Three taps record `window.open`, `linking` and `anchor` URLs, the browser keeps one page, and `--clear` empties the record.
  - Residual: RNW sends `tel:` links through `window.location`, which cannot be wrapped.

**S4 Deep links in and the initial route**
- Used: expo-router, plus `appConfig.initialRoute`.
- Web: the URL path. Status `done` via `wf browser navigate <path>`.

**S5 `Alert.alert`**
- Used: no.
- Native: a modal system dialog.
- Web: RNW `class Alert { static alert() {} }` does nothing, so a confirm flow silently skips on web.
- Mock (T3): a lab `Alert` that renders a native-styled dialog and records calls.
- Guard (T4): lint fails if `Alert` is imported outside the lab module.
- Status `done` (T3). `lib/device-lab/alert.ts` patches `Alert.alert` in a lab export: the dialog is a 270 px card with system-blue buttons (cancel bold, destructive red), dark-mode aware, shown one at a time, answered by `[data-testid='LabAlert-Button-<n>']` (or the backdrop when `cancelable`), and recorded in `__deviceLab.alerts` with the answering button.
  - Evidence: flow test `deviceLabAlert` (four cases) and a screenshot in the iPhone 15 Pro frame.

**S6 Share sheet**
- Used: no. RNW `Share.share` calls `navigator.share`, which Linux Chromium does not expose.
- Mock (T3): a lab stub that records payloads.

**S7 Android back button**
- Used: no. RNW `BackHandler.addEventListener` logs `console.error` and does nothing.
- Mock (T3): a lab `BackHandler`.
- Drive: new `wf browser back`, which emits `hardwareBackPress` and falls back to `history.back()`.

**S8 App state**
- Used: no. RNW `AppState` follows `visibilitychange`.
- Mock (T2): CDP `Page.setWebLifecycleState`. Spike first: the visibility change may need a second page brought to front.

**S9 AsyncStorage**
- Used:
  - 8 stores and data files in the app and `coding-agent-impl`;
  - 2 stores in `rn-agentation`.
- Web: localStorage. T1, status `done`.
- Flow: `storageHarness`.

**S10 Secure store, S11 Notifications, S15 File system**
- Used: no. The SDK 57 docs list none of the three for web.
- Mock (T3): lab stubs, built only when a feature needs them.

**S12 Location**
- Used: no. Web supported.
- Mock (T2): `context.setGeolocation` plus a permission grant.

**S13 OTA reload**
- Used: `frontend/mla-tamagui/lib/devRestart.ts` (`Updates.reloadAsync`, with `DevSettings.reload` first in dev); `AboutSettingsScreen.tsx`.
- Web: `expo-updates` has no web platform.
- Guard (T4): unit-test the native branch. The web path is a page reload.
- Status `done` (T4). `lib/__tests__/devRestart.test.ts` runs the development reload, the release `Updates.reloadAsync` branch and the rejected reload.

**S14 Constants, device info, launch args**
- Used:
  - `expo-constants` (About screen);
  - `expo-native-launch-arguments` (`frontend/libraries/app-config/src/nativeLaunchArgs.ts`, which degrades gracefully).
- T1, status `done`.

### Native-backed UI libraries

**U1 WebView**
- Used: `features/leetcode/code-editor/core/EditorInput.tsx` and `features/leetcode/ui/MermaidDiagram.tsx`.
- Web: the SDK 57 docs list android and ios only. The app ships its own `lib/webview/ReactNativeWebView.web.tsx`, which uses an iframe with `srcdoc`.
- Gap: message timing, `injectedJavaScript` and storage differ. Known incidents:
  - iOS 17.1 wipes IndexedDB (`docs/archive/solutions/mobile-app/ios-17-1-webview-indexeddb-wipe.md`);
  - the blank-editor `ready` race, caught by flow prerender.
- Guard (T4): a bridge contract test runs the same message script against both transports.
- Status `done` (T4). `tests/flow/webviewBridge.flow.test.tsx` runs one scripted exchange (ready, injected posts, a throwing script, ordered bursts) against a phone-shaped bridge (synchronous, string-only `postMessage`) and the iframe shim; both deliver the same messages. Residual: iOS IndexedDB and `injectedJavaScript` timing need a device.

**U2 Gesture handler, U3 Reanimated, U4 Skia**
- Used:
  - gesture handler: 6 files;
  - reanimated: 7 files;
  - Skia: 3 files.
- Web: all three run their web builds (pointer events, a JS-driven animation loop, CanvasKit WASM). The app forks at `TracingCanvasSkia.tsx:111`.
- Gap: gesture arbitration, UI-thread worklets and Skia rasterisation differ.
- Guard (T4):
  - lint for worklet rules (`mobile-learn/js-function-in-worklet` and related, already present);
  - an explicit residual list per screen.
- Status `done` (T4). `mobile-learn/js-function-in-worklet` is enforced in the app lint, and now understands `.runOnJS(true)` gesture chains, which run on the JS thread. `docs/plans/device-lab/native-ui-residuals.json` lists every importer of the three libraries with what differs and what stays residual; `lib/device-lab/__tests__/nativeUiResiduals.test.ts` fails a new importer until it is listed.

**U5 Bottom sheet**
- Used: `frontend/libraries/tamagui-design-system/ui/sheet.tsx`, which has web branches at 128 and 226.
- Guard (T4): classify the forks (J3) and add a flow test per branch.
- Status `done` (T4). Both forks are classified in `platform-forks.json`; `tests/flow/sheetBranches.flow.test.tsx` runs the web branch (blur, vertical activation window) and the native branch (`Keyboard.dismiss` only, no web prop) of the real `Sheet`.

**U6 Scroll physics, `RefreshControl`, `KeyboardAvoidingView`**
- Used: `ScrollView` in 21 places.
- Web: RNW renders `RefreshControl` and `KeyboardAvoidingView` as plain `View`s, so there is no pull-to-refresh and no avoidance. There is also no iOS bounce or momentum parity.
- Guard (T4): lint fails if `RefreshControl` or `KeyboardAvoidingView` is imported without a lab mock. Existing: `mla-tamagui.no-scrollview-offset`.
- Status `done` (T4). The app lint rejects importing `Alert`, `RefreshControl` or `KeyboardAvoidingView` from `react-native` outside `lib/device-lab`; none is used today, so there is nothing to mock yet. The lab gains a mock when the first use arrives.

**U7 Native stack transitions**
- Used: expo-router `Stack` with `headerShown: false` everywhere, so there are no native headers.
- Residual: the transition animation differs. Screenshots are taken at rest.

**U8 JS-only UI libraries**
- `react-native-svg`, `react-native-markdown-display`, `sonner-native`, `@quidone/react-native-wheel-picker`. T1, status `done`.

**U9 Native view configs (New Architecture)**
- Used: `ActivityIndicator` (1 file); any host component.
- Native: Fabric needs a registered view config. Tamagui's `Spinner` crashed the problem sheet on device ("View config getter callback for component `RCTActivityIndicatorView` must be a function").
- Web: has no view configs, so it never fails.
- Guard (T4): the existing `no-restricted-imports` entry for `Spinner`, extended to a list of host components verified on device.
- Status `done` (T4). `mobile-learn/no-styled-native-host` bans `styled()` over any react-native host component except View and Text, which covers `Spinner`'s cause rather than one name.

### JS engine and web globals

**J1 Hermes API gaps**
- V8 in Chromium has APIs that Hermes and the RN polyfills lack.
- Precedent: `AbortSignal.timeout` broke voice edits on device only. It is now banned by `mobile-learn/no-hermes-unsupported-api` (a seed list).
- Guard (T4): grow the ban list from the Hermes feature matrix: `structuredClone`, `Intl.Segmenter`, `fetch` streaming `response.body`, `TextDecoder` streaming, `crypto.subtle`, and so on, each verified against the shipped Hermes. Unverified entries are marked "spike".
- Status `done` (T4). The ban list gains `structuredClone`, `Intl.Segmenter`, `crypto.subtle` and `response.body.getReader()`, each marked `spike` in its message: there is no Hermes engine for this sandbox to verify them against, so each is banned on the documented gap and lifted with evidence.

**J2 DOM and web globals in shared code**
- Used: `document`, `window`, `localStorage`, `visualViewport` and `matchMedia` in DS code. `frontend/libraries/tamagui-design-system/lib/keyValueStore.ts` guards with `typeof`.
- Guard (T4): lint allows these globals only in `*.web.*` files or behind a `typeof` / `Platform.OS === 'web'` guard.
- Status `done` (T4). `mobile-learn/no-unguarded-web-global` is on for the app tree (tests exempt) and finds nothing there; the design-system and `features/` packages sit outside the app's lint scope and are listed in `platform-forks.json` instead.

**J3 Platform forks**
- The browser only ever runs the `web` branch. On dev there are 25 `Platform.OS`/`Platform.select` uses in 19 files, plus 7 fork files (§9). Each is classified as one of:
  - **inherent:** keep, with a unit test of the native branch;
  - **removable:** make platform-agnostic;
  - **instrumentation:** no behaviour.
- Status `done` (T4). `docs/plans/device-lab/platform-forks.json` classifies all 25 uses and the fork files; `lib/device-lab/__tests__/platformForks.test.ts` fails a new fork until it is listed, and an inherent one needs a native-branch test or a stated residual. The `voiceEditTransport` native `FormData` shape now has its unit test (`voiceEditTransport.native.test.ts`).
- Example (inherent): `voiceEditTransport.ts:96-106` appends a `Blob` on web but `{uri,name,type}` on native. The browser never runs the native upload, so the native `FormData` shape needs a unit test.

**J4 Metro native bundling**
- The web export code-splits `import()`; the native bundle does not.
- Precedent: lazy-loading the old Storybook route black-screened Expo Go (`docs/known-gaps.md:6393`).
- Guard (T4): lint or review rule — no `import()` for code that must run on native, unless it has native verification.
- Status `done` (T4). `mobile-learn/no-unverified-dynamic-import` needs a `native-verified:` comment on every `import()` in the app tree (tests exempt); the one there, a web-only spike route, says why. The settings screen and the chart components import native modules dynamically outside this lint's scope: Metro resolves those to a plain `require` on native.

**J5 Native config**
- Suspected: `app.config.js` sets `runtimeVersion: 'exposdk:54.0.0'` while `expo` is `~57.0.20`. Expo Go only loads updates for its own SDK; verify before acting.
- `expo-audio`'s config plugin is not listed in `plugins`, so a dev/EAS build lacks `NSMicrophoneUsageDescription`. Expo Go ships its own.
- Guard (T4): a config check that each native module in `dependencies` has its plugin, and that the runtime version matches the SDK.
- Status `done` (T4). Verified: `runtimeVersion: 'exposdk:54.0.0'` is the Expo Go client's SDK that `docs/solutions/workflow/eas-publishing.md` publishes for, not a stale copy of the package's SDK, so it stays; `lib/device-lab/__tests__/nativeConfig.test.ts` fails when the two disagree. `expo-audio`'s plugin and its microphone string were missing and are added; the test lists every dependency that ships a config plugin and explains the four deliberately unlisted.

## 5. Parity gaps found in our code

These come from §4, ordered by risk.

1. **Voice upload fork** (`voiceEditTransport.ts:96-106`). The native `{uri,name,type}` branch never runs in a browser (J3).
2. **Recording container.** Web sends webm/Opus; native sends m4a/AAC (I4).
3. **Zero safe-area insets in every device frame**, even though the registry has the values (D2).
4. **Fallback fonts on Linux Chromium** change wrapping (D4).
5. **The WebView editor runs on an in-repo iframe shim**, not `react-native-webview` (U1).
6. **Backend requests**:
   - CORS and the sandbox's cross-host blocking apply only in the browser;
   - the export defaults to localhost mocks (S1).
7. **Microphone permission**: only "granted" is reachable in the browser today (I5).
8. **`runtimeVersion` (54) vs SDK (57)** and the missing `expo-audio` config plugin (J5).
9. **The driver acts like a desktop, not a hand.** It clicks with a mouse and replaces the whole editor buffer in one insert, so touch handling, auto-indent, bracket closing and the Android input path never run (I13, I14).
10. **The Tamil write screen's controls sit in the home-indicator area.** On iPhone 15 Pro, "Check my writing" spans 795–839 px, but the bottom inset starts at 818 px. It went unseen while the browser reported zero insets; D2 made it visible (G2 dry run, 2026-10-05).
11. **The leetcode problem screen shows two headers.** The root stack's "leetcode" bar sits above the screen's own header, with a blank band the height of the top inset between them. Check whether the root header also shows on the phone, then hide it for `leetcode` in `app/_layout.tsx`.

## 6. Render container

The device frame renders the app in an iframe inside a controlled page, `clis/multi-browser/harness-shell`. The old top bar of three `<select>`s and the bezel art read as a debug tool. The new default is a clean screen.

- **Screen-only by default.**
  - The iframe sits at the device's exact viewport, with the device's display corner radius and a soft shadow.
  - It is centred on a quiet backdrop that follows the colour scheme.
  - A status bar (time, signal, battery) and a home indicator are drawn inside the emulated safe areas. That way screenshots read as a phone, and content under the bars is visible as a bug.
- **Bezel on request.** The bezel is opt-in with `frame=bezel`.
- **No option bar.**
  - Device, orientation and scheme come from URL parameters and the daemon bridge (`wf browser open|set-device --color-scheme`, `window.__shell.setDevice/setColorScheme`).
  - A one-line caption under the screen (device · orientation · scheme) is hidden with `chrome=0` for screenshots.
- **Unchanged:** the mock keyboard, `window.__shell`, the `__MCP_KEYBOARD_*` bridge and every `data-testid` the daemon or tests read.
- **Evidence:**
  - `tests/e2e/device-frame-keyboard.test.ts` passes 3/3 against a real headless Chromium: the screen frame, a frameset bezel and a custom bezel, each typing through the mock keyboard.
  - The multi-browser unit suite passes 132/132.
  - Screenshots of the real web export were sent with the PR: iPhone 15 Pro light, iPhone 15 Pro dark, Pixel 8, and iPhone 15 Pro with the keyboard up.

## 7. Acceptance gates

Owner decision, 2026-10-04: three journeys run the way a person runs them, driven by an agent through the headless browser SDK. They are the proof that the browser render stands in for the phone, so the goal is not done until each passes on the final tree.

**Human input model** (applies to every gate)

- **Acts only through input.**
  - Taps, strokes and keys are real input events from the driver (`wf browser tap|stroke|trace|type|press`); speech reaches the app through the fake microphone (`wf harness voice-fixtures drive`).
  - `eval` and `outer-eval` may read the page. They never click, focus, set a value or dispatch an event.
  - No paste, no select-all-and-replace, no multi-character insert.
- **Sees only the screen.** Decisions come from screenshots, the accessibility snapshot and the geometry of what is drawn: an element's box, or the guide line a pad draws. Never from app source, stores, course data or fixtures.
- **Touch, not mouse.** Taps and strokes are touch pointers with human timing (I13). Typing goes in at the caret through the platform's soft-keyboard model, with the mock keyboard raised by the tap that focused the field (I14).
- **Human pace.**
  - Strokes run slow–fast–slow at hand speed, with tremor.
  - Typing runs at thumb speed.
  - The agent looks (a screenshot or snapshot) before each decision.
- **Visible in the recording.**
  - Every run records video in the default screen frame on `iphone-15-pro`.
  - Every tap shows a dot, every stroke its trail and every key a tint on the mock keyboard.
  - `wf browser video-check` confirms a dot at every logged tap.
- **The render under test** is the lab web export (`EXPO_PUBLIC_DEVICE_LAB=1`) served statically, with the live backends (S1).

**How a gate runs**

1. Land the commit under test on `dev` first, so the recorded sha stays on the history.
2. An agent drives the runbook step by step through `wf browser`, under the human input model, recording with `--video`. It writes the run to `docs/runbooks/.results/<gate>.json` (`runbook`, `sha`, `ranAt`, `verdict`, `evidence`). The greenfield cutover removed the runner that used to record runs (`wf runbook run --record`).
3. Publish the videos, screenshots and input logs as one page.
4. Tick the gate in [`progress.md`](progress.md) with the record's sha and the page's link.

A failing gate is a bug to fix in the app, the mock or the driver, never a threshold to relax.

**G1 Voice edit, recorded with tap dots** ([`device-lab-voice-edit.md`](../../runbooks/device-lab-voice-edit.md))
- Runs problems 1–3 of [`voice-edit-iterative-solve.md`](../../runbooks/voice-edit-iterative-solve.md), spoken pass, under the human input model.
- Every tap of the voice flow is a touch tap with a dot: record, send, confirm, pick an alternative, Run Local and reset.
- Pass:
  - that runbook's gates 1–7 hold for all three problems;
  - Run Local reaches all-pass on each problem;
  - `video-check` finds a dot at every logged tap.
- Evidence: one video per problem, plus the JSON line of every turn.
- Final run (2026-10-05, `df9e1c983`): passed, 14 / 14, 12 / 12, 11 / 11, after five defects found by the earlier run were fixed (planner provider and `top_p`, whole-solution limit, rename matching, bodiless block header); page `g1-final-2026-10-05`. Earlier run (`5df8a002a`): passed, with the agent run from the checkout. `valid-palindrome` 14 / 14, `two-sum` 12 / 12, `container-with-most-water` 11 / 11; the whole-solution ask refused each time; every rename local; transcribe p50 883 ms and plan p50 2795 ms; 31–35 tap dots per video, all present. The attempts before it found four defects, all fixed first: the planner named a Kimi provider models.dev no longer lists, a whole-solution ask returned a passing solution, and the local rename missed names whisper writes as homophones, splits in two or speaks as words. Evidence: [page](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g1-2026-10-05/index.html).

**G2 Tamil writing, marked right** ([`device-lab-tamil-writing.md`](../../runbooks/device-lab-tamil-writing.md))
- **Letters on the lesson pad.** Lesson `uyir-1` (அ ஆ இ ஈ உ ஊ) and the first letter of `uyirmei-k` (க) are written stroke by stroke along the guide the pad draws.
  - Pass: every stroke is accepted within two attempts.
  - Pass: every letter scores at least 80 % ON THE LINE, the app's own local similarity check and the floor `verify-tamil.mjs` holds exact walks to.
- **A letter written large.** `tamil/write/uyir-a` is written over the faint template, then "Check my writing". Pass: `LOOKS RIGHT` from the live marker (`POST /api/tamil/check`).
- **Words.** `tamil/write/word-uyir-a` (அம்மா), `word-uyir-aa` (ஆடு) and `word-uyir-i` (இலை) are written over the faint template and checked. Pass: `LOOKS RIGHT` for each within two attempts (Clear, then write again).
- **Where the strokes come from.** On the lesson pad, from the guide line it draws. On the write pad, from `wf browser trace`, which reads the template's centre lines off the pad's pixels and replays them as human strokes. Never from glyph tables, course data or an injected SVG.
- Evidence: the video, plus a screenshot of each letter and word with its score or verdict.
- Dry run (2026-10-05, today's driver: mouse input, no tremor): all seven letters were accepted on the first attempt at 100 % ON THE LINE. The large அ came back "NOT YET · 0, Could not reach the marker", because `remote-chat-agent-prod` is not on the tailnet from the sandbox (`docs/known-gaps.md`). Evidence: [screenshots, video and log](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g2-dry-run-2026-10-05/index.html).
- Run (2026-10-05, `d251cef5b`): passed, with the marker run from the checkout. Seven letters at 97–100 % ON THE LINE with no stroke refused; அ large and the three words LOOKS RIGHT at 91–95, each at the first attempt; 9 of 9 tap dots. The attempts before it found seven defects, all fixed first: the trace drew the pad's border and missed dots, the lab cut requests at 30 s and drew Tamil in FreeSerif, the verdict shrank the pad under the ink, a word overflowed its pad, and the marker clipped the printed word and ran on a model that passed wrong words. Evidence: [recording, screenshots and input log](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g2-2026-10-05/index.html).

**G3 CodeMirror, typed like a person** ([`device-lab-codemirror-typing.md`](../../runbooks/device-lab-codemirror-typing.md))
- `valid-palindrome` and `two-sum` in Python, each from its starting stub.
- The agent writes each solution the way a person does on a phone:
  - taps the editor at the end of the stub's last line, which raises the keyboard;
  - types line by line, tapping the syntax toolbar for the symbols it offers (`EditorSyntax-Colon`, `-Paren`, `-Bracket`, `-Equals`, `-Underscore`, `-Dot`);
  - presses Return for new lines and lets CodeMirror indent, dedenting with Backspace or `EditorSyntax-Dedent`;
  - types over auto-closed brackets instead of doubling them;
  - makes at least one typo per problem, sees it in a screenshot and fixes it with Backspace.
- Pass:
  - Run Local reports every test passing (`14 / 14` for `valid-palindrome`);
  - the input log shows no paste, no replace and no multi-character insert;
  - the inter-key p50 is within 150–300 ms.
- Evidence: the video, the final screenshot with the Run Local result, and the input log.
- Run (2026-10-05, `dd73fab52`): passed. `valid-palindrome` 14 / 14 and `two-sum` 12 / 12, 282 single keys at a 221 ms p50, 39 of 39 tap dots. The attempts before it found five defects, all fixed first: the toolbar's bracket pair and colon did not act as typed, Return indented with tabs, a tap could land off the glass, and the export kept the development backend token. Evidence: [recording, screenshots and input log](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g3-2026-10-05/index.html).

## 8. Phases

The goal file drives all phases autonomously: there are no review gates, `dev` is pulled every iteration, and work lands on `dev`.

0. **This spec.** The render container (§6), [`progress.md`](progress.md) and the goal.
1. **Contract and units.** Derive [`contract.md`](contract.md) (seams: driver commands, lab module API, guard rules) and `units.jsonl` from §3–§4 and §7.
2. **Lab export.** The `EXPO_PUBLIC_DEVICE_LAB` gate and the `device-lab` sentinel. A flagless export contains no sentinel.
3. **Chromium emulation (T2 rows).**
   - device metrics;
   - safe area;
   - fonts;
   - permissions;
   - camera feed;
   - file upload;
   - clipboard;
   - external URLs;
   - network through Node;
   - touch input;
   - soft-keyboard text entry.
   Each gets a `wf browser` command and an e2e test.
4. **Acceptance gates (§7).** G1 voice edit, G2 Tamil writing and G3 CodeMirror typing, each run by an agent and recorded.
5. **In-app lab mocks (T3 rows).** Alert and the status bar chrome; the rest stay as P3 stubs.
6. **Static guards (T4 rows):**
   - Hermes list;
   - web globals;
   - fork classification and native-branch tests;
   - `import()`;
   - native config;
   - view configs;
   - `RefreshControl`/`KeyboardAvoidingView`.
7. **Flow-test reuse and done.**
   - Seeded safe-area insets and the permission double, plus the agent docs.
   - Exit: no P1 or P2 row in §3 is `todo`.
   - Exit: each gate's last passing run postdates every change to the code it exercises.

## 9. Defaults and appendix

**Defaults**

The autonomous loop uses these unless the owner objects on the PR:

- **The agent driver stays in `clis/multi-browser` (`wf browser …`).** It is the only browser driver, and greenfield's delete-with-check verdict predates this need.
- **Fonts:** Roboto for Android and Inter for iOS (D4).
- **`export:web` sets `EXPO_PUBLIC_DEVICE_LAB=1`.** Production is the EAS Update, which never sets it.
- **Residual rows (I2, I8, U7) are accepted** with the guards written above.

**Inventory commands, run on `dev` at `ca91f7d`**

```sh
git grep -h -o -E "(from|import\(|require\()\s*['\"](expo-[a-z-]+|react-native-[a-z-]+|@shopify/react-native-skia|@gorhom/bottom-sheet|…)['\"]" -- frontend/mla-tamagui features libraries ':!**/*.test.*' ':!**/__tests__/**'
git grep -n -E "Platform\.(OS|select)" -- frontend/mla-tamagui features libraries ':!**/*.test.*' ':!**/__tests__/**'
git ls-files frontend/mla-tamagui features libraries | grep -E '\.(web|native|ios|android)\.(ts|tsx|js)$'
```

**Native-backed imports**

| Package | Imports |
|---|---|
| `expo-router` | 39 |
| `@react-native-async-storage/async-storage` | 10 |
| `react-native-reanimated` | 7 |
| `react-native-gesture-handler` | 6 |
| `react-native-safe-area-context` | 5 |
| `react-native-svg` | 4 |
| `@shopify/react-native-skia` | 3 |
| `victory-native` | 3 |
| `@quidone/react-native-wheel-picker` | 3 |
| `react-native-webview` | 2 |
| `expo-audio` | 2 |
| `expo-clipboard` | 2 |
| `expo-updates` | 2 |
| `sonner-native` | 2 |
| `expo-constants` | 1 |
| `expo-native-launch-arguments` | 1 |
| `@gorhom/bottom-sheet` | 1 |
| `react-native-markdown-display` | 1 |

RN core APIs imported:

- 21 `ScrollView`
- 17 `Platform`
- 4 `useWindowDimensions`
- 3 `useColorScheme`
- 3 `TextInput`
- 3 `Keyboard`
- 2 `PanResponder`
- 2 `Linking`
- 1 `DevSettings`
- 1 `ActivityIndicator`

**Fork files:**

- `frontend/mla-tamagui/lib/webview/ReactNativeWebView.web.tsx`
- `frontend/libraries/rn-agentation/src/capture/capture.{native,web}.ts`
- `frontend/libraries/rn-agentation/src/document/driver/documentTree.{native,web}.ts`
- `frontend/libraries/tamagui-design-system/composites/keyboard-toolbar/portal.web.tsx`
- `frontend/libraries/tamagui-design-system/lib/keyboard/keyboardSource.web.ts`

**Expo SDK 57 catalogue**

The data comes from each docs page's `platforms` frontmatter, fetched 2026-10-04. An asterisk means the docs flag a platform caveat.

The "`expo-go` badge" column shows whether the page carries the badge. A missing badge does not prove the module is absent: `expo-haptics` has no badge.

| Module page | Package | Web | `expo-go` badge | Platforms |
|---|---|---|---|---|
| accelerometer | `expo-sensors` | **yes** | yes | android, ios*, web |
| age-range | `expo-age-range` | no | yes | android, ios |
| app-integrity | `@expo/app-integrity` | no | yes | android, ios |
| apple-authentication | `expo-apple-authentication` | no | yes | ios, tvos |
| application | `expo-application` | **yes** | yes | android, ios, web, tvos |
| asset | `expo-asset` | **yes** | yes | android, ios, tvos, web |
| async-storage | `@react-native-async-storage/async-storage` | **yes** | yes | android, ios, tvos, macos, web |
| audio | `expo-audio` | **yes** | yes | android, ios, web, tvos |
| auth-session | `expo-auth-session` | **yes** | yes | android, ios, web |
| background-fetch | `expo-background-fetch` | no | no | android, ios |
| background-task | `expo-background-task` | no | yes | android, ios, tvos |
| barometer | `expo-sensors` | no | yes | android, ios* |
| battery | `expo-battery` | **yes** | yes | android, ios*, web |
| blob | `expo-blob` | **yes** | yes | android, ios, web |
| blur-view | `expo-blur` | **yes** | yes | android, ios, tvos, web |
| brightness | `expo-brightness` | no | yes | android, ios |
| calendar | `expo-calendar` | no | no | ios*, android* |
| calendar-legacy | `expo-calendar` | no | yes | android, ios |
| camera | `expo-camera` | **yes** | yes | android*, ios*, web |
| cellular | `expo-cellular` | **yes** | yes | android, ios, web |
| checkbox | `expo-checkbox` | **yes** | yes | android, ios, tvos, web |
| clipboard | `expo-clipboard` | **yes** | yes | android, ios, web |
| constants | `expo-constants` | **yes** | yes | android, ios, tvos, web |
| contacts | `expo-contacts` | no | yes | android, ios |
| contacts-legacy | `expo-contacts` | no | yes | android, ios |
| crypto | `expo-crypto` | **yes** | yes | android, ios, tvos, web |
| date-time-picker | `@react-native-community/datetimepicker` | no | yes | android, ios |
| device | `expo-device` | **yes** | yes | android, ios, tvos, web |
| devicemotion | `expo-sensors` | **yes** | yes | android, ios, web |
| document-picker | `expo-document-picker` | **yes** | yes | android, ios, web |
| filesystem | `expo-file-system` | no | yes | android, ios, tvos |
| filesystem-legacy | `expo-file-system` | no | yes | android, ios, tvos |
| flash-list | `@shopify/flash-list` | **yes** | yes | android, ios, tvos, web |
| font | `expo-font` | **yes** | yes | android, ios, tvos, web |
| gesture-handler | `react-native-gesture-handler` | **yes** | yes | android, ios, web |
| gl-view | `expo-gl` | **yes** | yes | android, ios, web |
| glass-effect | `expo-glass-effect` | no | yes | ios, tvos |
| gyroscope | `expo-sensors` | **yes** | yes | android, ios*, web |
| haptics | `expo-haptics` | **yes** | no | android, ios, web |
| image | `expo-image` | **yes** | yes | android, ios, tvos, web |
| imagemanipulator | `expo-image-manipulator` | **yes** | yes | android, ios, tvos, web |
| imagepicker | `expo-image-picker` | **yes** | yes | android, ios, web |
| intent-launcher | `expo-intent-launcher` | no | yes | android |
| keep-awake | `expo-keep-awake` | **yes** | yes | android, ios, tvos, web |
| keyboard-controller | `react-native-keyboard-controller` | no | yes | android, ios |
| light-sensor | `expo-sensors` | no | yes | android |
| linear-gradient | `expo-linear-gradient` | **yes** | yes | android, ios, tvos, web |
| linking | `expo-linking` | **yes** | yes | android, ios, web, tvos |
| live-photo | `expo-live-photo` | no | yes | ios |
| local-authentication | `expo-local-authentication` | no | yes | android, ios |
| localization | `expo-localization` | **yes** | yes | android, ios, tvos, web |
| location | `expo-location` | **yes** | yes | android, ios, web |
| magnetometer | `expo-sensors` | no | yes | android, ios |
| mail-composer | `expo-mail-composer` | **yes** | yes | android, ios*, web |
| map-view | `react-native-maps` | no | yes | android, ios |
| maps | `expo-maps` | no | no | ios, android |
| masked-view | `@react-native-masked-view/masked-view` | no | yes | android, ios, tvos |
| media-library | `expo-media-library` | no | yes | android, ios, tvos |
| media-library-legacy | `expo-media-library` | no | yes | android, ios, tvos |
| mesh-gradient | `expo-mesh-gradient` | no | yes | android, ios, tvos |
| navigation-bar | `expo-navigation-bar` | no | yes | android |
| netinfo | `@react-native-community/netinfo` | **yes** | yes | android, ios, tvos, web |
| network | `expo-network` | **yes** | yes | android, ios, web, tvos |
| notifications | `expo-notifications` | no | no | android, ios |
| pedometer | `expo-sensors` | no | yes | android, ios |
| picker | `@react-native-picker/picker` | **yes** | yes | android, ios, macos, web |
| print | `expo-print` | **yes** | yes | android, ios, web |
| reanimated | `react-native-reanimated` | **yes** | yes | android, ios, tvos, web |
| router | `expo-router` | **yes** | yes | android, ios, tvos, web |
| safe-area-context | `react-native-safe-area-context` | **yes** | yes | android, ios, web, tvos |
| screen-capture | `expo-screen-capture` | no | yes | android, ios |
| screen-orientation | `expo-screen-orientation` | **yes** | yes | android, ios, web |
| screens | `react-native-screens` | **yes** | yes | android, ios, tvos, web |
| securestore | `expo-secure-store` | no | yes | android, ios, tvos |
| segmented-control | `@react-native-segmented-control/segmented-control` | **yes** | yes | android, ios, web |
| sensors | `expo-sensors` | **yes** | yes | android, ios, web |
| sharing | `expo-sharing` | **yes** | yes | android, ios, web |
| skia | `@shopify/react-native-skia` | **yes** | yes | android, ios, tvos, web |
| slider | `@react-native-community/slider` | **yes** | yes | android, ios, web |
| sms | `expo-sms` | no | yes | android, ios |
| speech | `expo-speech` | **yes** | yes | android, ios, web |
| splash-screen | `expo-splash-screen` | no | no | android, ios, tvos |
| sqlite | `expo-sqlite` | **yes** | yes | android, ios, macos, tvos, web |
| status-bar | `expo-status-bar` | **yes** | yes | android, ios, tvos, web |
| storereview | `expo-store-review` | no | yes | android, ios |
| stripe | `@stripe/stripe-react-native` | no | yes | android, ios |
| svg | `react-native-svg` | **yes** | yes | android, ios, macos, web, tvos |
| symbols | `expo-symbols` | **yes** | yes | android, ios, tvos, web |
| system-ui | `expo-system-ui` | **yes** | no | android, ios, tvos, web |
| task-manager | `expo-task-manager` | no | yes | android, ios, tvos |
| tracking-transparency | `expo-tracking-transparency` | no | yes | android, ios, tvos |
| ui | `@expo/ui` | no | yes | android, ios, tvos |
| updates | `expo-updates` | no | yes | android, ios, tvos |
| video | `expo-video` | **yes** | yes | android, ios, web, tvos |
| video-thumbnails | `expo-video-thumbnails` | no | yes | android, ios, tvos |
| view-pager | `react-native-pager-view` | no | yes | android, ios |
| webbrowser | `expo-web-browser` | **yes** | yes | android, ios, web |
| webview | `react-native-webview` | no | yes | android, ios |
| widgets | `expo-widgets` | no | no | ios |

**RNW 0.21 behaviour cited above** (`react-native-web/dist/exports/*/index.js`)

| API | Web behaviour |
|---|---|
| `Alert.alert` | Empty function |
| `BackHandler.addEventListener` | `console.error` and a no-op |
| `Keyboard.addListener` | No-op |
| `Keyboard.isVisible()` | `false` |
| `KeyboardAvoidingView` | Plain `View` |
| `RefreshControl` | Plain `View` |
| `StatusBar` | Renders `null` |
| `Share.share` | `navigator.share` |
| `Vibration` | `navigator.vibrate` |
| `Linking.openURL` | New tab |
| `AppState` | `visibilitychange` |
| `Appearance` | `matchMedia` |

**Build vs adopt, dropped with the catalog**

The figures come from a registry closure walk with app-provided peers excluded:

| Candidate | Packages | Size | Notes |
|---|---|---|---|
| Storybook RN 10.6.0 on-device | 189 | 69.0 MB | Peer-requires `react-native-safe-area-context` 5.8.0; SDK 57 ships `~5.7.0` |
| Storybook RNW-Vite 10.6.1 | 183 | 54.8 MB | |
| React Cosmos 7.4.1 + native | 139 | 16.0 MB | |
| expo-dev-client 57.0.19 | 8 | 3.7 MB | Dev-menu items "not compatible with Expo Go" |

All four are MIT-licensed. None controls phone APIs.

Audit baseline: the lockfile already carries 79 high and 2 critical advisories (registry bulk advisory API), so `pnpm audit --audit-level=high` is red before any change.

**Relay research, kept in case native control returns**

- Expo CLI 57.0.22 mounts `/expo-dev-plugins/broadcast` on every `expo start` (`DevToolsPluginWebsocketEndpoint.js`). It is a `ws` server that rebroadcasts each message to every other client.
- `vm-ops-api` has no WebSocket server.
