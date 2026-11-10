# backend/reporting-service: ported to Rust

Date: 2026-10-10. Plan: `docs/plans/ts-removal/plan.md` (phase 2). Earlier change (2026-10-09): the pricing file is read from `clis/rca-core/data/model-pricing.json`.

## Scope

The Node service (2,277 lines with its tests) is replaced by a Rust pair on the `leetcode-backend` layout. `core` holds the domain (samples and series points, container and host figures, headroom, plan and run reports) and the application (the three sweeps, the scheduler, the report use cases) with the ports `MetricsStore`, `DockerEngine`, `HostReaders`, `PlanSource` and `Clock`. `rs` holds the edges: an axum fallback handler for the read API, `rusqlite` (SQLite compiled in) for the store, `reqwest` over the Docker Unix socket, `/proc` and `statvfs` for the host, and the repository readers. The SQLite file format is unchanged, so a redeploy keeps the history on the `reporting-data` volume. Telemetry, heartbeat and shutdown come from `service-kit`.

## Tests, by tier

- Integration (black box, the service as a process on a real port, a mock Docker Engine on a Unix socket, a fixture repository root, a SQLite file the harness seeds): 39 tests, written first and green on the Node service, then 39 of 39 on the Rust binary (`REPORTING_CMD` swaps the implementation). The Node scheduler intervals became overridable by environment first, so a run does not wait a minute for a sweep.
- Flow (in-process fakes): 12 in `core/tests/flow_reporting.rs` (sweeps, absent containers, partial `/proc` failures, scheduler cadence and stop, series window, headroom, plan join), 3 unit tests in `core`.
- Adapter tests: 4 SQLite (upsert, both rollup passes, reopen), 3 API (health, not found, window refusal).
- `wf` black box: the deploy plan now plans a `reporting-service` leg and the VM-buildable image list names it (tests changed first, 1,153 of 1,153 CLI integration tests).

## Benchmark (n=5 rounds of 5 runs, interleaved; `docs/audits/benchmarks/reporting-service-node-vs-rust.json`, script beside it)

| | Node | Rust |
| --- | --- | --- |
| start to answering `/health` | 597.7 ms | 18.4 ms |
| idle RSS | 114,924 KB | 10,212 KB |
| `/health` p50 | 0.566 ms | 0.783 ms |
| `/api/metrics/series?window=24h` p50 (20,000 stored rows) | 95.9 ms | 37.5 ms |

`/health` is 0.2 ms slower: the row count runs on the blocking pool and the request passes the trace layer. Release binary 7.4 MB.

## Deliberate behaviour changes

- A store failure answers `500 {"error":"internal"}` and is logged; in Node it was an uncaught exception that ended the process.
- Run timestamps are read as RFC 3339 (what `wf` writes); other date forms land on the `unknown` day instead of being parsed by JavaScript's looser `Date`.
- The Docker calls have a 30 s timeout (none before).
- The host disk figure is computed from `statvfs` directly instead of rendering and re-parsing a `df` line.
- A malformed percent escape in a slug is decoded lossily and answers 404 instead of throwing.
- Telemetry reaches BetterStack through the OpenTelemetry exporter like the other backends (the pair is delivered by `sync-env --for reporting-service`).

## Architecture deviations

- None; `pnpm run arch:rust` passes without ignores for this service. The package keeps the name `@backend/reporting-service` and its TypeScript is the black-box suite only.

## Known gaps

- `vm-ops-api`'s trunk build list (`src/scripts/trunkBuild.ts`) does not name this image; it is deployed through the pipeline leg and `wf deploy run`.

## On the VM (2026-10-10)

`wf deploy run reporting-service --ref claude/ts-removal` built `sha-ec31f4b9` on the VM and redeployed it: `Up (healthy)`, `/health` over the tailnet `{"status":"ok","uptime":65,"rows":71}`, `/api/metrics/current` returns container samples (agent-shell: cpu, memory, memory_limit), `/api/headroom` answers, and `wf vm logs reporting-service` shows `reporting.server.listening` and the heartbeat (`rssMb=8`). This closes the gap "the reporting-service image has never been built" (the gap file is removed). Tier: runbook-style probe against the deployed service; the pipeline leg itself runs on the first push of this branch to `dev`.
