# tooling/wf-blackbox: expectations follow the `wf` changes

Date: 2026-10-08.

## Scope

Two black-box files pin strings the `wf` binary prints or runs, and both changed with the service:

- `deploy.integration.test.ts`: the paths the `remote-chat-agent` plan leg compares (it gained `backend/Cargo.toml`, `backend/Cargo.lock` and the embedded Tamil face, and lost `clis/rca-edge`).
- `verify-dry-run.integration.test.ts`: the e2e command the gate runs for `remote-chat-agent` (now `REMOTE_CHAT_AGENT_BASE_URL=... pnpm --filter @app/remote-chat-agent run test:e2e`).

Each expectation was changed first and run red against the old binary, then the binary changed.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| Tests in the touched files | 16 (`deploy`), 14 (`verify-dry-run`) | 16, 14 |
| Install footprint | not applicable: test files only | not applicable |

## Tests, by tier

- Black-box through the `wf` binary: `deploy` 16, `verify-dry-run` 14, `verify-tier3` 7, `report` 16: all pass.

## Deliberate behaviour changes

- The suite's expectations changed only for logged deliberate changes of `wf` (clap's wording and usage for refused command lines, the union usage of the mode commands, the opencode child without an Anthropic credential, `wf work` and `wf visual` on `wf dispatch`). Each was edited first and run red against the old binary. Four cases in `binary-smoke` pin the SDK's call shape (`--json` and `--app` between a command and its first word). The whole suite on the final tree: 1,186 of 1,186, no skips.

## Deviations

- None.

## Known gaps

- Several other tests in this package mention `backend/remote-chat-agent/src/server.ts` as a changed-file string; the file no longer exists, but the tests only use the path as text to decide which app changed, so they pass and were left alone.

## 2026-10-09: deploy plan expectations include `backend/service-kit`

Scope: `tests/integration/cli/deploy.integration.test.ts`, the two `git diff --quiet` path lists the plan must use for the `remote-chat-agent` and `leetcode-backend` legs. Before: 16 of 16 against the old paths; the changed expectations failed 2 of 16 on the old `wf`, and pass 16 of 16 on the rebuilt one. Tier: integration (the real `wf` binary with stub `git` and `docker`). Deviations and known gaps: none.

## 2026-10-09: fakes at network level for the curl-free `wf`

Plan: `docs/plans/rust-services/standard-crates.md` (phase 4). With `curl` gone from `wf` (`clis-wf.md`, `clis-http.md`), the suite's stub `curl` binaries on `PATH` no longer intercept anything. The fakes moved to the network, as the integration tier requires: separate Node processes serve HTTP (`mock-http-server.ts`), HTTPS with a generated CA and leaf certificate (`mock-https-server.ts`; rustls refuses a self-signed certificate used as an end entity) and a SOCKS5 proxy that resolves the host name itself (`mock-tailnet.ts`, for the `wf vm` and tailnet paths).

Converted to the mock servers: `agent-deps-install`, `deploy`, `deploy-run`, `dispatch-run`, `dispatch-stats`, `ios`, `ios-dev-client`, `ios-expo-go`, `ios-launch`, `metro-forward`, `metro-link`, `metro-teardown`, `report-health`, `verify-tier3`, `vm-exec`, `vm-logs`, `vm-ops` and `vm-recover`. New: `secrets-telemetry.integration.test.ts` (the OTLP post of a secret lookup arrives at a mock ingest with the typed request's shape). `dispatch-batch` gets a hermetic environment (a closed port for the preflight and the tailnet). The `browser-attach` test expects `text/javascript; charset=utf-8`.

Result on the real `wf` release binary built in the repository: 1141 of 1142. The one failure, `dispatch-batch` "a file whose dispatch fails ... FAIL 0s", is a rounded-duration assertion that reads `FAIL 1s` or more on this machine and fails identically three of three times against the std-only baseline binary. It is run on the release binary because a debug build fails a second `dispatch-batch` assertion for the same speed reason, and a binary copied out of the repository fails more tests (the harness and registry lookups follow the executable's path).

Deviation: none from the instructions. Known gap: the `dispatch-batch` timing assertion should be written against a tolerance, not a rounded second; it was not changed because it fails on the baseline too and is not part of this migration.

## 2026-10-09: the token gate and control channel on axum

Scope: `tests/integration/cli/auth-listener.integration.test.ts`, written before the gate was rewritten (`clis-wf.md`, "the VNC gate on tokio"). New or rewritten cases: the preflight for `/desktop/fit` (red on the old binary: 404) and none for `/auth/start`; a body declared over 1 MiB is answered 413 on the control port without waiting for it (rewritten from a 1.2 MB post, which the library answers before reading, so a client still writing could see a reset); two requests through the gate ride one keep-alive connection (red on the old binary: `Connection: close`); a gzip answer from the backend arrives byte for byte with its header; a real RFC 6455 echo server behind the gate negotiates a subprotocol and echoes a binary frame. Result on the real release binary: 39 of 39, none skipped; the whole suite 1146 of 1147 with `--maxWorkers=3`, the one failure being the known `dispatch-batch` rounded-duration assertion.

Deviations: none. Known gap (closed below, "flaky tests"): a handful of tests in other files failed intermittently at full parallelism.

## 2026-10-09: `dispatch-batch` pins the shape of its duration, not its value

The assertion "a file whose dispatch fails is marked FAIL" compared the whole output with `FAIL   0s    a.ts`, a rounded elapsed time that reads `1s` or more on a slower machine (it failed three of three on the std-only baseline on this one). It now matches `FAIL`, whole seconds, the file name and the totals line with a regular expression, and the stderr line likewise. Test fix, no behaviour change; the full suite is 1147 of 1147 with `--maxWorkers=3`. Known gap: at full parallelism on a 4-core machine a few timing-sensitive tests elsewhere (`browser-state` video, `deploy-run`, `metro-forward`) still fail intermittently and pass alone.

## 2026-10-09: option errors

New `tests/integration/cli/option-errors.integration.test.ts` (6 tests, the real release binary): an unknown option, a stray word, a missing value, a missing required option and a flag given a value each print clap's wording and the usage line and exit 2; a shape no command serves keeps `wf`'s own line. The whole suite is 1153 of 1153 with `--maxWorkers=3` (`clis-wf.md`, "option parsing on clap"). Deviations: none. Known gap: none now (see "flaky tests" below).

## 2026-10-09: flaky tests, root causes fixed

The suite failed a different test in most full runs on this 4-core machine (`metro start` and `restart`, `deploy run`, `browser-state`, `vm recover`, `vm ledger cat`, `metro _forward-log`, `metro status`, the Chromium group of `auth-listener`), each passing alone. Causes, found by looping single files and reading the assertions, and fixed:

- The mock servers' port files (`mock-http-server`, `mock-https-server`, `mock-tailnet`, `mock-daemon` and three inline mocks) were written in place, so a reader that saw the file exist could read it empty and dial port 0. They are written to a temporary name and renamed.
- Two files bound mock servers to the same fixed port (8123 in `metro-link` and `metro-teardown`, which other files seed as a port nobody answers on) and `ios-launch` bound 28000, which other files seed as unanswered. Each now has its own port (18124, 18123, 28900). `metro-forward` took `39000 + pid % 900` and started a server on it for every test, so the previous test's server could still hold it; it now lets the mock pick a free port.
- `metro-start` read the stub `npx` logs the moment `wf metro start` returned, while the stub could still be writing them (one failure in twelve even alone); those reads wait with `vi.waitFor`.
- `vitest.config.ts`: workers are capped at the cores minus one (at most 4) and `hookTimeout` is 45 s like `testTimeout`, because a real Chromium cold-starting in `beforeAll` outlasted the default 10 s under load.

Result: the default `npx vitest run` passed 1153 of 1153 in each of 4 consecutive runs (about 170 s each) after the fixes; before them most runs failed one to five tests. Deviations: none. Known gap: none.
