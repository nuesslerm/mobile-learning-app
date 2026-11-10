# After device-lab: loose ends, event waits, sandbox-first CI

Owner request, 20 Oct 2026, after `docs/goals/2026-10-14-device-lab-fast-green.md` closed. Four bodies of work: A, the loose ends of that goal; B, the harness's sleeps replaced by event waits (item E of the 19 Oct leftover plan); C, the sandbox-first CI spec (`docs/plans/sandbox-first-ci/spec.md`); D, oxlint with a few rules for the React Native app (owner, 20 Oct, added while A3 ran). Goal: `docs/goals/2026-10-20-after-device-lab.md`. Ledger: `docs/plans/after-device-lab/progress.md`.

## Owner rule (20 Oct)

The Android emulator is never started, stopped or driven by hand. The Android e2e tests run only in CI, in the Android job of a push to `dev`. No `wf android up|down|start|stop|lease` and no device-driving verb by hand, from a sandbox or on the VM; fresh device evidence, and every device measurement, comes from CI runs. The close of the device-lab goal on 20 Oct brought `rdl` up by hand to pass the older goal's a1 and a3, which broke this rule; A0 removes the reason and records the correction. The rule is followed, not guarded: no wf refusal and no test for it.

## Constraints for every item

- Never loosen a gate: scores, zero warnings, recordings and their read-back stay; `initial` baselines are never overwritten.
- Capability in `wf` (never `scripts/`), tests at the tier the change needs, commits through lefthook, landed on `dev`, PR #346 kept current.

## A. Loose ends

**A0. The emulator only through CI.** One line in `.agents/instructions/root.md` (then `pnpm run sync:agent-config`). The older goal's a1 and a3 (`docs/goals/2026-10-13-device-test-process.md`) read the device health the newest CI warm-up published instead of a live device, with the same conditions (`boot.completed`, `boot.activityService`, `restartProven`, `disabledTotal >= 40`): the warm-up (`e2e/warm-app.lua`, run by CI once per Android job) writes `wf android features --json` into its run bundle. The ledger and the decision log record the correction. Done when both goal files pass with no device up.

**A1. Device numbers in the VM samples.** The fix of 19 Oct (`42fe17124`: the device counted through `agent-shell-dind`, an interrupted call retried) is confirmed on the next CI Android run: every G1 to G3 unit's samples carry the device's memory and CPU. If they do not, the container lookup is fixed.

**A2. CI usage rows.** The runner's `wf` finds `WF_USAGE_ORDS_BASE_URL` and `HEALTH_REPORTS_ORDS_PASSWORD` (both in the deploy vault, which the runner's read token reads), so `HEALTH_RPT.WF_USAGE` gets a row for every CI command. Done when no CI log says "usage row not recorded".

**A3. Flow warnings to zero, then a gate.** The flow run of 19 Oct counted 10 warnings: `process.env.EXPO_OS` not inlined in the flow setup, `selectable` reaching the DOM (`frontend/libraries/tamagui-design-system/ui/code-block.tsx`), unknown responder props in ServiceReportScreen, and the WebView bridge's thrown script (kept only if the test means it). Each is fixed at its source; then the flow stage fails on any warning, as the device harnesses do. Done when a flow run reports none and a planted warning fails the stage.

**A4. Stale gaps.** The 63 open `docs/gaps/*.md` are re-checked against the 19 and 20 Oct runs: the ones that no longer hold (for example the Android Tamil gate on the slim device, which passes) are closed and archived, the rest re-pointed, `docs/known-gaps.md` regenerated. Done when every open gap carries a re-check of that date and `wf check --dead-refs` reports 0.

**A5. Device reruns without a hand run.** Two levers, both CI only. (a) The e2e cache key includes the device driver code (`clis/wf/src/device/`, the Android and browser drivers, `clis/wf/src/run.rs`), so a harness change reruns the device tests; this reverses part of the 18 Oct lean keys. (b) A commit trailer (`E2E-Fresh: android`, or `browser`) makes CI run that stage fresh, which gives B and C their n≥3 measurements. Done when flow tests of `wf test plan` and `wf test run` prove both and one CI push shows a fresh Android stage.

**A6. The sampler's interrupted call.** The first `/stats` call of nearly every unit on 19 Oct failed with EINTR (os error 4). The signal is found (reproduced under a flow-tested Android run) and fixed at its source; the retry stays only if the cause cannot be removed. Done when the newest Android units have no failed sample.

## D. Lint: oxlint, only the most useful rules

Owner, 20 Oct: oxlint replaces the React Native app's ESLint setup; the rules that want a log in every callback and a testID on every component go; the rest is cut to the most useful ones.

**D0. Keep list.** A rule stays only when it catches a defect that the typecheck, the tests and the reviews do not, mostly one that shows only on a device: oxlint's correctness category (unused variables among it), `react/rules-of-hooks`, `no-console` (the Logger is the only sink), the import bans with their reasons (the `tamagui` and `lucide-react-native` barrels, the react-native APIs react-native-web fakes, the two Reanimated scroll hooks that never fire on iOS), `vi.mock` banned in flow tests; and from `@lib/eslint-plugin-mobile-learn`, run as an oxlint JS plugin: `no-hermes-unsupported-api`, `no-styled-native-host`, `no-unguarded-web-global` and `no-unverified-dynamic-import` (the app's own code), `no-unhandled-catch`, `require-structured-log`, `no-direct-io-in-feature-ui`, and `thin-route` (new: oxlint has no `no-restricted-syntax`, which carried the route-file shape). The feature packages' layer boundaries move from `eslint-plugin-boundaries` to dependency-cruiser (`frontend/feature-kit/depcruise.feature.cjs`), run by each package's lint. Dropped: `require-log-in-unit`, `require-testid-on-element`, `require-testid-on-interactive`, the testID and prop format rules, the other logger-shape and style rules, `exhaustive-deps` and `given-when-then-title` (warnings nobody acted on: 5 and 228 open), the React Compiler checks (the app does not run the compiler), and the promise rules (`no-floating-promises`, `no-misused-promises`, `await-thenable`), which returned in D4. The plugin keeps only the kept rules and the ones the backend and tooling ESLint still uses.

**D1. oxlint.** One `.oxlintrc.json` for the app and its feature packages (pinned oxlint, vetted per the dependency rule); each package's `lint` script runs it, so Nx, CI's lint stage and the pre-commit gate pick it up unchanged; ESLint, its configs (`frontend/mla-tamagui/eslint.config.mjs`, `frontend/feature-kit/eslint.feature.mjs`) and their dependencies leave these packages. Docs that say "ESLint rejects it" name oxlint.

**D3. No ESLint left.** The packages still on ESLint held about 60 TypeScript files after the move to Rust, and their configs mostly guarded files that no longer exist. A root `.oxlintrc.json` lints everything outside `frontend/` (correctness, unused variables, `guarded-detached-spawn`, the test-quality rules); ESLint, its configs, `@lib/eslint-config-node-ts` and every ESLint package go. Done when every package lints with oxlint and nothing in the repository installs ESLint except the plugin's own rule tests.

**D4. The promise rules return.** oxlint's type-aware mode (oxlint-tsgolint, typescript-go inside) runs on TypeScript 5.9 once no tsconfig sets `baseUrl`, which typescript-go no longer reads; the eight package tsconfigs and the root one drop it, the root's `paths` targets become `./`-relative. `no-floating-promises` (with node:test's `describe`/`it`/`test` allowed: its runner awaits them), `no-misused-promises` and `await-thenable` are errors in both configs, and their 10 findings are fixed at the source. The other 13 type-aware rules in oxlint's correctness category went through the same review as the plugin's (D4b) and stay off: their 66 findings across the repository were all correct code (`unknown` narrowed to `{}`, labels inferred as unions in plain `.mjs`, sort calls on strings), and the two with no findings guard against patterns tsc already rejects or the codebase never writes. One React for the workspace: an override pins `react` to 19.2.3, the version Expo SDK 57's renderer runs, because auto-installed peers had resolved 19.2.7 and the hoisted react-native flipped to that copy between installs. With one copy the app's typed routes reach `@lib/app-config`, which showed its route allowlist naming four routes the app does not have; they are gone.

**D4b. Only the useful rules, on oxlint alone.** Owner, 20 Oct: port only the useful rules. Each of the plugin's 14 rules went through a finder (files in scope, violations with its overrides lifted, history, overlap with the typecheck, built-ins, tests and guards) and a skeptic arguing to drop it. Two stay: `no-hermes-unsupported-api` (APIs that typecheck and pass every browser check but crash on Hermes, as `AbortSignal.timeout` did on a phone in August; no longer applied to tests, which never run on Hermes) and `no-unhandled-catch` (a swallowed error is a silent failure). Twelve go: `no-styled-native-host` (its premise was a misdiagnosis, and it never fired), `no-unguarded-web-global` and `no-unverified-dynamic-import` (nothing caught, and the latter only ever matched a web-only route), `require-structured-log` (never fired), `no-direct-io-in-feature-ui` (a layering convention dependency-cruiser's `ui-never-imports-data` already enforces), `thin-route` (a convention, nothing caught), `guarded-detached-spawn` (nothing in its scope since the spawning code moved to Rust), and the five test-shape rules. The two rules are plain rule objects and their tests run on oxlint's own RuleTester, so typescript-eslint and ESLint leave the repository.

**D5. TypeScript 7 is the compiler.** `tsc` is TypeScript 7.0.2 (typescript-go) as `@typescript/native` in every package that runs it. 7.0 ships no JavaScript API, and Nx's project graph, dependency-cruiser and Tamagui's babel and metro plugins `require('typescript')`, so `typescript` is the 6.0 API package (`npm:@typescript/typescript6`) until 7.1 ships an API they use. The config changes 6.0 and 7.0 need: `moduleResolution` Bundler for the two CommonJS builds (logging's babel plugin, the lint plugin), `vite/client` types for the browser harness, a `scrollMargin` on the flow harness's IntersectionObserver stub, two annotations 7.0's checker asks for in `glyphGeometry.ts`. dependency-cruiser refuses to run when `typescript` resolves to 7 or above, since it would cruise 0 modules and pass. The editor's TypeScript server is the 6.0 one the alias pulls in.

**D2. Measured.** The app's lint wall time before and after (n=3, cold cache), logged in the decision log. Done when every app and feature package lints with oxlint and no ESLint, the kept rules each fire on a planted case, and the numbers are logged.

## B. Harness sleeps become event waits

**B0. Baseline.** The per-unit timing of the newest CI runs on both platforms is committed as `initial` and never overwritten.

**B1. Wait verbs** for what the scripts poll for today: the soft keyboard hidden or shown (`e2e/editor-typing.lua`), the editor buffer grown (`device:wait_editor_longer` exists), the pad drawn (`editor-typing`, `voice-edit`), a toast gone (`device:wait_gone` exists), the recorder started (`voice-edit`, instead of the fixed sleeps around a clip; the clip's own real-time length stays). Each new verb has flow tests on both drivers.

**B2. Scripts.** The polling loops and fixed sleeps in `e2e/editor-typing.lua`, `e2e/voice-edit.lua` and `e2e/tamil-writing.lua` become those waits, with the same assertions, zero warnings, recordings and goldens.

**B3. Measured in CI.** n≥3 runs per platform over successive CI runs (A5b), each unit's median against B0; a change stays only when it is faster with no new flakes; the numbers go into `docs/audits/2026-09-02-token-diet-decision-log.md` and the `current` baseline is ratcheted. Done when no fixed sleep is left but real-time media and every unit's median is at or below B0 on both platforms.

## C. Sandbox-first CI

The phases of `docs/plans/sandbox-first-ci/spec.md`, in order; every device measurement runs in CI.

| Phase | What | Done when |
| --- | --- | --- |
| S0 | Benchmark: `eas update`'s export, upload and serve on the runner; the first load in Expo Go from an update and from the dev Metro, and one unit's wall time against each, as a CI e2e benchmark test (n=3 through A5b) | the numbers and the decision are logged |
| S1 | A global JS error and rejection handler logs `app.crash` through `Logger`; the features whose flows log nothing diagnosable get logs | a flow test covers the handler and a BetterStack query returns the line a test caused |
| S2 | An `eas-update` job before Android, CI only, when the app's package graph changed; source maps to the bucket; an `https://` link in the summary | an app change publishes and a non-app push does not |
| S3 | Android against the update (if S0 says so): the run id through the launch URL, BetterStack lines and mapped stacks in the reports, the Android debugger removed, the root rule on debuggers updated | G1 to G3 pass on Android against an update in CI, each with its log lines |
| S4 | A remote Nx cache in vm-ops-api; the sandbox writes from clean, pushed trees; runtime and platform versions in the inputs | a sandbox flow pass replays on CI at the same commit |
| S5 | The pre-commit gate through Nx; CI replays the lint, typecheck, flow and browser runs the sandbox did and runs the rest; a drift report and a contract test | VM CPU-minutes per push measured lower, the replay rate on a clean push 100%, no gate loosened |

## Order

A0 first, then A2 and A3, then D (asked for during A3), then A4 to A6 (A1 rides the next CI Android run), then B, then C from S0 to S5. Device evidence comes from CI only; flow and browser work runs in the sandbox.
