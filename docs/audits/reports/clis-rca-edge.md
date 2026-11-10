# clis/rca-edge: retired

Date: 2026-10-08. The crate is removed.

## Scope

`rca-edge` was written on 2026-10-06 as the strangler front for the Node service: a std-only Rust binary that answered the routes it had ported (`GET /health`, preflights and the origin-scoped CORS policy, the 120 per minute limiter) and tunnelled everything else, WebSocket upgrades included, to the Node service byte for byte. The plan was to move one route at a time behind it. It was never wired into a compose file, an entrypoint or an image command: it was a workspace member that three Dockerfiles (`remote-chat-agent`, `agent-shell`, `wf-cli`) copied so the workspace would load, and a path in the deploy plan.

The black-box suite turned out to be a sufficient oracle for replacing the service in one change, so the strangler front was never needed. This departs from the goal's step 2, which said to grow `rca-edge` into the service skeleton; the service is a new crate instead (`docs/audits/reports/backend-remote-chat-agent.md`, "Deviations from the instructions"). With the whole service in Rust it fronts nothing. Its two behaviours, the CORS policy and the limiter, are in `rca-service` and pinned by `http.blackbox.test.ts`.

Removed: the crate (246 lines of code, 206 of socket tests, a 355 kB binary), its workspace member and lockfile entry, the `COPY` line in the three Dockerfiles, and its path in the deploy plan (and the plan test that pins the list).

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| Source | 246 lines of code, 206 of tests | removed |
| Binary | 354,912 bytes (334 KB when measured on 2026-10-06) | none |
| Deployed | never | not applicable |
| Workspace members of `clis/` | 5 (`wf`, `agent-config-sync`, `rca-core`, `rca-edge`, `cdp`) | 4 |
| Numbers measured on 2026-10-06 against the Node service it fronted | `/health` p50 0.48 ms against Node's 1.09 ms, about 2.2 MB resident, as a floor while Node still ran | not applicable: the service's own `/health` is 0.31 ms at 6.5 MB resident |

## Tests, by tier

- Before removal: 13 socket tests in `clis/rca-edge/tests/edge.rs` (health, CORS, preflight, limiter, tunnelling); they are removed with the crate.
- After: `wf` builds with `--locked` from exactly the files the Dockerfiles now copy (built in a clean tree holding only those files); the `wf deploy` black-box file (16 tests) pins the shorter plan path list; the service's own CORS and limiter behaviour is covered by `http.blackbox.test.ts` (21 tests, in the 304 passing on the Rust binary).

## Deliberate behaviour changes

- None: the crate is retired and nothing calls it; the service it fronted is the Rust `remote-chat-agent` itself.

## Architecture deviations

- None in behaviour. Anything that expected an `rca-edge` binary (none does) would break.

## Known gaps

- `agent-shell` and `wf-cli` images are deployed by hand and were not rebuilt in this change; their next build copies one directory fewer. The workspace build they run was checked from the file set their Dockerfiles copy.
