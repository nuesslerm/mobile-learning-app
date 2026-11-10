# infra/oracle-cloud/watchdog: ported to Rust

Date: 2026-10-10. Plan: `docs/plans/ts-removal/plan.md` (phase 5).

## Scope

The 199-line Node daemon (`watchdog.mjs`) that recreates unhealthy or restart-looping compose containers is replaced by a Rust pair, `backend/watchdog/core` (domain: snapshots, the decision with its window, loop limit and cooldown, the recreate argv; application: the sweep and the loop, with the ports `Docker`, `Events` and `Clock`) and `backend/watchdog/rs` (the `docker` CLI adapter, the `tracing` event sink, config). It still shells out to the `docker` CLI and the compose plugin, which is the point of the daemon and of its alpine runtime image (the binary is built for musl). Telemetry comes from `service-kit`. The Dockerfile now builds from the repository root.

## Tests, by tier

- Integration (black box: the daemon as a process beside a stub `docker`, a local TLS ingest): 19 tests, written first and green on the Node daemon, then 19 of 19 on the Rust binary (`WATCHDOG_CMD` swaps the implementation). The Node interval became overridable by environment first.
- Flow (in-process fakes): 6 in `core/tests/flow_sweep.rs`; unit: 11 (the decision table, the argv, the inspect parser).
- `wf` black box unchanged; the image list entry for `watchdog` now has the repository root as its build context.

## Benchmark (n=5 rounds of 5 runs, interleaved; `docs/audits/benchmarks/watchdog-node-vs-rust.json`)

| | Node | Rust |
| --- | --- | --- |
| process start to first `docker ps` | 44.6 ms | 4.8 ms |
| idle RSS | 46,476 KB | 5,320 KB |

Release binary 5.3 MB (glibc build; the image carries the musl build).

## Deliberate behaviour changes

- Console lines are `service-kit`'s JSON (`timestamp`, upper-case `level`, `target`, then the fields) instead of the daemon's own flat line; the black box reads both.
- OTLP records carry typed attributes (`ok` is a boolean) and the SDK's resource attributes beside `service.name`; the Node daemon sent every attribute as a string.
- SIGTERM stops the loop and flushes telemetry before exit; the Node daemon died without flushing.
- A `docker` call that runs longer than 300 s is killed and reported as a failed sweep or recreate (the Node timeout did the same through `spawn`).

## Architecture deviations

- None. `pnpm run arch:rust` passes without ignores.

## Known gaps

- `vm-ops-api`'s trunk build table (`src/scripts/trunkBuild.ts`) names the new build context; vm-ops-api is deployed by hand, so the change reaches the running trunk deployer with its next hand deploy.
