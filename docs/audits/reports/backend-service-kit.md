# backend/service-kit: what the Rust backends share, on standard crates

Date: 2026-10-09. Plan: `docs/plans/rust-services/standard-crates.md`. Branch `claude/service-kit` (PR #332).

## Scope

New workspace member of the backend cargo workspace, used by `leetcode-backend` now and `remote-chat-agent` next. It replaces the logging, telemetry, rate limit and heartbeat code both services carried by hand (`limiter.rs` identical, `telemetry.rs`, `log.rs`, `curl.rs`, `server.rs` near-identical). Contents:

- `telemetry`: a `tracing` subscriber (JSON lines on the console, errors on stderr) and, when `BETTERSTACK_OTLP_TOKEN` and `BETTERSTACK_OTLP_HOST` name an ingest (and `SECRETS_LOG_BETTERSTACK` is not `false`), `opentelemetry-otlp` spans and logs over HTTP/JSON with the bearer token. The host must be a bare host and optional port. Failure to build the exporter is reported once on stderr and never fails the service.
- `redact`: the one hand-written piece. Fields whose names look like credentials are replaced whole and known secret values (the environment's secret-looking variables plus any the service names) are replaced inside any text, on the console and in the OTLP log layer, which also stamps each record with its request's trace. The standard appender cannot redact, which is why the layer exists.
- `trace`: a tower-http layer that makes one span per request, the child of the caller's W3C `traceparent` when it sent one (an unsampled parent is not exported), named `METHOD path`, with route, method and status.
- `rate_limit`: `tower_governor` token bucket per peer address.
- `heartbeat` and `shutdown`: `health.heartbeat` at boot and every 10 minutes (15 seconds with `DEBUG_MODE=true`); SIGINT/SIGTERM for graceful shutdown.

## Before and after

New crate, so the figures are the services' (see `backend-leetcode-backend.md`): the std-only `curl` exporter and logger are gone, and the standard stack costs about 3.5 MB of binary and 4 MB of resident memory per service. `remote-chat-agent` figures follow in its own report when it moves.

| Measure | Before (per service) | After |
| --- | --- | --- |
| Lines of telemetry, log, limiter, heartbeat code | 490 in each service (leetcode-backend copy counted) | 0 in the service; 579 in `service-kit` (with the trace layer, shutdown and tests), shared |
| Direct dependencies of the crate | n/a | 14 (plus 2 for tests) |

## Tests, by tier

- Flow (the real subscriber and exporters, the ingest replaced by a server on a local port): export with the bearer token and service name, the parent span id carried through, a log stamped with its request's trace, redaction of field values and secret text in what reaches the ingest, an unsampled parent not exported, no ingest configured, host validation (5 tests); the rate limit layer driven in-process (2 tests).
- Unit: 5 (redaction rules, the scrubbing console writer).
- `cargo clippy --all-targets -D warnings` clean. Integration and E2E: through the services that use it (leetcode-backend integration 79 of 79; E2E after deploy).

## Deliberate behaviour changes (library wins)

- Console lines are the `tracing` JSON shape (`timestamp`, `level`, `message`, fields, `target`), not the old `time`, `level`, `event`.
- The rate limit is a `tower_governor` token bucket per peer address (an evenly refilled bucket, the library's 429 body), not the fixed window of the old limiter.

## Architecture deviations

- The OTLP encoding is JSON, as before (`http-json`), not protobuf, so the ingest sees the same wire format.
- The redaction layer is hand-written on purpose (see Known gaps): the one exception to "no hand-rolled code" in this crate.

## Known gaps

- The redacting log layer is hand-written (about 90 lines) because no standard crate redacts `tracing` fields before export.
- None open: the BetterStack sink was queried after the deploys and returns the lines of this path (below, and the decision log).

## 2026-10-09: the rate limiter is keyed by caller

`rate_limit::per_minute_by(max, key)` takes any `tower_governor` key extractor next to the peer-IP default `per_minute`, so a service can limit by a header or a token without writing its own limiter. Both services still use the default. `remote-chat-agent` and `leetcode-backend` now share the one `service-kit` for `tracing` and OTLP export, redaction, the trace layer, heartbeat, shutdown and the limiter; `telemetry.rs`, `log.rs`, `limiter.rs` and the shutdown code exist once.

The BetterStack sink was queried after `remote-chat-agent` was redeployed from the branch (`wf vm logs remote-chat-agent`): it returned the lines its `tracing` path writes for the probes sent, each with its trace and span ids, so the gap above is closed (decision log, "remote-chat-agent on the VM from the pipeline-built image"). The redaction layer remains the one hand-written piece, for the reason stated above.
