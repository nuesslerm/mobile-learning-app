# backend/leetcode-auth-cookie-capture: the dependency stages of its image

Date: 2026-10-09. Plan: `docs/plans/rust-services/standard-crates.md` (done-when 4; the speed-up the owner asked for after it). Branch `claude/wf-global-options`, landed on `dev`. Tier that ran: e2e (the pipeline's Deploy workflow on the real VM runner); no unit or integration tier, because no source of the service changed.

## Scope

One file changed: `Dockerfile`. The `libs` and `proddeps` stages copied the whole `scripts/` directory for the workspace root's postinstall hook, which does nothing under `DOCKER_BUILD=true`; any change under `scripts/` (the approve guard, for one) invalidated the layer that runs `pnpm deploy` (84 s on a miss). Both stages now copy `scripts/postinstall.mjs` and `scripts/prepare.mjs`, the two files pnpm's lifecycle hooks name. The `build` stage still copies the whole tree. The service itself, its entrypoint and its compose file are unchanged (a comment line added to the entrypoint and a comment added to `metro.config.js` to time a deploy were reverted in the same series).

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| `deploy (leetcode-auth-cookie-capture)` when all four deploy targets change (run 317, then run 37966325120) | about 2 min 40 s | 54 s |
| The same job when only this Dockerfile changes (run 37965945128, the layer rebuilt) | n/a | 67 s |
| Whole Deploy, all four targets changed | 7 min 06 s | 3 min 17 s (with the Rust `deploy` profile, see `workspace-root-and-infra.md`) |

One run per case (the cost of a run is minutes of the one runner; the question was whether the five-minute limit is met). The saving shows on a push that changes `scripts/` and nothing else in the image's inputs; that case was not made up for the measurement, since the layer key is the content of the two named files.

## Tests, by tier

- E2E: the three Deploy runs above, each green through `plan`, the leg and the redeploy; the capture container runs `leetcode-auth-cookie-capture:sha-846130f8`, healthy (`wf deploy status leetcode-auth-cookie-capture`).

## Deliberate behaviour changes

- None. The image's contents are the same files.

## Architecture deviations

- None.

## Known gaps

- The Node service's own tests were not rerun: nothing in it changed.

## Port to Rust (2026-10-10, branch `claude/ts-removal`)

The Node service is replaced by a Rust pair on the `leetcode-backend` layout: `core` (domain: config, cookies, sign-in, outcome; application: the capture use case and its ports `Backend`, `Notifier`, `BrowserLauncher`/`BrowserSession`, `DisplayStacks`, `Clock`) and `rs` (api: axum `POST /trigger`; infrastructure: reqwest backend and notifier, the `wf browser` daemon client, the Xvfb/x11vnc/websockify children, clap config). Telemetry, heartbeat, shutdown and rate limiting come from `service-kit`; the secret loader moved there from `remote-chat-agent`.

Tests first: a black-box suite (`tests/blackbox`, real process, mock backend, notifier and browser daemon, stub display tools) ran green 20 of 20 against the Node service and 20 of 20 against the Rust binary. Flow tests with in-process fakes: 9 (`core/tests/flow_capture.rs`). The Node unit and integration tests were deleted with the code they covered.

Benchmark (`docs/audits/benchmarks/capture-listener-bench.py`, record `capture-listener-node-vs-rust.json`): start to listening 1,022.7 ms to 14.2 ms; idle RSS 137,788 KB to 6,572 KB; refused request p50 0.659 ms to 0.520 ms; Rust release binary 4,191,112 B.

### Deliberate behaviour changes

- The trigger rate limit is a token bucket (`service_kit::rate_limit::per_minute_json(30)`), not a fixed window.
- `--once` runs one capture and exits (replaces `pnpm run capture:local`); `--healthcheck` is the container probe.
- The display teardown waits for each child after SIGTERM instead of firing and forgetting.
- The secret comparison is constant time.
- Telemetry goes to BetterStack through the OpenTelemetry exporter like the other backends.

### Architecture deviations

- None; `pnpm run arch:rust` passes without ignores for this service.

### Known gaps

- The binary is built on bookworm and runs on the Playwright jammy runtime image; glibc compatibility is confirmed only by the pipeline build and the VM check.
- The Docker build was not run locally.
