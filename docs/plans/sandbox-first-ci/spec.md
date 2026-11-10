# Sandbox-first verification, an EAS Update before Android, and logs instead of breakpoints on the device

Owner requests, 19 Oct 2026. To be built after the open device-lab work (`docs/goals/2026-10-14-device-lab-fast-green.md`) is done.

## Why

The VM (one Always-Free A1, 2 OCPU / 12 GB) runs every service, the Android device and the CI runner. Every minute CI spends there on something the sandbox could have done is a minute the services and the device do not get. The owner's workflow:

- **Flow tests are the broad base.** They prove the business logic and the interactions of a whole main process, with external dependencies doubled. They run in the sandbox, as often as a change needs.
- **Browser e2e tests are fewer and prove a feature end to end.** They also run in the sandbox (`wf browser run` against a local `expo export --platform web`, or against deployed services over the tailnet).
- **A push to `dev` is for what only the VM can do:** build and deploy the backend services (to test against them), run the Android e2e tests on the emulator, and publish the app to Expo Go. Android e2e has roughly the browser tests' scope, and it matters because it is the only proof the feature works on a native device.
- **Lint and typecheck run in the sandbox and replay on the VM.** The sandbox's pre-commit gate runs them through the Nx cache; CI sees the cached green and skips them, and still runs them when the sandbox did not (owner, 19 Oct: "still run it if we fucked it up"). Nothing may let the two drift apart over time, and the pre-commit gate itself runs through Nx.
- **The sandbox runs exactly what CI runs**, through the same commands, so the Nx cache (and, where it applies, the Docker build cache) serves both.
- **Every push that changes the app publishes an EAS Update**, so the owner's phone always has the latest stable app in Expo Go. The publish comes before the Android tests, and the Android tests run against that same update.
- **On the device, debugging is logs, not breakpoints.** BetterStack crash reports and structured logs, with stacks mapped to source lines, replace the Android debugger; breakpoints stay in the browser harness.

## Where the minutes go today

CI run `39234982117` at `5fa8024a7` (19 Oct), one job after another on the VM's single runner:

| Job | Minutes | Notes |
| --- | --- | --- |
| plan | 0.4 | `wf test plan`, `wf ship --changed` |
| flow-tests | 6.8 | the step itself 6.5 (cold run on 2 OCPU, vitest on one worker) |
| typecheck-lint-unit-tests | 0.6 | mostly Nx replays on this push |
| clis | 0.1 | the wf build was already current |
| deploy | 0 | nothing changed |
| deployment-verification-tests | 8.6 | failed: the old `wf` shim compiled itself inside `pnpm run` (fixed on `claude/device-lab`) |
| browser-e2e-tests | 3.7 | 3.0 running the tests |
| android-e2e-tests | 5.3 | device up 1.0, tests 3.1, device down 0.6 |

The run also waited 6 minutes in the queue behind the previous one (`cancel-in-progress: false`).

What the sandbox can take over: flow (6.8), typecheck and lint (0.6 here, 3.5 on a cold cache), browser e2e (3.7). That is 11 of the 26 minutes on this run, and more on a cold one; with a shared cache CI keeps a replay of each (a checkout, an install and the Nx lookup, well under a minute per stage) instead of the work.

## Target

### Who runs what

| Test type | Sandbox | VM CI on push to `dev` |
| --- | --- | --- |
| Lint, typecheck | pre-commit (`wf check --staged`, through Nx) | replayed from the shared Nx cache; a miss runs |
| Unit, flow | pre-commit runs the affected ones; `wf test run --stage flow` for the full set | replayed from the shared Nx cache; a miss runs |
| Browser e2e | `wf browser run` against a local web export (or `wf test run --stage browser`) | replayed from the shared Nx cache; a miss runs |
| Deployment verification | against deployed services over the tailnet | after each deploy, as now |
| EAS Update | never (CI only, owner) | after the deploys, when the app's package graph changed |
| Android e2e | never | against the EAS Update just published |

A lint, typecheck, flow or browser stage on CI costs a checkout, an install and the Nx replay when the sandbox already ran it. It still runs anything the sandbox did not, so a push that skipped the sandbox, or a sandbox that got it wrong, is still verified on the VM.

### One cache for both environments (nx-cache plan N2, amended)

`docs/plans/nx-cache/plan.md` N2 puts a remote Nx cache in blob storage: a loopback shim inside `wf` speaks the Nx self-hosted cache API (`GET`/`PUT /v1/cache/{hash}`) and stores to an OCI bucket, so the sandbox and CI share one cache and nothing durable lives on the VM (`docs/plans/blob-cache/plan.md`; an in-VM listener in vm-ops-api was built and removed on 23 Oct). N2 as first written lets only CI write. This spec changes that, so a sandbox pass serves CI, and the blob-cache plan adopted it (one write guard, `write_allowed`, for the sandbox, the pre-commit gate and CI):

1. **Writes from the sandbox are allowed only from a clean tree.** `wf test run` writes an entry only when every input of the task is committed (`git status --porcelain` is empty for the project's inputs) and the commit is on the remote. A dirty tree reads, never writes.
2. **The hash must mean the same thing in both places.** The task inputs gain the runtimes that can change a result without a file changing: the Node, pnpm and Rust versions (`mise.toml` pins them, so hash the resolved versions; Node is 22.22.0 and pnpm 11.5.2 in `mise.toml` and in every `setup-node`), the Chromium build for browser tests, and the platform (`linux-arm64` on the VM; the sandbox is `linux-x64`). Entries do not cross platforms unless a task is declared platform-independent (lint, typecheck, flow tests of pure code). Which tasks qualify is decided by experiment: run each stage in both places at the same commit and compare the outputs.
3. **A cached pass carries its evidence.** The entry stores the run bundle (report, screenshots, recordings) next to the terminal output, so a replayed browser stage on CI still publishes its bundle under `verification/test-runs/` and the dashboards see it.
4. **No drift.** The sandbox and CI must keep computing the same hash for the same inputs, or every replay silently becomes a rerun and the VM is back to doing the work. Three guards:
   - CI writes each task's hash and whether it replayed into the run bundle; `wf test results` reports the replay rate of tasks whose hash a sandbox run had already written. A push where a sandbox-written hash did not replay on CI is a drift finding, named with the task and the differing input.
   - A contract test in `clis/wf/tests/flow/nx_cache.rs` computes the hashes of a fixed fixture workspace under two environments (the sandbox's and the runner's settings: PATH, HOME, platform, tool versions) and fails when an input that should not matter changes a hash, or one that should does not.
   - The runtimes are pinned in one place (`mise.toml`) and both environments install from it; `wf check --staged` refuses to write to the shared cache when the local tool versions differ from the pins, so a stale sandbox reads but never seeds. `wf test drift --pins` reports the installed node and pnpm and every `node-version:` of a workflow against the pins, and the weekly audit runs it.
5. **The pre-commit gate runs through Nx.** `wf check --staged` hands lint, typecheck and the affected unit and flow tests to `nx run-many` for the projects the staged files touch, with the local cache and the shared one, so a recommit after a small fix replays everything it did not touch. Benchmarked before and after (median of 3 on the same staged change: a one-file edit in the app, one in a Rust crate, one in a doc).
6. **Docker.** The sandbox has no Docker (the guards block a local daemon), so image builds stay on the VM, and their layers are cached in Docker Hub (`<image>:buildcache`, `docs/plans/blob-cache/plan.md`, "Docker layers via Docker Hub"), not in a VM volume and not in the OCI bucket: a recreated VM imports them.

### An EAS Update before the Android tests

Order on CI: `plan` → `clis` → `deploy (<service>)` → `deployment-verification-tests` → **`eas-update`** → `android-e2e-tests`. The browser stage drops out of the VM line (it replays or runs before the deploys, as now).

- **When:** the app's package graph changed since the commit the last update was published from (`wf ship eas` already keeps that commit). A push that changes nothing in the app publishes nothing, and Android runs against the last update.
- **Where it goes:** EAS branch `dev`, on the channel the owner's Expo Go opens. The job's summary links the update (the `https://` expo.dev update link, per the links rule) and names its group id and commit.
- **What Android runs against:** the update's URL instead of the shared dev Metro. The harness opens it in Expo Go with `--initial-route` per test, as it opens the Metro URL today.
- **What it is built with:** a production export with source maps (`expo export --source-maps`), kept with the run (the maps go to the verification bucket under the update's group id, not to the device). These are the "debug symbols": every stack from the device maps back to `file:line`.

### Benchmark first

Nothing above replaces the Metro path until the numbers say so. Each measurement n=3, median and spread, logged in `docs/audits/2026-09-02-token-diet-decision-log.md`:

1. `eas update` split into its phases on the runner: the export (on the VM's 2 OCPU), the upload (network), and the time until EAS serves the update. For reference, `wf ship eas` from the sandbox took 134 s end to end on 18 Oct.
2. The app's first load in Expo Go on the emulator: from the update vs from the shared dev Metro (a warm Metro and a cold one).
3. One Android unit's wall time against each (editor-typing two-sum part 2 is the reference: 22.8 s against Metro).
4. The VM's CPU-minutes per push before and after (`wf vm` metrics for the runner container).

Decision rule: Android moves to the update when publish plus first load is no slower than a Metro deploy plus its first bundle, and no Android unit gets slower or flakier. Either way the update is published on every app change, because that is the owner's way of getting the app on the phone. If the update path loses, Android keeps Metro and the publish runs after the Android tests instead, so a failing test never ships.

Open question for the benchmark: whether Expo Go runs an update exported in development mode (`--dev`, unminified, `__DEV__` warnings on). If it does, compare its cost; the device-lab gates require zero warnings, and a production bundle cannot show React's development warnings.

### Logs instead of breakpoints on the device

The browser harness keeps breakpoints, logpoints and stacks at source lines (`wf browser debug-*`). The Android harness drops its Metro-attached debugger, because an EAS Update has no Metro to attach through, and gains:

1. **Crash reporting in the app.** A global handler for uncaught JavaScript errors (`ErrorUtils.setGlobalHandler`) and for unhandled promise rejections, installed once at start-up in the app shell. It logs `app.crash` through `Logger.error` (static event name, the message, the raw stack, the update group id and commit, the route, the harness run id when one is set) and then lets React Native's own handler run. The logs already forward to BetterStack; a test proves the line arrives at the sink (the logging rule: a query at the sink returns the line you caused).
2. **Functioning logs.** Every feature's key events log through `Logger` with fields, as the logging rule already requires. The audit before this work lists the features whose flows log nothing a failing Android test could be diagnosed from, and fills those in.
3. **Stacks at source lines.** A `wf` verb maps a logged stack through the update's source maps (fetched from the verification bucket by group id). The Android harness calls it for every `app.crash` and every error line of its run.
4. **The Android report carries them.** After each unit the harness queries BetterStack for the lines carrying its run id (the run id goes in through the launch URL), and the report shows them beside the screenshots and recordings: crashes first, with their mapped stacks, then errors and warnings. Native crashes (Expo Go itself, Hermes fatal errors) come from `adb logcat`, which the harness already reads.
5. **The rule changes with it.** CLAUDE.md's "Every harness has source maps and a debugger" becomes: the flow and browser harnesses have breakpoints and logpoints; the Android harness has crash reports and logs with stacks at source lines. Edited in `.agents/instructions/root.md`, then `pnpm run sync:agent-config`.

## Phases

| Phase | What | Done when |
| --- | --- | --- |
| S0 | Benchmark (above), Metro path untouched | numbers and the decision logged |
| S1 | Crash handler and the logging audit in the app; flow test for the handler; a BetterStack query returns a crash the test caused | the line is at the sink |
| S2 | `eas-update` job in `deploy.yml` (CI only), source maps to the bucket, summary link | an app change publishes; a non-app push does not |
| S3 | Android against the update (if S0 says so); run id through the launch URL; report with BetterStack lines and mapped stacks; debugger removed from the Android harness | G1 to G3 Android pass against an update, each report showing its log lines |
| S4 | Remote Nx cache with sandbox writes from clean trees (N2 amended); runtime and platform inputs | a sandbox flow pass at a commit replays on CI at that commit |
| S5 | Pre-commit through Nx; CI's lint, typecheck, flow and browser stages replay what the sandbox ran and run the rest; the drift report and contract test | VM CPU-minutes per push measured lower, the replay rate of sandbox-run tasks reported and at 100% on a clean push, nothing loosened |

Each phase lands on `dev` through the usual gate, with its benchmark or test at the tier it needs.

## Risks

- **A cache entry from a different environment hides a failure.** Mitigated by the clean-tree rule, the runtime and platform inputs, and the side-by-side comparison before a task is declared platform-independent.
- **A production bundle hides development-only warnings.** The browser harness and the flow tests still run development bundles; the device-lab zero-warnings gate reads those. Android's gate becomes zero `app.crash` and zero error lines at the sink.
- **EAS limits.** Updates count against the account's EAS Update quota; publishing only on app changes keeps it low. The benchmark records the quota use per push.
- **BetterStack ingest delay** makes a report wait for its lines. The harness waits for a bounded time (measured in S1) and says when lines may still be missing, rather than reporting none.

## Note, 1 Nov 2026: what the hexagonal rings add

The backend `core` crates are now checked pure (no I/O crate, no tokio `fs`/`net`/`process`/`signal`/`io-util`; `wf check --budget`, `backend-core-io-violations`), and flow tests double every port. That is what lets unit and flow stages run in the sandbox with no VM, network or service behind them; the ring baseline (`docs/audits/2026-09-02-token-diet-decision-log.md`, 1 Nov) found 81 % of CI stage seconds in pushes that touch `clis/`, so the move above, not ring-scoped inputs, is the large lever.
