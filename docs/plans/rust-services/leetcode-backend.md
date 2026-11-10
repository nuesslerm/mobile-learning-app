# Port `leetcode-backend` to Rust, behind a strangler front, merged continuously to dev

## Status (2026-10-08): done and deployed
The port landed as `backend/leetcode-backend/rs` (commits `af6d6b5`, `8721ff5` on `dev`). It differs from the plan below in four ways, each decided on evidence, and the numbers are in `docs/audits/2026-09-02-token-diet-decision-log.md` ("leetcode-backend is Rust, deployed").
- No strangler (phases 2 to 6 merged into one change). The black-box suite of phase 1 was the oracle: the Rust service was written until it passed the same 69 tests as the Node bundle, then replaced it. A front door proxying to Node would have cost more memory than either end.
- No HTTP or TLS crate. The server is std only (thread per connection); outbound HTTPS goes through `curl` with the request on stdin, as in `wf`. The only dependency is `serde_json`. Decisions 1 and 3 below are answered: no hyper or rustls, and no shared crates yet (`secrets-rs` was not needed: the service reads only its environment, the deploy composes `.env` from the vault).
- Decision 2 (where the Rust build runs): inside the service Dockerfile on the VM, as the plan proposed. The leg took 115 s, warm.
- The image keeps Python (with `black`) and Node for the code it executes, so it shrank by a quarter (352 MB to 266 MB) rather than losing the Node runtime. The service process went from 78 MB to 2 MB idle.
Open: the exported spans were not queried back at the sink (the ingest accepts the document and the logs path is verified; the query credentials are not reachable from here), and the real leetcode.com relays are covered by the mock suite and a TLS check from the image, not by a call with a live session.

## Context
`@app/leetcode-backend` is the small cold service (1,426 source lines, 17 test files): Express on :8789, zod, `@lib/logging`, `@lib/rate-limit`, `@lib/secrets` (a thin loader over the Rust `wf secret`), contracts from `@app/leetcode-backend-contracts`. Same method as the `wf` port: black-box tests first, one PR per slice off `origin/dev`, unported routes forwarded to the Node process, TypeScript deleted with each slice, numbers in `docs/audits/2026-09-02-token-diet-decision-log.md`. It is also the proof that the pattern works for a long-running service before `remote-chat-agent`.

What the service does (all of it must be reproduced):
- `GET /health`; CORS wildcard; rate limit 120/min; JSON body limit 1 MB; request spans.
- `/api/leetcode/*` behind a constant-time `Authorization: Bearer <VM_GATEWAY_AUTH_SEED>` check: `user-status`, `login-session` (GET), `test`, `submit`, `poll/:id`, `run`, `format`. `/session` and `/login-session` (POST) behind `X-Admin-Secret`.
- Outbound HTTPS to `leetcode.com` (GraphQL and the interpret/submit/check endpoints, cookie and CSRF headers) with an hourly session refresh that triggers the capture tool over HTTP.
- `run` and `format` spawn `python3` with an embedded harness (5 s per case, 30 s per process; `black`/`autopep8`/`ast.unparse` for formatting); JavaScript through Node.
- File-backed session store (`LEETCODE_BACKEND_SESSION_FILE`).

The memory win is the Node runtime (measured below: 78 MB idle for the production bundle) for a Rust process of a few MB. Python stays: it is the language being executed, so each `run` still starts an interpreter. Expect tens of MB saved on a 24 GB VM: modest, and the reason to do it first is that the service is small.

## Baseline (measured 2026-10-08, before any port)
The service as production runs it (`node dist/bundle/index.mjs`, built by `pnpm --filter @app/leetcode-backend run bundle`, 2.6 MB), env supplied, the secrets loader stubbed, `/health` only, x86_64 4-core sandbox, node 22, n=5, medians: cold start to the first `/health` 252 ms; idle RSS 78.2 MB; RSS after 300 sequential `/health` requests 79.9 MB; `/health` p50 1.07 ms, p99 5.8 ms. The dev entry (`babel-register` over the sources) is not what runs and is far worse (1.41 s, 150 MB), so it is not the baseline. Not yet measured, and needed before slice 2 lands: `/format` and `/run` latency (they spawn `python3`), the image size and the image build time on the VM, and RSS under a realistic mix.

Mechanism notes after the `wf` port: the Rust workspace already exists (`clis/Cargo.toml`, with `wf`, `cdp`, `rca-core`, `rca-edge`), so the service can be a member of it instead of a second workspace; the secrets registry is compiled into `wf` (`clis/secrets/src/data/registry.json`), so `secrets-rs` is an extraction of `clis/wf/src/secrets`, not new code; and `wf ship --changed` already takes extra paths per leg next to the pnpm package (the remote-chat-agent leg lists the Rust sources its image builds), so the cutover changes a leg's list rather than adding a kind of leg.

## Mechanism
- New Cargo workspace `backend/Cargo.toml`; crate `backend/leetcode-backend/rs`. Shared crates move to `libraries/*-rs` as they are needed: `secrets-rs` (extracted from `clis/wf/src/secrets`), `logging-rs`, `rate-limit-rs`, `vm-request-auth-rs` (the signing code in `clis/wf/src/vmops.rs`).
- The Rust binary owns :8789. A route it does not serve yet is reverse-proxied to the Node process on 127.0.0.1:8790 (the service's own `__legacy`). The container entrypoint starts both until the last slice.
- HTTP stack: hyper 1.x on a current-thread tokio runtime, rustls for outbound TLS. No axum, no reqwest default features. Decide from a size and RSS measurement in slice 1, not by habit.
- Contracts stay TypeScript (the app and `leetcode-backend-mock` import them). Rust types are hand-written serde structs; the black-box suite validates every response against the zod schemas from the contracts package, against both implementations.

## Phases (each its own PR; finish and land before the next)
0. **Baseline.** Add a `leetcode-backend` suite to `clis/app-bench` (scenarios as data): idle RSS, RSS after N requests, cold start to first `/health`, p50 and p99 for `/health`, `/format`, `/run`, image size, image build time on the VM. Stub leetcode.com. Record in the decision log before any port.
1. **Black-box tests in TypeScript.** The 11 integration tests call `createApp()` in process. Re-aim them at a real socket: start the service as a process (`SERVICE_CMD`, default the Node entry; later the Rust binary), env points at a mock leetcode.com and a mock capture tool. Seams needed, defaults unchanged: `LEETCODE_ORIGIN` (hard-coded today), a configurable spawn path for `python3`. Delete the unit tests the suite subsumes (middleware, use-cases). Same suite later runs unchanged on Rust.
2. **Skeleton + front proxy.** Rust server, `/health`, bearer and admin-secret middleware, CORS, rate limit, body limit, proxy to Node for everything else. Tier: black-box green on both; a deploy to the VM behind the same compose project. Measure the proxy overhead, which is the cost of the intermediate state.
3. **Session routes + store.** `POST /session`, `POST /login-session`, `GET /login-session`, file session store, capture registry and launcher (HTTP to the capture tool).
4. **`/run` and `/format`.** Executor: embedded harness, timeouts, kill on overrun, stdout/stderr capture, JS path. Highest-risk slice: process handling and resource limits must match, and a negative probe must prove the request reached the executor.
5. **LeetCode gateway routes.** `user-status`, `test`, `submit`, `poll`, session middleware and the hourly refresh, outbound TLS with cookies and CSRF.
6. **Observability parity.** Spans and log lines with the same static event names through `logging-rs` to BetterStack. Done only when a query at the sink returns the line the request caused (CLAUDE.md logging rule).
7. **Cutover.** Remove the Node child, `src/`, the TS tests and the Node stages of the Dockerfile. Image: slim base plus `python3` and `black` plus the binary; no Node, no `node_modules`. `wf ship --changed` legs are keyed on a pnpm package today (`@app/leetcode-backend`); add a path-based leg for a Cargo package and keep its test.
8. **Re-benchmark** with the phase 0 suite; before/after table in the decision log.

## Tests and verification (tiers named in every PR)
1. Runbook checks against the deployed service after each deploy (the real payloads through the real entry point). 2. The black-box suite through a real socket on both implementations. 3. Contract validation against the zod schemas. 4. Rust unit tests only where there is pure logic (cookie parsing, comparison modes). Given/When/Then everywhere.

## Risks
- Docker build of a Rust service on the A1 VM: build time and cache behaviour (BuildKit cache mounts for the cargo registry and target dir) must be measured in phase 2, because every deploy builds natively on the VM.
- The proxy phase runs two processes in one container: memory is higher than today until cutover. Phases 2 to 7 should therefore not sit unlanded for long.
- Executor fidelity (signals, timeouts, zombie children). Cover with a test that never returns and one that floods stdout.
- Telemetry credentials and the OTLP export format must match what `@lib/logging` sends, or the dashboards go quiet.

## Decisions needed
1. HTTP stack (hyper + rustls as proposed).
2. Whether the Rust build runs inside the service Dockerfile on the VM (proposed) or in CI with a prebuilt aarch64 artifact.
3. The shared crates' home (`libraries/*-rs`) and whether `secrets-rs` is extracted from `wf` in this effort.

## Done when
No TypeScript under `backend/leetcode-backend/src`, the image has no Node, the black-box and runbook checks pass against the deployed Rust service, and the final before/after table (idle RSS, RSS under load, cold start, latency, image size, build time) is in the decision log.
