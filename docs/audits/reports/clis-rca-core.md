# clis/rca-core: the shared run-planning crate

Date: 2026-10-08.

## Scope

`rca-core` is the pure part of remote-chat-agent that `wf` and the service share: the model ladder, the pool cooldown ledger, the run log and stats, the file stores, the opencode stream parser and detectors, the dispatch summary, the run loop and the one-shot plugin chain (2,021 lines). It was written on 2026-10-06 for `wf dispatch`. In this migration it gained a second consumer: `POST /runs` in `rca-service` judges a run by the same code, so the service and `wf dispatch` cannot disagree about a ladder, a cooldown or a failure class.

Changed in this migration: only its module documentation. The TypeScript implementation that its golden file (`tests/golden/rca-core.json`) was recorded from has been removed with the Node service, so the crate's header no longer says the TypeScript has to reproduce the file; the Rust replay (`tests/golden.rs`) is now the only reader and still pins every ladder, cooldown and ledger output. The crate's logic and API did not change.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| Behaviour | pinned by the golden file in Rust and TypeScript | pinned by the golden file in Rust |
| Consumers | `wf` | `wf` and `rca-service` (`POST /runs`) |
| Library size | 2,021 lines | 2,021 lines |
| Start, memory, latency, image | not applicable: a library, no process of its own | not applicable |

The numbers that depend on it are the service's: a one-shot run takes 106.8 ms at 9.0 MB resident in the Rust service against 121.6 ms at 168.9 MB in Node (`docs/audits/reports/backend-remote-chat-agent.md`, n=5).

## Tests, by tier

- Runbook: the deployed service's `POST /runs` is 401 without the secret and 400 with it and an empty body (`sha-0dd11c76`); the success path through the real opencode was not run live.
- Black-box through a real socket: `runs.blackbox.test.ts`, 13 tests, passed on the Node bundle before it was removed and on the Rust service (ladder or pinned models, rotation after a crash, a run that writes nothing, tool events folded as Node folded them, the diff of what was written, the run log row).
- Unit and golden: 20 tests in 3 suites (`cargo test -p rca-core`), including the replay of 469 kB of recorded cases.

## Deliberate behaviour changes

- None in the crate. The ladder file moved to `clis/rca-core/data/model-ladders.json` and is embedded with `include_str!` from there; `wf dispatch` removes `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` from the opencode child it starts (`clis-wf.md`, last section).

## Deviations

- None in the crate. In the service, a model that belongs to the claude or codex CLI fails its attempt with the text Node's provider service gives for a CLI the image does not carry.

## Known gaps

- Resolved: the ladder file and the pricing file moved to `clis/rca-core/data/` (the ladder is embedded with `include_str!` from there), and every reader (`wf`, the reporting service, the Dockerfiles) follows.
- Resolved: `wf work` and `wf visual` run on `wf dispatch`; the TypeScript `rca run` is deleted. The claude and codex gateways are not ported (`docs/audits/reports/backend-remote-chat-agent.md`, last section).

## 2026-10-09: dates on the `time` crate (standard-crates plan, phase 4)

`rca_core::time` held Howard Hinnant's civil-date arithmetic and a hand-written ISO parser, and `wf` kept two more copies of each. It is now a thin layer over the `time` crate and the only one: `iso(ms)` (`toISOString`), `utc_date(ms)`, `parse(text)` (RFC 3339, or a date-time without a zone read as UTC, seconds optional), `date_ms("YYYY-MM-DD")` and `next_utc_midnight`. `wf/src/time.rs`, `tailscale.rs`, `agent_deps.rs`, `knowledge.rs` and `secrets/telemetry.rs` call it instead of their copies.

Deliberate behaviour change: `parse` refuses `2026-10-06T12:00Z` (no seconds, with a zone), an offset without minutes (`+02`) and a day that does not exist (`2026-02-30`, which the arithmetic used to roll over). Tests: 20 in the crate (5 unit including the new `utc_date`, `date_ms` and zone-less cases, 15 golden replays), all passing. Dependency added: `time` with `formatting`, `parsing` and `macros`; no tokio.
