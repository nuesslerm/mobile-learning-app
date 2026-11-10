# Backend flow tests: reporting-service and watchdog (17 Oct)

Owner decision: each service's unit, flow and black-box tests become a handful of Rust flow tests in one test binary per
service, driving the service in-process through the entry point `main` builds, every outside dependency a double injected at
construction. Why: CI compile time (one test binary in the default test profile instead of one per `#[cfg(test)]` module and
per `tests/*.rs`, each built in the deploy profile) and tests that go through what callers hit.

## What changed in production code (behaviour unchanged)

- reporting-service: `api::wiring::{App, Ports}` is the composition root. `main` fills `Ports` (store, Docker engine, host
  readers, plan source, clock, commit) with the real adapters; `App::router()` is the router it serves and
  `App::start_sampling(Config::intervals())` the scheduler it starts. `/health`'s `commit` now comes from `Ports::commit`
  (`main` reads `GIT_COMMIT` once at start; `unknown` when unset, as before).
- watchdog: `infrastructure::wiring::{watchdog, Ports}` builds the `Watchdog` `main` runs. The `docker` binary is a port
  (`DockerCommand`, real: `DockerOnPath`); `DockerCli` keeps the argv, the parsing and the 500-character stderr tail over it.
- Gone: both `core` crates' `testing` feature and fakes, every `#[cfg(test)]` module, every old `tests/*.rs`, both TypeScript
  black-box suites with their vitest config, tsconfig, eslint config and TS dev/peer dependencies, and the `build`, `test`,
  `test:unit`, `test:blackbox`, `typecheck` and `lint` scripts (they existed for the TS suites). `test:flow` replaces them.

## Tests before and after

| | before | after |
| --- | --- | --- |
| reporting-service Rust | 22 (3 unit in `core/src/domain/host.rs`, 12 in `core/tests/flow_reporting.rs`, 3 in `rs/tests/api.rs`, 4 in `rs/tests/sqlite_store.rs`) | 2 in `rs/tests/flow.rs` |
| reporting-service TS black box | 39 (`api` 15, `plans` 13, `sampling` 11) | 0 |
| watchdog Rust | 17 (8 in `decide.rs`, 3 in `snapshot.rs`, 6 in `core/tests/flow_sweep.rs`) | 2 in `backend/watchdog/rs/tests/flow.rs` |
| watchdog TS black box | 19 | 0 |

Flows:

- **R1** `given_a_running_service_when_a_caller_reads_it_empty_then_with_history_then_with_plans_then_every_report_answers_as_promised`
- **R2** `given_containers_a_host_and_old_history_when_the_service_samples_stops_and_restarts_without_docker_then_it_stores_prunes_and_keeps_answering`
- **W1** `given_a_vm_with_quiet_unhealthy_and_restarting_containers_when_the_watchdog_runs_for_ten_minutes_then_it_recreates_exactly_the_sick_ones`
- **W2** `given_a_failing_docker_or_ingest_when_swept_then_failures_become_error_events_and_the_loop_goes_on`

Doubles: reporting-service gets a Docker Engine served on a Unix socket in the test process (the real `DockerSocket` talks
to it), `/proc` and the clock as port doubles, and real SQLite and repository files in a temp dir. The watchdog gets the
`docker` binary and the clock as doubles (virtual time advances one interval per sweep) and exports its events through the
real service-kit telemetry to an OTLP ingest served in the test process.

## reporting-service: old test -> covering flow

| old test | covered by |
| --- | --- |
| TS api: empty database, `/health` ok, uptime, rows 0, `commit` from GIT_COMMIT, JSON content type | R1 (commit `abc1234`); `unknown` without a commit: R2 |
| TS api: rows count samples and rollups | R1 (11 = 8 + 3) |
| TS api: non-GET -> 404 `not_found`; unknown path -> 404 | R1 (POST, DELETE, `/api/nope`, `/api/plans/a/b`) |
| TS api: current returns the newest of each series; empty list without samples | R1 |
| TS api: series default window 1h, newer points only | R1 |
| TS api: `it.each` 1h/24h/7d/30d raw counts | R1 (raw and hour counts per window) |
| TS api: raw, then minute, then hour, each by time | R1 (default window and 7d) |
| TS api: unknown window -> 400 with accepted list | R1 (`90d` and empty `window=`) |
| TS api: headroom without samples; cpu scales to OCPUs, container samples ignored | R1 |
| TS plans: torn/invalid manifests skipped, sorted by slug, empty plan summary | R1 |
| TS plans: resolved/unresolved/unpriced and per-day totals | R1 |
| TS plans: detail units, resolved runs, nulls; plan without start/finish | R1 |
| TS plans: unknown or torn slug -> 404; percent-encoded slug decoded | R1 |
| TS plans: no plans dir and no run log -> empty list | R1 (before the fixture is written) |
| TS runs: priced run, model without cache rate, unpriced/none/no tokens -> null never 0 | R1 |
| TS runs: torn line or non-string ts skipped; unknown run id -> 404 | R1 (`bad-ts`, `zzz`) |
| TS runs: no pricing file -> unpriced, not an error | R1 (pricing removed while running) |
| TS sampling: cpu/memory/limit per container name; requests made | R2 |
| TS sampling: stalled counter -> cpu 0; percpu fallback | R2 (`idle`, `perc`) |
| TS sampling: failing stats -> `absent=1`, others sampled | R2 (`bad`) |
| TS sampling: nameless container goes by its id | R2 (`deadbeef`) |
| TS sampling: Docker socket gone -> service stays healthy | R2 (restart phase) |
| TS storage: size_rw/size_rootfs, nameless df entries skipped, `verbose=1` | R2 |
| TS host: cpu, memory, disk samples | R2 (exact values through the `/proc` double) |
| TS persistence: samples survive a restart | R2 |
| TS prune: raw older than a day -> minute averages; minute older than a week -> hour averages | R2 (`old` series) |
| Rust flow_reporting: container sweep, failing stats, listing fails, nameless, host sweep, one unreadable proc file, df | R2 |
| Rust flow_reporting: scheduler stores each sweep and stops on request | R2 (rows grow while running, constant after stop) |
| Rust flow_reporting: series default and unknown window; headroom; plan summary; unknown slug/run | R1 |
| Rust api.rs: health rows; POST/unknown 404; empty window 400 | R1 |
| Rust sqlite_store.rs: later value wins on the same key | R2 (a newer meminfo reading replaces the one at the same instant) |
| Rust sqlite_store.rs: minute and hour rollups | R2 |
| Rust sqlite_store.rs: database reopened keeps its rows | R2 |
| Rust host.rs: cpu share of two readings; meminfo used = total - free | R2 |
| Rust host.rs: unparseable `/proc/stat` or meminfo text -> 0 | dropped: implementation detail (a parse fallback for text the kernel never writes) |
| Rust flow_reporting: the first container sweep comes one interval after start | dropped: implementation detail (timer phase) |
| TS lifecycle: SIGTERM -> exit 0 | dropped: process signal handling lives in `main`/service-kit `shutdown`, outside the in-process app; unchanged |
| TS host: values read from the real `/proc` are sane | dropped: `/proc` is an outside dependency, now doubled; `ProcHost` is three file reads and a `statvfs` |

## watchdog: old test -> covering flow

| old test | covered by |
| --- | --- |
| TS: first event `watchdog.started` with the host label | W1 (severity 9, `hostLabel`) |
| TS: no containers -> never inspected, nothing recreated | W1 (first sweep empty) |
| TS: `ps -a --format {{json .}}`, then `inspect` with the ids | W1 |
| TS: healthy, starting, healthcheck-less containers left alone | W1 |
| TS: unhealthy compose container -> compose recreates its service, error-level event with container, reason, ok | W1 |
| TS: working dir and config files become `--project-directory` and `-f` flags in order | W1 (`a-1`) |
| TS: compose fails -> ok=false and the 500-character tail of trimmed stderr | W2 |
| TS: more than three restarts in the window -> `restart_loop` | W1 (`loop-1`) |
| TS: exactly three restarts -> left alone | W1 (`three-1`) |
| TS: cooldown, not recreated twice | W1 (again exactly at 10 minutes) |
| TS: restart counter drops -> new baseline | W1 (`recreated-1`) |
| TS: the watchdog never acts on itself | W1 |
| TS: unhealthy without compose labels -> left alone | W1 (`unlabelled-1`) |
| TS: two unhealthy containers, once each, listing order | W1 (`a-1`, `b-1`) |
| TS: `docker ps` fails -> `watchdog.sweep_failed` error with the stderr, loop goes on | W2 (three sweeps, three events) |
| TS: `docker inspect` fails with no output -> sweep fails with the stderr | W2 |
| TS: OTLP/JSON log records reach the ingest with the bearer token, service name, severities, attributes | W1, W2 (every flow reads its events from the ingest) |
| TS: unreachable ingest -> keeps sweeping and recreating | W2 |
| TS: no host label -> `unknown` | dropped: a clap default on `Config`, read only by `main` |
| TS: the ingest over HTTPS with a throwaway CA | dropped: the transport is service-kit's (`flow_telemetry.rs`); the flows use a plain-HTTP local ingest |
| Rust decide.rs (8): unhealthy, loop, spread wider than the window, drop is a baseline, cooldown, self, no labels, argv order | W1 (`slow-1` is the spread case) |
| Rust snapshot.rs: name loses its slash, labels read, empty config file entries skipped, missing health/labels | W1 (through `DockerCli` over the inspect JSON) |
| Rust snapshot.rs: inspect output that is not an array -> no containers | dropped: implementation detail (`docker inspect` writes an array) |
| Rust flow_sweep.rs (6): recreate + event, compose error, no inspect without containers, listing failure keeps history, cooldown, started + sweeps per interval | W1, W2 |

## Timings

Wall times on the 4-CPU session container, one run each. Old: every Rust test of the service in the deploy profile, as
`test:unit` ran them (the TS black-box suites, which also needed a deploy build of the binary, are not included). New:
`pnpm run test:flow`, the default test profile. "Clean": the service's crates cleaned, dependencies cached. "Incremental":
one source file of the service crate touched.

| | old | new |
| --- | --- | --- |
| reporting-service, dependencies and tests from an empty target | 114.8 s | 62.5 s |
| reporting-service, clean | 10.6 s | 9.2 s |
| reporting-service, incremental (`rs/src/api/mod.rs`) | 9.5 s | 3.3 s |
| reporting-service, test binaries / tests run | 8 / 22 | 1 / 2 (1.2 s) |
| watchdog, clean | 3.3 s | 4.7 s |
| watchdog, incremental (`rs/src/infrastructure/docker_cli.rs`) | 1.7 s | 2.0 s |
| watchdog, test binaries / tests run | 6 / 17 | 1 / 2 (0.4 s) |

The watchdog's old tests were in `core` alone and linked no HTTP or telemetry stack; its flow binary links service-kit's
telemetry and axum, so it costs a second or so more while covering what the 19-test black-box suite covered. The deploy
builds (`cargo build --profile deploy --locked -p reporting-service-rs` / `-p watchdog-rs`) are green.

## Notes

- `Scheduler::stop` aborts the ticking loops, not a job already running; a sweep in flight at stop still stores its samples.
  Not caller-visible (`main` stops the scheduler on shutdown, then exits); R2 waits for it before asserting nothing more is
  stored.
- No production bug surfaced: every old behaviour holds through the flows. A mutation check (`>=` for `>` in the restart-loop
  rule, `MAX` for `AVG` in the minute rollup) fails W1 and R2.
