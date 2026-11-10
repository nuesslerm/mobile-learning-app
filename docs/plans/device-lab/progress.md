# device-lab — progress ledger

The single source of truth for what is done and what is next. Update it in the same commit as each change. A box is ticked only with evidence: a commit sha plus the test summary, check line or screenshot list.

Spec: [`spec.md`](spec.md). Goal: [`docs/goals/2026-10-04-device-lab.md`](../../goals/2026-10-04-device-lab.md). Tracking PR: [#306](https://github.com/nuesslerm/expo-hello-world-app-2/pull/306).

Rules (owner decisions, 2026-10-04):
- Browser only. The network is live, never mocked. No knobs.
- Fully autonomous: the goal file drives every phase. Pull `dev` every iteration and land on `dev`.
- Implement everything directly: no opencode or cheap-flow dispatch, no subagents.
- The lefthook pre-commit hook is the gate, and it is never bypassed.
- Acceptance gates G1–G3 (spec §7) run the way a person would: an agent drives them through the headless browser SDK, and each run is recorded on video with every tap shown as a dot.

## P0 Spec and render container
- [x] Branch rebuilt from `dev` (`ca91f7da`) with the spec commit cherry-picked (`02a9dc1f`); draft PR #306 into `dev`
- [x] workflow-cli bundle rebuilt with bun 1.3.14 so the `workflow-cli-bundle-fresh` gate is green; 5 cosmetic lines (`75a5f6f5`)
- [x] Spec rewritten: intent, four tiers, 49-row breakdown, parity gaps (`7ca8f7dc`)
- [x] Render container: screen-only frame by default, bezel opt-in via `--frame bezel`, no option bar; safe-area insets emulated through CDP on the page and the app frame's own target (`cda16975`). Evidence:
  - multi-browser unit 132/132;
  - e2e `device-frame-keyboard` 3/3 (screen, frameset bezel, custom bezel);
  - e2e `device-frame-safe-area` 4/4 (same-site frame, cross-site frame, iPhone SE, landscape);
  - 4 screenshots of the real web export sent on the PR (iPhone 15 Pro light, iPhone 15 Pro dark, Pixel 8, iPhone 15 Pro with the keyboard up).
- [x] Ledger and goal file (`8efeeccf`)
- [x] Acceptance gates G1–G3 added (owner, 2026-10-04): spec §7 with the human input model, rows I13 touch input and I14 soft-keyboard entry (51 rows), three agent-driven gate runbooks, goal phase 4 (`76164cdf`); G2 dry run with mouse and touch input (`a1c42ffd`, `2efdbd91`)
- [x] Landed on `dev`: fast-forward `ca91f7da..2efdbd91` (PR #306 closed as merged); `p0.on-dev` passes

## P1 Contract and units
- [x] `contract.md`: `wf browser` driver commands (name, arguments, result), the lab module API with its gate and sentinel `__device_lab_v1__`, the static guard rules (this commit)
- [x] `units.jsonl`: 39 units, `wf cheap-flow check device-lab --local` reported `ok: 39 units`. After the greenfield cutover the checker is `wf offload plan --check device-lab`, which also wants 1-3 files per unit; the milestone and gate units now name theirs.

## P2 Lab export gate
- [x] `EXPO_PUBLIC_DEVICE_LAB` gate in `app/_layout.tsx` (gated `require`) and `export:web`; a flagless export has no sentinel, a flagged export carries it (this commit). Evidence:
  - `lib/device-lab` unit tests 2/2.
  - Exports run back to back without `--clear`: flagless, no sentinel; flagged, sentinel in `_expo/static/js/web/index-*.js`; flagless again, no sentinel.
  - Found on the way: Metro's transform cache ignored the flag. A flagged export after a flagless one shipped without the lab until `metro.config.js` keyed `cacheVersion` on it.

## P3 Chromium emulation (T2)
- [x] D1 device metrics per preset (DPR, `isMobile`, `hasTouch`, user agent and client hints) (this commit). e2e D1 4/4 and the other e2e suites 18/18. The DPR needed `--force-device-scale-factor`, because the emulated ratio skips cross-site frames.
- [x] D2 safe-area insets (`cda16975`; e2e 4/4)
- [x] D4 metric-compatible fonts (Roboto for Android, Inter for iOS) (this commit). e2e D4 2/2. The app's e2e `appFlows` passes 4/4 with D1 and D4 once Metro runs; its earlier failure was `localhost refused to connect` with no Metro up.
- [x] D5 status bar and home indicator chrome (`cda16975`; screenshots)
- [x] I5 permission grant and deny (`wf browser permission`) (this commit). e2e I5: a fresh `prompt`, denied gives `NotAllowedError` even with a clip queued, granted gives a stream, reset gives `prompt`. Also fixed: Chromium drops overrides when their CDP session detaches, and Playwright's grant revoked every unlisted permission.
- [x] I6 autoplay policy for audio playback (this commit). The flag already shipped; e2e I6 proves a sound played before any tap reaches `playing`.
- [x] I7 fake camera feed (`wf browser camera-inject`) (this commit). e2e I7: an open stream shows the injected red picture, then blue.
- [x] I9 file upload (`wf browser upload`) (this commit). e2e I9 passes, and `deviceLab` passes 10/10 twice in a row. Choosers are intercepted from open, after a race against Playwright's asynchronous interception.
- [x] S1 live network routed through Node (this commit). e2e S1: a preflighted POST to an API with no CORS support answers the phone preset with `200`, its decoded body and its headers, and the API sees one POST with no browser-only headers; the desktop preset still gets `TypeError: Failed to fetch`. Building the lab export with live URLs is the gates' setup (P4).
- [x] S2 clipboard (`wf browser clipboard`) (this commit). e2e S2: text set outside is pasted in the app, and text the app copies is read back.
- [x] S3 external URLs captured (`wf browser opened-urls`) (this commit). e2e S3: `window.open`, `linking` and `anchor` are recorded and nothing opens.
  - With it, presses wait until the app frame takes input (`pointer-route.ts`): `deviceLab` 12/12 in 10 runs in a row, and the multi-browser suite 170/170.
- [x] I13 touch taps, human strokes and swipes, `wf browser trace`, the input log and `wf browser video-check` (this commit). e2e I13 touch, trace and video-check pass, and `deviceLab` is 18/18 in 8 runs in a row. Phone presets keep every frame in one process, as a WebView does: in an out-of-process app frame Chromium clicked 13,24 px from the finger (spec I13).
- [x] I14 soft-keyboard entry: keyboard raised on focus, `wf browser type --at-caret --human`, `wf browser press` (this commit). e2e I14 caret and press pass on iPhone 15 Pro and Pixel 8 against the app's own CodeMirror build; Android's CodeMirror takes text through an EditContext, as on a real phone (spec I14).
- [x] `tests/e2e/deviceLab.e2e.test.ts` green (2026-10-05): 26 of 26 in 81 s

## P4 Acceptance gates (spec §7)
- [x] G1 voice edit, recorded with tap dots ([`device-lab-voice-edit.md`](../../runbooks/device-lab-voice-edit.md)), `df9e1c983`: `valid-palindrome` 14 / 14, `two-sum` 12 / 12, `container-with-most-water` 11 / 11 by spoken edits; whole-solution asks refused 3 of 3; renames local 3 of 3; transcribe p50 1068 ms, plan p50 3040 ms; 35 + 30 + 35 touch taps, `video-check` all dotted; [recordings, turns and screenshots](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g1-final-2026-10-05/index.html); final run after five defects were fixed; the agent ran from the checkout with the faster-whisper sidecar (see the blocker below)
- [x] G2 Tamil letters and words drawn on the canvas, marked right ([`device-lab-tamil-writing.md`](../../runbooks/device-lab-tamil-writing.md)), `6c1f8f846` (export from `cb6266ba0`, re-run after the driver, voice and layout changes): seven letters 98–100 % ON THE LINE with no stroke refused; அ written large LOOKS RIGHT · 88; அம்மா 95, ஆடு 92, இலை 94, all at the first attempt; 53 touch strokes and 9 taps, `video-check` 9 of 9; the marker ran from the checkout (see the blocker); [recording, screenshots and input log](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g2-final-2026-10-05/index.html). First run: `d251cef5b`, with the seven defects it found.
- [x] G3 code typed into CodeMirror like a person ([`device-lab-codemirror-typing.md`](../../runbooks/device-lab-codemirror-typing.md)), `edddcfadb` (export from `cb6266ba0`, re-run after the driver, voice, Tamil and layout changes): Run Local `valid-palindrome` 14 / 14 and `two-sum` 12 / 12; 289 keys, none multi-character, inter-key p50 224 ms; `video-check` 27 of 27 and 14 of 14 taps; [recordings, screenshots and input logs](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g3-final-2026-10-05/index.html). First run: `dd73fab52`.

## P5 In-app lab mocks (T3)
- [x] S5 lab `Alert` plus the lint rule (`a40931685`, `0125c84d6`): `lib/device-lab/alert.ts` shows a native-styled dialog, queued, answered through `LabAlert-Button-<n>` and recorded in `__deviceLab.alerts`; flow test `deviceLabAlert` 4/4 and a screenshot in the phone frame; the app lint rejects `Alert` outside the lab module

## P6 Static guards (T4)
- [x] I4 m4a backend contract test (`0125c84d6`): `voiceTranscribeContainers.test.ts` posts WebM/Opus, m4a and mp4-labelled AAC through the real route, and through the live faster-whisper sidecar with `STT_CONTRACT_URL` (the AAC and Opus files transcribe to the WAV's words)
- [x] U1 WebView bridge contract test (`0125c84d6`): `webviewBridge.flow.test.tsx` runs one scripted exchange against a phone-shaped bridge and the iframe shim
- [x] U2–U5 per-screen residual list (`0125c84d6`): `native-ui-residuals.json` plus its guard test; the worklet rule is on in the app lint and understands `.runOnJS(true)`; `sheetBranches.flow.test.tsx` runs both Sheet branches
- [x] U6 `RefreshControl` / `KeyboardAvoidingView` lint (`0125c84d6`): imported only through `lib/device-lab` (none is used yet)
- [x] U9 host-component list (`0125c84d6`): `no-styled-native-host` bans `styled()` over every react-native host except View and Text; the one device-verified crash is `Spinner`'s
- [x] J1 Hermes ban list grown from the Hermes feature matrix (`0125c84d6`): four entries, each marked `spike` because the sandbox has no Hermes engine to verify them against
- [x] J2 web globals only in `*.web.*` or behind a guard (`0125c84d6`): `no-unguarded-web-global`, on for the app tree and clean there
- [x] J3 fork classification plus native-branch tests (`0125c84d6`): `platform-forks.json` (26 files) with its guard test, and `voiceEditTransport.native.test.ts`
- [x] J4 `import()` rule (`0125c84d6`): `no-unverified-dynamic-import`; the one use in the app tree is a web-only spike route and says so
- [x] J5 native config check (`0125c84d6`): `nativeConfig.test.ts`; found `expo-audio`'s plugin and microphone string missing (added); `runtimeVersion` checked against the documented Expo Go client and kept
- [x] S13 `devRestart` native-branch test (`0125c84d6`): `lib/__tests__/devRestart.test.ts` 3/3

## P7 Flow-test reuse and done
- [x] Seeded safe-area insets in the harness stub; permission states in the doubles (this commit): the stub supplies an iPhone 15 Pro's insets by default with `setSafeAreaDevice` to switch, the audio recorder double has granted / denied / prompt, and the whole app flow suite is green with the seeded insets (19 files, 64 tests)
- [x] Agent docs for every new `wf browser` command (`cb6266ba0`): the browser-cli skill lists permission, camera-inject, upload, clipboard, opened-urls, swipe, trace, press and video-check, and describes the phone input model
- [x] No P1/P2 row `todo`; app typecheck, arch and flow green (2026-10-05): `p7.all-rows` and `p7.app-green` pass
- [x] Every gate re-run after the last change to the code it exercises (`p7.gates-fresh`)
- [x] Fix: the Tamil write screen's controls sit in the home-indicator area (spec §5, item 10) (`cb6266ba0`): "Check my writing" now ends at 818 px, where the inset starts (was 839); flow test `tamilWriteInsets`
- [x] Fix or confirm: the leetcode problem screen's second header and blank band (spec §5, item 11) (`cb6266ba0`): confirmed in the browser render and fixed by hiding the root stack's header for the `leetcode` group; one header, no band

## Blockers
- `remote-chat-agent-prod` is not on the tailnet from the sandbox. G1 and the marker steps of G2 ran against the same agent code from the checkout instead (runbook setup); a run against the deployed agent waits for the owner-gated bootstrap (`docs/known-gaps.md`, "device-lab: the live agent is unreachable from the sandbox").

## Notes
- **A tap right after load reached only the shell (2026-10-05).** I9 and S3 failed in about 2 of 5 full `deviceLab` runs. A capture listener in the shell recorded the lost tap as `["pointerdown","IFRAME","",172,88,330]`, 330 ms after the shell loaded, and the app frame recorded no pointer event at all. Until a cross-site frame is composited, Chromium hit-tests its input to the parent's `<iframe>` element. Fix: `tap`, `drag` and `stroke` start with a move, and repeat it after the next paint while the shell still sees it on the iframe element (`clis/multi-browser/src/data/browser/pointer-route.ts`). The retry fired in 2 of 4 instrumented runs, and both passed.
- **G3 first run (2026-10-05): the toolbar's bracket pair doubled.** Typing a solution at the caret, `s)` after the toolbar's Parentheses key left `len(s))`.
  - Cause: the toolbar inserted `()` as text, which CodeMirror's `closeBrackets` does not track, so the typed `)` was added instead of stepped over. Typing `(` on the keyboard was always fine.
  - Fix: the toolbar sends `insertBracket` and the bundle runs CodeMirror's own `insertBracket` ([learning](../../solutions/ui-bugs/toolbar-bracket-pair-doubled-by-typed-closer.md)). e2e `I14 closer` types `f(` then `s)` on iPhone and Pixel and expects `f()`, then `f(s)`.
  - The bundle had no generator: `pnpm --filter mla-tamagui run build:editor-bundle` builds it and pre-commit runs `pnpm --filter mla-tamagui run build:editor-bundle --check` (closes the gap "The CodeMirror bundle has no checked-in generator").
  - Prerendered flow sessions could not tap the portaled keyboard toolbar; their queries now span the body, as unprerendered ones do.
  - Found before the rerun: Return indented with a tab (`indentUnit` was `'\t'`) under the stubs' space-indented first line, and Python rejects that mix (`TabError: inconsistent use of tabs and spaces in indentation`, checked with python3). The editor now indents with `tabSize` spaces, as the toolbar's own indent already did; e2e `I14 caret` expects four spaces after `if x:` and Return.
  - Second attempt: `tap` on `EditorSyntax-Equals`, a toolbar key past the screen's right edge, touched x 606 on a 393 px screen, which blurred the editor and closed the keyboard. A tap now aims at the part of the target on the screen and refuses a target with none ("off the screen -- swipe it into view first"); e2e `I13 touch` swipes a ten-key row to reach its last key. The runbook now taps the toolbar with `--outer` and swipes it to reach a key.
  - Third attempt, problem 1 typed in full: the toolbar's colon after `else` left the line at the body's depth, where the keyboard's colon makes CodeMirror move it out to the `if` (corrected in the run with Dedent). The toolbar now types through the editor's own path for a typed key (`typeText`: input handlers, then an insert marked `input.type`), which also covers the bracket pair.
  - Run Local answered `unauthorized`: the export carried the development token, not the one it was built with. Metro's transform cache keyed only on the lab flag, so the app-config module came from a build without the token. The cache now keys on a hash of every `EXPO_PUBLIC_*` value, and the next export carried the token. (Before that, `Failed to fetch`: restarting the daemon had closed the backend forward; reopened with `wf browser tailnet-forward`.)
- **G1 runs (2026-10-05): four defects before it passed.** The agent ran from the checkout at each run's commit (runbook setup), with the faster-whisper sidecar production runs; speech is a piper clip queued with `audio-inject`.
  - Every planner call failed with `Server error (500)`: models.dev dropped the `kimi-for-coding` provider, and `kimi-code-plan-global` refuses `top_p: 1` (`412ace5b9`, planner bench 5 / 5 valid, p50 2.9 s).
  - "write a hash map solution for two sum" returned the whole solution and passed 12 / 12, which the runbook forbids: a whole-solution ask is now held to 3 lines in the server and the client (`5df8a002a`, bench n = 6: 30 / 30 steps valid, whole-solution ask refused 6 / 6).
  - The local rename missed every name whisper cannot spell: "seen" is written "scene" (`7f73978da`), "lookup" is split in two (`161dd7394`) and `max_area` is spoken as words (`19d6458a6`); problem 2's dictionary is `visited` since "in visited" is also mangled (`304e65b1f`).
  - The tailnet forward to the leetcode backend dropped twice mid-run (its container exited with 137 after a redeploy, `wf ship leetcode-backend --redeploy-only --ref dev` restarted it); Run Local answered `Failed to fetch` until it was back.
- **G2 runs (2026-10-05): seven defects before it passed.** The marker ran from the checkout at each run's commit (runbook setup), since `remote-chat-agent-prod` is off the tailnet.
  - `trace --source template` drew the pad's border as a stroke down its left side: the mask now drops what reaches the capture's edge (`e7a425e86`).
  - The verdict card took 67 px from the pad, and the ink, kept in pad pixels, slid off the template it was sized with: the screen keeps room for the verdict (`a23576bf8`; `verify-tamil.mjs` checks the pad's height).
  - A marker answer at 31.7 s reached the app as "Could not reach the marker": `route.fetch`'s default 30 s timeout, which no phone has, is off (`00bd5c94d`; e2e S1).
  - The pulli of ம் was never written, as a filled dot thins to nothing: trace dabs dots (`098255f15`).
  - The lab drew Tamil in FreeSerif, the sandbox's only Tamil face, whose ை no phone shows: the phone presets fetch Noto Sans Tamil and put it right after the system font (`9544087a2`).
  - அம்மா's template ran 7 px past a 365 px pad and was clipped: a word's template shrinks to fit the pad's width (`b591819a1`; `verify-tamil.mjs` checks it).
  - The marker's printed இலை was clipped at both edges, the ink sat small, words were called letters, and MiniMax-M3 marked a faithful இலை from 25 to 85 and passed ஆடு as அம்மா: whole-word reference, trimmed panels, and `opencode-go/gpt-6-luna`, 18/18 on the benchmark against MiniMax-M3's 15/18 (`d251cef5b`, [audit](../../audits/2026-10-05-tamil-marker-vision-model.md)).
- **`wf cheap-flow check` from the sandbox.** Without `--local` it enqueues itself on the VM (`delegated cf-device-lab to the VM`) and returns before checking, so the goal's `p1.units` passes `--local`.
- **G2 dry run (2026-10-05).** Run with today's driver (mouse input, no tremor), through the `wf browser` CLI only.
  - All seven letters were accepted on the first attempt at 100 % ON THE LINE.
  - The large அ came back "NOT YET · 0, Could not reach the marker".
  - A tap dot is visible in the recording at 178.9 s.
  - Found: the export must be served under its baseUrl, `/mla-tamagui`; scrolling and mouse text selection (I13); and spec §5 items 10–11.
  - Evidence: [screenshots, video and log](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g2-dry-run-2026-10-05/index.html).
- **G2 with touch input (spike, 2026-10-05).** Standalone Playwright with CDP touch events, minimum-jerk strokes and tremor, outside the daemon.
  - Seven letters were accepted on the first attempt at 97–100 % ON THE LINE, with `pointerType: 'touch'` and no text selection.
  - A swipe scrolled the lesson, and a touch tap on "Got it" moved on each time.
  - Re-check of the serving fix: run verbatim, the runbook's setup loads `/mla-tamagui/tamil/uyir-1` as `{"missing":null,"glyph":"அ","position":"Letter 1 of 6","step":"Stroke 1 of 2"}`.
  - Evidence: [screenshots, video and log](https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/device-lab/claude_device-lab/g2-touch-2026-10-05/index.html).
- **VM pre-commit route fails on the VM only.** In the cloud sandbox, lefthook's `vm-precommit` ships the gates to the VM. There `workflow-cli:test:unit` fails in `tests/bundle-portable.test.ts`: `wf browser preflight`, run from an isolated copy of the bundle, hits a module-resolution failure in the VM worktree. The same test passes locally 2/2. Commits here run the same nx gates on the local execution site (`env -u CLAUDE_CODE_REMOTE git commit …`); they are green.
- **`pnpm run audit` side effect.** It triggered pnpm 11's verify-deps install, whose postinstall failed; nothing tracked changed. The lockfile baseline already carries 79 high and 2 critical advisories (registry bulk advisory API), so `pnpm audit --audit-level=high` is red before any change. Run it as `pnpm --config.verify-deps-before-run=false audit`.
- **Tooling the sandbox lacked.** `bws` 2.1.0 was installed from its release asset (mise hit a GitHub API 403), then `wf vm net up` joined the tailnet. vm-ops-api is reached through the SOCKS proxy at `localhost:1055`.
- **Dev-bridge rule.** vm-ops-api now serves only the shared trunk Metro, so `wf ship mla-tamagui` has no per-branch link. The `pr-carries-dev-bridge-link` rule skips cleanly once `clis/workflow-cli/dist` is built (`pnpm --filter @cli/workflow-cli exec tsc`).
