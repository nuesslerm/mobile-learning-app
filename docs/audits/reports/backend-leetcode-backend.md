# backend/leetcode-backend: moved onto standard crates, in two layers (core and service)

Date: 2026-10-09. Plan: `docs/plans/rust-services/standard-crates.md`. Decision log: "leetcode-backend on standard crates". Branch `claude/leetcode-backend-standard-crates`, stacked on `claude/service-kit`.

## Scope

The service stays Rust, but the infrastructure it carried by hand is replaced by standard crates: `axum` and `tokio` for the server, `tower-http` for CORS, the body limit and the request trace, `tower_governor` for the rate limit, `reqwest` for the calls to leetcode.com and the capture tool (no more `curl` process per call), `tokio::process` for the code runner, `tracing` and `opentelemetry-otlp` (through the new `backend/service-kit`) for logs and spans, `clap` for configuration, `tempfile` for the atomic session file, `subtle` for constant-time secret checks, `percent-encoding`, `secrecy`. Layout: `backend/leetcode-backend/core` (application layer: use cases, ports as traits, request types, fakes behind a `testing` feature) and `backend/leetcode-backend/rs` (`infrastructure`, `api`, `main.rs`). Removed: `http.rs`, `server.rs`, `curl.rs`, `limiter.rs`, `telemetry.rs`, `log.rs`, `zod.rs`, `app.rs`, `session.rs`, `gateway.rs`, `executor.rs` (2,011 lines) and their tests (the zod golden test and the curl-stub telemetry test); `curl` is no longer installed in the image.

## Before and after (n=5 medians, one fresh process per run, same scripted session; `bench:service`)

| Measure | Before | After |
| --- | --- | --- |
| Binary | 0.63 MB | 4.10 MB |
| Cold start to `/health` | 5.12 ms | 12.10 ms |
| Idle RSS | 2.59 MB | 6.42 MB |
| RSS after 60 health calls | 2.63 MB | 6.49 MB |
| Peak RSS | 3.01 MB | 6.85 MB |
| `GET /health` p50 | 0.46 ms | 0.46 ms |
| `GET /api/leetcode/user-status` p50 (one relay) | 6.66 ms | 1.08 ms |
| `POST /api/leetcode/run` p50 | 33.89 ms | 33.27 ms |
| `POST /api/leetcode/format` p50 | 37.09 ms | 35.42 ms |
| Direct runtime dependencies | 1 (`serde_json`) | 14 in the service, 4 in core |
| Hand-written infrastructure | HTTP server, HTTP client via `curl`, limiter, CORS, OTLP exporter, logger, zod issue builder | none; the redacting log layer lives in `service-kit` |

Image size is not measured here (the image is built by the pipeline after merge); the runtime stage no longer installs `curl`.

## Tests, by tier

- Integration (black box, real binary, real sockets, mock LeetCode and capture tool on local ports): `tests/blackbox`, 79 tests. Expectations for the deliberate changes were edited first: 19 failed against the old binary, then 79 of 79 pass on the new one, no skips.
- Flow (in-process, every dependency replaced in code): core 12 (the real use cases, fakes behind every port), service 15 (the real axum router driven with `tower::oneshot`), `service-kit` 7 (real subscriber and exporters against a local ingest; the rate limit layer).
- Unit: 5 (core and the service crate), 5 (`service-kit`). `cargo clippy --all-targets -D warnings` clean.
- E2E (the real VM, 2026-10-09): the image `sha-1a311f7e` was built and redeployed by `wf deploy run leetcode-backend --ref claude/rca-serde-errors` (exit 0); `/health` reports `1a311f7e2a104411880f3f1ee74e636b5b74aeaa`; from agent-shell `POST /api/leetcode/format` without the session secret is 401 and an oversized body is 413; BetterStack returns `server.listening` and `health.heartbeat` lines from the new container. The authenticated routes were run afterwards, once the approval guard allowed a vault fetch beside other commands (PR #342), from the sandbox through the tailnet proxy (8 of 8, secrets never printed): `POST /api/leetcode/format` with the bearer is 200 and returns the formatted text; `POST /api/leetcode/run` with the bearer runs a Python class and reports its case (`pass` true, `actual` 3, `stdout` `1\n`); `POST /api/leetcode/session` without the admin secret is 401, with it and a body missing `cookie` or with an empty one 422, so no session was written; an unknown path with the admin secret is 404. Details in the decision log.

## Deliberate behaviour changes (library wins for framework-generated responses)

A body that does not fit its request type is 422 with the parser's plain-text message (was 400 with zod-style issues as JSON). A body that is not JSON content is 415. The bearer check runs before the body is read. CORS preflight answers 200 (was 204). The rate limit is a token bucket (120 a minute per peer address, refilled evenly) with the library's 429 text. Routes are exact (`/HEALTH/` is 404). An unknown path outside `/api/leetcode` is an empty 404. Console log lines are tracing JSON (`timestamp`, `level`, `message`, fields, `target`). Two emulations of the TypeScript original were dropped: JavaScript truthiness for `isSignedIn`, and the "Cannot read properties of null" message.

## Architecture deviations

No `domain` layer (the service has no rules beyond its use cases). Request types derive `Deserialize` in the application layer and a `NonEmpty` newtype replaces the validator crate. The capture port is a synchronous fire-and-forget method so core needs no tokio. The `core` crate carries its fakes behind a `testing` feature so the service crate's flow tests can use them.

## Known gaps

- E2E on the VM, the BetterStack sink query and the image size wait for the deploy.
- The `service-kit` OTLP path is covered by flow tests against a local ingest, not by the service's own black-box suite, which sends no telemetry.
- Any new member of the backend workspace still needs its `COPY` line in both service Dockerfiles (see the earlier change below).

## Earlier change (2026-10-08): image build repaired after the backend workspace grew


### Scope

`leetcode-backend` is already a Rust service. Its Docker build copies the backend cargo workspace manifest and only its own crate. When `remote-chat-agent/rs` became a second member of that workspace, cargo refused to load the workspace in the build (it reads every member's manifest and every path dependency even for `-p leetcode-backend-rs`), and the leetcode-backend leg of the Deploy workflow failed on every push to `dev` from that commit on (runs 291 to 297, exit 101 at `cargo build`; run 290 and earlier were green). The Dockerfile now also copies `backend/remote-chat-agent/rs` and `clis/rca-core`; neither is compiled there. No source of the service changed.

### Before and after

| Measure | Before | After |
| --- | --- | --- |
| Deploy leg on `dev` | failed, runs 291 to 297 | passed, run 298 onward; the VM runs `leetcode-backend:sha-effbe7f8`, healthy |
| Runtime numbers (start, memory, latency, image) | 16 ms start, 2.3 MB idle, 266 MB image (recorded for the port) | unchanged: no source or runtime change; not re-measured |

### Tests, by tier

- Runbook: after the fix, the VM reported the leetcode-backend container `Up (healthy)` on the new tag.
- Build: `cargo metadata --locked --offline` on exactly the paths the `COPY` lines name, in an empty directory: exit 0 (it failed with the manifest error before the two lines were added).
- CI: the Deploy workflow's leetcode-backend leg built the image and redeployed it (run 298).

### Deviations

- None.

### Known gaps

- The dependency is structural: any new member of the backend workspace, or any new path dependency of one, needs the same two-line addition. Separate workspaces per service would remove the coupling at the cost of two lockfiles.

## Folder layout in the layers (2026-10-09)

`rs/src/config.rs` moved into `infrastructure/config.rs` (`leetcode_backend::infrastructure::config::Config`). No behaviour change; black box 79 of 79 on the deploy-profile binary, image on the VM at `323b2a3d1`. The layer rules pass with no ignores for this crate.

