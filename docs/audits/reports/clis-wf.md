# clis/wf: deploy plan, tier-3 gate and report paths follow the service

Date: 2026-10-08.

## Scope

`wf` is already Rust (the October 6 port). This migration touched four small places so the CLI describes the repository as it now is:

- `wf deploy plan` (`deploy.rs`): the `remote-chat-agent` leg ships when `backend/Cargo.toml`, `backend/Cargo.lock` or the embedded Tamil face change (the image now builds the service binary from them), and no longer lists `clis/rca-edge`.
- `wf verify` and `wf verify <sha>` (`verify.rs`, `verify_tier3.rs`): the e2e gate for `remote-chat-agent` runs against the deployed service (`REMOTE_CHAT_AGENT_BASE_URL`, default `http://remote-chat-agent-prod:8787`), where it used to spawn a local Node copy of the service against the real opencode, so the gate now tests what was deployed.
- `wf report` (`report_run.rs`): the package's integration tests run with the default vitest config; the separate integration config is gone.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| `wf` binary | 5,712,536 bytes (x86_64 release build, after); 4,936,120 bytes on the VM (aarch64, recorded 2026-10-06) | not re-measured on the VM: the change is string constants and one list |
| Start, memory, latency | recorded for the whole CLI on 2026-10-06 (`--help` 1.6 ms, 5.2 MB) | unchanged, not re-measured for the same reason |
| Services the plan ships for a service-code change | the service package plus `clis/*` | the same, plus the backend workspace manifests and the Tamil face |
| What the tier-3 e2e gate exercises | a local Node copy against the real opencode | the deployed service |

## Tests, by tier

- Runbook: `wf deploy plan` run for real against the VM answered `{"legs":[{"app":"remote-chat-agent"},{"app":"leetcode-auth-cookie-capture"}],"metro":false}` for the working tree; the CI plan job on `dev` printed the same legs and passed. `wf deploy run remote-chat-agent --ref <branch>` built and redeployed the Rust image twice.
- Black-box through the `wf` binary: `deploy` (16), `verify-dry-run` (14), `verify-tier3` (7) and `report` (16) pass, 53 tests. The expectations were changed first and failed before the strings changed.
- Build: `wf` builds with `--locked` from exactly the files its three Dockerfiles copy, checked in a clean tree.

## Deviations

- The tier-3 gate's meaning changed on purpose (see Scope).

## Known gaps

- The e2e `chat` test has not yet run from the CI runner against the deployed service; the same turn was run by hand (`docs/audits/reports/backend-remote-chat-agent.md`).
- Resolved on 2026-10-09: `wf work` and `wf visual` run on `wf dispatch`; see "The worker and the visual judges on `wf dispatch`" at the end.

## 2026-10-09: the deploy plan knows `backend/service-kit`

Scope: `wf deploy plan` lists `backend/service-kit` in the paths of the `remote-chat-agent` and `leetcode-backend` legs, so a change to the shared crate redeploys both services (`clis/wf/src/deploy.rs`). No other command changed.

| Measure | Before | After |
| --- | --- | --- |
| Paths in the `leetcode-backend` leg | 2 plus its package | 3 plus its package |
| Paths in the `remote-chat-agent` leg | 12 plus its package | 13 plus its package |
| Binary, start, memory | not affected | not re-measured |

Tests, by tier: integration, `tooling/wf-blackbox` deploy tests, 16 of 16 (the two path expectations were changed first and failed on the old binary, 2 failed; green after rebuilding `wf`). Deviations: none. Known gaps: none new.

## 2026-10-09: everything `wf` did by hand moves to standard crates (standard-crates plan, phase 4)

Plan: `docs/plans/rust-services/standard-crates.md`. Decision log: the three phase 4 entries. Branch `claude/clis-ureq` (PR #338), stacked on the backend slices.

### Scope

`wf` carried its own copy of most of the infrastructure the backends had: `curl` spawns for every HTTP call, hand-assembled OTLP JSON posted through a curl config on stdin, three static-file servers (`wf visual`, the bench lab, the browser daemon's device-frame route), the browser daemon's JSON server, the VNC control listener, an HTTP client for the daemon, a SOCKS5 client for the tailnet forwarder, `extern "C"` bindings for `kill`, `signal`, `setsid`, `gethostname`, `dup`, `dup2`, `wait4` and `_exit` in eleven places, three copies of civil-date arithmetic and an ISO parser, three `percent_decode` and three `form_encode` copies, `hex`, a `/dev/urandom` read for link tokens, and nanosecond-named scratch directories. Now: the shared client `cli-http` (see `clis-http.md`), `opentelemetry-proto`'s types for the OTLP request (`otlp.rs`; its serde mapping is the OTLP/JSON encoding), `tiny_http` for every server (`staticd.rs`, the browser daemon, the VNC listener), `mime_guess`, `httparse` for the token gate's heads, `socks` for the SOCKS5 handshake, `nix`, `signal-hook` and `libc` for the system calls, `tempfile`, the `time` crate behind `rca_core::time`, `getrandom` and `uuid`, `hex`, and `cdp`'s tungstenite client (see `clis-cdp.md`). `Command::new("curl")` and `extern "C"` are gone from `clis/`.

### Before and after (release builds)

| Measure | Before | After |
| --- | --- | --- |
| Binary | 5.71 MB | 7.36 MB |
| `wf --help` cold start p50 (n=7 after a warm-up) | 1.62 ms | 2.0 ms (noise between runs is about 0.2 ms) |
| `wf --help` peak RSS p50 | 12.1 MB | 12.2 MB (measured after slices A to D) |
| One command that posts its telemetry event (wall p50, n=5) | 11.9 ms | 5.6 ms |
| `curl` spawns | 19 | 0 |
| `extern "C"` bindings | 11 sites | 0 |

### Tests, by tier

- Integration (`tooling/wf-blackbox`, the real `wf` release binary, fakes at network level): 1141 of 1142. The one failure, `dispatch-batch` "a file whose dispatch fails ... FAIL 0s", reads `FAIL 1s` or more on this machine and fails the same three of three times with the std-only binary: a rounded-duration assertion against this machine's speed. A debug build fails a second `dispatch-batch` assertion for the same reason, so the suite is run on the release binary. A binary copied out of the repository fails more tests (the harness and registry lookups follow the executable's path).
- Real-dependency (a real Chromium): the browser, visual and bench suites drive `wf` against it; `cdp` has its own 22.
- Unit and flow: 44 in `wf` (its binary and its test files), including the OTLP encoding and the caveat guard's golden payloads compared as typed requests.
- Runbook and e2e: `wf` is a CLI run from a checkout or from an image; its deployed images (`agent-shell`, `wf-cli`, and the `wf` inside `remote-chat-agent`'s image) rebuild through the pipeline. The `remote-chat-agent` leg of `wf deploy plan` now plans on `clis/http` too, and the Dockerfiles that build `wf` copy it (a first deploy failed on the missing `COPY`; fixed with its plan test).

### Deliberate behaviour changes (library behaviour wins)

- A transport failure has no `curl` exit code: `wf vm logs` exits 1 with the client's error text.
- OTLP records of `wf log emit` and the caveat guard are built and encoded by the OpenTelemetry SDK (`opentelemetry-otlp`, `Protocol::HttpJson`; see "OTLP export on the SDK" below): all default fields present, an empty `traceId` and `spanId`, and the caveat guard records carry `observedTimeUnixNano`.
- JavaScript is served as `text/javascript` and every served file's type comes from the full `mime_guess` table (the daemon knew eight extensions).
- The servers keep connections alive; a body over 1 MiB to the VNC control listener is answered 413.
- The VNC link tokens and session ids come from `getrandom` and `uuid`, which fail loudly; the old `/dev/urandom` read left a token of zero bytes if the file could not be read.
- `rca_core::time::parse` reads RFC 3339 and a zone-less date-time (UTC) and nothing else; a calendar day that does not exist is refused.
- The Ctrl-C handlers of long waits are installed once and fall back to the default action after `release`.
- Base64 with embedded whitespace is refused by the `cdp` helpers.

### Deviations from the instructions

- (Superseded by the 2026-10-09 section "the VNC gate on tokio" below.) The VNC token gate (`wf/src/vnc/proxy.rs`) was still a byte-level relay: heads are parsed by `httparse`, but it writes its own 403 and 502 answers, rewrites the `Connection` header and copies bytes for a plain response and for an upgraded WebSocket, and reaches an `https` backend through `openssl s_client`. A reverse proxy that carries a full-duplex upgrade to a TLS backend needs an async runtime or a poll loop and the owner ruled out tokio in the CLIs; a frame-level relay would also change what the gate's black-box tests pin ("a raw pipe to the backend"). It is the one hand-written HTTP forwarder left. The control route `POST /desktop/fit` writes its own `access-control-allow-origin: *` header on one response.
- (Superseded by the section "option parsing on clap" below.) `wf`'s argument parser was the repository's own (`clap` not adopted: every command's parser would change, and with it the error texts the black-box suite pins).
- Each command calls the blocking client directly, with no port trait for outbound HTTP; the black-box tier is the check for CLI entry points.

### Known gaps

- `wf` still has 17 compiler warnings that were there before this migration (an unused import in `app_bench`, a few `mut`s), (since cleared: `cargo clippy --workspace --all-targets -- -D warnings` is green for every CLI crate, after removing four dead functions and an unread field, grouping a function's arguments, adding type aliases, hoisting a regex out of a loop and renaming two variables; the black-box suite was 1141 of 1142 before and after).

## 2026-10-09: the VNC gate on tokio, axum and reqwest (standard-crates plan, phase 4b)

Plan: `docs/plans/rust-services/standard-crates.md`. Decision log: "wf's VNC gate and control listener on tokio, axum, reqwest and tower-http". Branch `claude/wf-vnc-gate-tokio` (draft PR #340), stacked on the serde slice (#339). Tier that ran: integration (the real `wf` release binary, mock backends as separate Node processes) and flow (in-process router tests); no e2e.

### Scope

The last hand-written HTTP in `wf` is replaced under an owner decision: tokio is allowed inside `wf auth listener` only. The token gate is an axum server in front of one `reqwest` client (plain requests streamed with the session cookie added; an upgrade becomes two upgraded connections joined by `tokio::io::copy_bidirectional`); the control channel is an axum router with `RequestBodyLimitLayer` and `CorsLayer` on `/desktop/fit`, the blocking engine and launcher under `spawn_blocking`. Removed: `vnc/http.rs`, `httparse`, the `openssl s_client` child process (and `openssl` from the vnc image), and the hand-set CORS header. The runtime is built in `vnc::listen()`, so no other command starts one.

### Before and after (release builds, n=5 medians)

| Measure | Before | After |
| --- | --- | --- |
| Binary | 7.36 MB | 8.15 MB |
| `wf --help` cold start (interleaved) | 3.45 ms | 3.37 ms |
| `wf auth listener` idle RSS | 7.14 MB | 8.71 MB |
| Gated GET p50 (new connection each) | 0.69 ms | 0.53 ms |
| Gated WebSocket echo round trip p50 | 0.078 ms | 0.073 ms |

### Tests, by tier

- Integration: `auth-listener.integration.test.ts` 39 of 39, none skipped (five cases new or rewritten; two were red on the old binary); the whole `wf` suite 1147 of 1147 with `--maxWorkers=3` (after the second slice's test fix of `dispatch-batch`'s duration assertion). At full parallelism on this 4-core machine a few timing-sensitive tests elsewhere fail and pass alone. (Fixed afterwards at its causes; see the flaky-tests entry in the decision log and `tooling-wf-blackbox.md`.)
- Flow and unit: `cargo test -p wf` 50 passed, 10 new (gate and control router); clippy `-D warnings` clean for the whole `clis/` workspace.
- E2E: not run (needs the desktop container's trigger secret; the owner declined secret reads for probes).

### Deliberate behaviour changes (library wins)

- Keep-alive on the gate and pooled backend connections (before: `Connection: close` both ways); response bodies are streamed.
- A body over 1 MiB, or declared as such, is answered 413 at once by `tower-http`, before the body is read.
- `OPTIONS /desktop/fit` is a preflight answered 200 by `CorsLayer`; `OPTIONS` elsewhere is the control channel's 404.
- `https://` backends are verified; `https+insecure://` is not (before, both went through `openssl s_client`, which does not verify).
- A refused upgrade carries `Connection: close` in place of a bare status line.

### Deviations from the instructions

- Tokio is in `wf`, in the listener only, by the owner's decision of 2026-10-09 (the plan said no tokio in the CLIs).
- No port trait for the backend client; the black-box tier is the check.
- `tailnet_forward.rs` moved onto `tokio-socks` and `tokio::io::copy_bidirectional` in a second slice (below); tokio therefore also runs, as one lazily started worker, in the processes that start a forward.

### Known gaps

- The vnc image no longer installs `openssl`. Both `wf-cli` and `vnc-auth-capture` were built natively on the VM from the branch (`sha-43261cb4`, build only, nothing redeployed), but the vnc image takes its `wf` from `nuesslerm/wf-cli:latest`: rebuild `wf-cli` as `latest` first, or the vnc image carries the old gate without the `openssl` it needs.
- `pnpm run audit` covers the JavaScript tree and timed out in the sandbox; no Rust audit tool is installed.

### Second slice: the tailnet forwarder

`tailnet_forward.rs` now dials through `tokio-socks` (target sent as a domain), joins the sockets with `tokio::io::copy_bidirectional` and runs on one lazily started one-worker runtime; the `socks` crate is gone. Benchmark (n=5): forward round trip p50 1.156 → 1.094 ms; binary 8,149,368 → 8,176,600 B. Tier: integration, 64 of 64 in the four black-box files that exercise it, unchanged. `dispatch-batch`'s failing assertion pinned a rounded elapsed time and now pins its shape (a test fix; the full `wf` suite is 1147 of 1147). Deviation: no port trait for the SOCKS dial. The known gap noted here about `wf`'s option matching is closed by the next section; the claim that the user-facing parser is a TypeScript Commander front end was wrong (that front end is gone).

## 2026-10-09: option parsing on clap (standard-crates plan, phase 4c)

Plan: `docs/plans/rust-services/standard-crates.md`. Decision log: "wf's options on clap". Branch `claude/wf-clap-options` (draft PR #341), stacked on the VNC gate (#340). Tier that ran: unit (`args.rs`, written first and green on the old parser) and integration (the real release binary, the whole suite); no e2e (a CLI).

### Scope

`wf`'s hand-rolled option parsing is replaced by one spec-driven wrapper over `clap` in `args.rs`: value options (repeatable, last wins, a value may start with `-`), flags, variadic and optional-value options, short aliases, required options and the positional count; help and version flags are off because `wf <command> --help` is served from the stored help texts. The 19 callers of `args::parse` and the bespoke loops of `browser`, `visual`, `bench`, `log emit`, `dispatch`, `dispatch --batch`, `ios` (sim, build, launch, start and the slot commands), `android`, `metro`, `tailscale`, `verify`, `vm recover`, `knowledge`, the browser daemon and `cm6` moved onto it. By hand in `args.rs`, with reasons: `lift_globals` (the global `--json` and `--app`, lifted from anywhere while the rest reaches the command unread) and `has_flag` (the mode probes that pick a command's mode before it parses). A correction to the earlier sections: the TypeScript Commander front end is gone and these parsers are the user-facing ones.

### Before and after (release builds, medians of 5 interleaved rounds of 30 spawns)

| Measure | Before | After |
| --- | --- | --- |
| Binary | 8.18 MB (forwarder slice) | 8.31 MB (`clap` is about 130 KB) |
| `wf --help` cold start | 3.24 ms | 3.16 ms |
| A refused command line (`metro status --bogus`) | 4.93 ms | 4.96 ms |
| A parsed command line (`metro status --identifier x`) | 7.42 ms | 7.66 ms |

### Tests, by tier

- Unit: `cargo test -p wf` 64 passed, 14 in `args.rs` (semantics pinned on the old parser first, then the clap-specific cases).
- Integration: the new `option-errors.integration.test.ts` 6 of 6 (clap's wording and usage, exit 2, and `wf`'s own line for an unserved shape); the whole suite 1153 of 1153 with `--maxWorkers=3`. At full parallelism a few timing-sensitive tests fail intermittently and pass alone. (Fixed afterwards at its causes; see the flaky-tests entry in the decision log and `tooling-wf-blackbox.md`.)
- `cargo clippy --workspace --all-targets -- -D warnings` clean.

### Deliberate behaviour changes (library wins)

- A refused command line prints clap's error and usage and exits 2 (before: nothing, or `wf <cmd>: the Rust wf does not support these arguments`, or Commander-worded messages in `visual` and `vm recover`).
- A bare `--` ends the options; `verify --staged --files` with no files is an error; a repeated flag is recorded once; a dangling `--app` is refused by the command that receives it.

### Deviations from the instructions

- `lift_globals` and `has_flag` were hand-written at this point (one file, reasons above); the next section replaces them. `help.rs` and the stored help texts stay, so clap's own help output is unused.

### Known gaps

- None for option parsing beyond the two helpers, which the next section replaces.

## Program options and mode probes on clap (standard-crates plan, phase 4d)

Plan: `docs/plans/rust-services/standard-crates.md`. Decision log: "wf's program options and mode probes on clap". Branch `claude/wf-global-options`. Tier that ran: unit (`args.rs`, written first and red) and integration (the real release binary, the whole suite four times in a row); no e2e (a CLI).

### Scope

`lift_globals` is replaced by `args::split_program`: one `clap` command with `--json`, `--app <name>`, `--help`, `--version` and an external subcommand, so the command and its words reach the command verbatim. Every command's own parse (`args::command`) accepts `--json` (unless the command lists it) and `--app`, read and dropped. `report-health`, `caveat` and its guard commands, `browser tunnel`, `deploy sync-creds`, `deploy tf apply`, `auth listener` and the `dispatch` mode flags moved onto the shared parse; `agent-config-sync`'s `clap` command accepts the two options. `has_flag` is twelve lines over `clap_lex` (the lexer `clap` is built on), because `clap`'s `ignore_errors` stops at the first option a command does not declare and so cannot find one after it; three of its eleven probe tests were red against that design.

### Before and after (release builds, medians of 5 interleaved rounds of 30 spawns, before = PR #341)

| Measure | Before | After |
| --- | --- | --- |
| Binary | 8,305,912 B | 8,311,672 B |
| `wf --help` | 3.13 ms | 3.05 ms |
| A refused command line (`metro status --bogus`) | 5.57 ms | 5.39 ms |
| A parsed command line (`metro status --identifier x`) | 8.71 ms | 8.37 ms |
| With the program options (`--app x --json metro status`) | 8.69 ms | 8.15 ms |

### Tests, by tier

- Unit: `cargo test -p wf -p agent-config-sync` 89 passed; the probe and `split_program` cases first, red, then green.
- Integration: `binary-smoke` 33 of 33 (a matrix of `--app` and `--json` placements, green on the old binary, and two tests red on it), `option-errors` 6 of 6; the whole suite 1176 of 1176, four runs in a row. One earlier full run had the `auth-listener` desktop-browser `beforeAll` fail (ten tests skipped); not repeated in five further full runs, cause not captured.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.

### Deliberate behaviour changes (library wins)

- A dangling `--app` or an unknown option before the command is clap's error (exit 2). A leading `--json` or `--app` before `wf browser ...` works (it did not).
- Every command accepts and ignores `--json` and `--app` after it. `--app` between a command and its sub-word (`wf ios --app x status`) is no longer lifted: an unserved shape, exit 2. Reversed on 2026-10-09 by `args::lift_leading_options` (see the last section): the SDK runs `wf browser --json <command>`, so the shape has a caller.
- After `deploy tf exec <dir>` the options are Terraform's; `caveat guard-eval` also takes `--runs N` and `--filter X`; `wf agents --app x sync` is accepted.

### Deviations from the instructions

- `has_flag` remained over `clap_lex` at this point; the section after the source-map one removes it. Two unit tests in `args.rs` showed the `clap`-only designs failing (`ignore_errors` misses a flag after an undeclared option, a catch-all positional swallows one after a word). The `--help` and `-h` word checks in `help.rs` and `app_bench` stay because they serve the stored help texts.

## Source-map decoding in `app_bench` on the `sourcemap` crate (standard-crates plan, final checks)

Plan: `docs/plans/rust-services/standard-crates.md`. Decision log: "wf's source-map decoding on the sourcemap crate". Branch `claude/wf-global-options`. Tier that ran: unit (`app_bench/domain.rs`, the new decoding cases written first and green on the old code), integration (the real binary, the whole `wf` suite and the two `app-bench` files); no e2e (an internal benchmark command that needs a browser and an Expo export, not run here).

### Scope

`app_bench/domain.rs` decoded the `mappings` field of a source map by hand (the base64 alphabet, the VLQ digits, the running source index and column; about 35 lines). It now reads the map with `sourcemap` 9.3.2 (getsentry, BSD-3-Clause, default features off): `suites.rs` parses the `.map` file into a `SourceMap`, and `decode_lines` groups its tokens by generated line. `executed_bytes_by_source` takes the `SourceMap` instead of the sources and the mappings string and no longer returns a `Result`. Dependencies added (all MIT or Apache-2.0 except `sourcemap` itself): `sourcemap`, `base64-simd`, `vsimd`, `outref`, `bitvec`, `funty`, `radium`, `tap`, `wyz`, `debugid`, `if_chain`, `unicode-id-start`. Maintenance and provenance were read from the registry metadata (repositories under getsentry, Nugine, bitvecto-rs, myrrlyn, lambda-fairy and Boshen); `pnpm run audit` covers npm only, so no Rust advisory check ran.

### Before and after

| Measure | Before | After |
| --- | --- | --- |
| Decode of a generated map (4,000 lines, 240,000 segments, 1.2 MB of mappings; the crate's figure includes parsing the JSON), medians of 5 interleaved runs, release build, the two decoders in one binary | 25.44 ms | 11.91 ms |
| The segments the two decoders return for that map | identical | identical |
| `wf` release binary | 8,311,672 B | 8,377,656 B (+65,984 B) |

### Tests, by tier

- Unit: `cargo test -p wf app_bench` 28 passed, 25 of them in `domain.rs`; five decoding cases (a continuation digit, a negative delta, a segment without a source, a segment with a name, a source delta on a later line) were written first and green on the hand-written decoder, then on the crate after two fixtures changed (a `names` entry in the generated map, column order in the negative-delta case).
- Integration: the whole `wf` suite 1176 of 1176 on the deploy-profile binary; `app-bench` and `app-bench-run` 17 of 17.
- `cargo clippy -p wf --all-targets -- -D warnings` clean.

### Deliberate behaviour changes (library wins)

- Segments of a line come in column order (the crate sorts tokens by generated line and column); the hand-written decoder kept the order of the map, which differs only for a map that is not sorted.
- The lines covered are the code's lines, so executed bytes after the last line the map mentions are reported as "unmapped" instead of being dropped.
- A map the crate refuses (a segment of two or three fields, a source or name index out of range) is an error at load, with the crate's message and the map's path; the old decoder tolerated both.

### Deviations from the instructions

- None for source maps.

## Mode probes on the union of the modes' specs, and `--json` from the command's own parse (standard-crates plan, final checks)

Plan: `docs/plans/rust-services/standard-crates.md`. Decision log: "wf's mode probes on the union of the modes' specs". Branch `claude/wf-global-options`. Tier that ran: unit (`args.rs`, written first), integration (the real binary, the whole `wf` suite), e2e for the gate that shares the binary (aarch64 on the VM host, see the decision log).

### Scope

`args::has_flag` (twelve lines over `clap_lex`) is gone, and so is the `clap_lex` dependency. The commands that picked their mode before parsing now parse once against the union of their modes' specs: `args::probe(args, &[&mode specs])` builds `Spec::union` (nothing required, any positionals; the lists are built once and kept) and returns the parse, and the mode is the one whose option is in it. `dispatch` (batch, runbook, pricing-sync, stats, prompt run), `verify` (staged, dry-run, budget, the real run), `report-health` and `knowledge index --gemini` do this; each mode's spec is now a `const` beside its parser (`SPEC` in `dispatch_run`, `dispatch_batch`, `runbook`, `report_run`, `verify_tier3`, `knowledge_gemini`), and each mode still parses with its own spec. `--json` is read from the command's own parse: `runbook` and `report-health` use `Parsed::flag("json")`, `browser` asks `args::json_seen()`, and the `--json` envelope of `ios`, `android`, `metro` and `cm6` starts when their parse meets the flag (`parse_spec` calls `json_mode::begin`, which redirects stdout to stderr from then on; `json_mode::run` arms it and prints the envelope).

### Before and after (release builds, medians of 5 interleaved rounds of 30 spawns)

| Measure | Before | After |
| --- | --- | --- |
| Binary | 8,377,656 B | 8,379,800 B |
| `wf --help` | 2.99 ms | 3.05 ms |
| A refused command line (`metro status --bogus`) | 4.79 ms | 4.75 ms |
| A parsed command line (`metro status --identifier x`) | 7.53 ms | 7.66 ms |
| With the program options (`--app x --json metro status`) | 7.59 ms | 7.67 ms |
| A mode probe (`dispatch --stats --log /nonexistent`) | 4.95 ms | 5.32 ms |

The mode probe costs one more `clap` parse (about 0.4 ms).

### Tests, by tier

- Unit: `cargo test -p wf -p agent-config-sync` (80 in `wf`); written first: a flag of either mode found wherever it stands, an option that is the value of another option is not a mode option, what follows `--` is a word, an option no mode takes is refused with clap's error kept, a value given to a mode flag is named in the error, `json_seen` before and after a parse. The two `has_flag` cases that pinned the lexer (`--` handling, a longer name) have no counterpart: the parse covers them.
- Integration: the whole `wf` suite 1178 of 1178 (1176 and the two new `option-errors` tests, which were green on the old binary: a value given to the flag that picks a mode, and an option no mode takes beside a mode flag). One test was red on the first run (`verify --budget=1` answered "unexpected argument '--budget'"); a failed probe now keeps the union's error, which names the flag and the value as before.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.

### Deliberate behaviour changes (library wins)

- A mode flag that stands as the value of another option (`dispatch --prompt --stats`) is that option's value, not a mode (the lexer scan saw a mode).
- The `Usage:` line of a refused `dispatch`, `verify`, `report-health` or `knowledge index` line is the union's (`wf verify [OPTIONS] [ARG]...`), not the one mode's.
- An unsupported shape of the four `--json` groups under `--json` is clap's error as before; a line whose parse fails never starts the envelope (before, the wrapper was armed by the scan and the handler's refusal still came out as the plain error).

### Deviations from the instructions

- `Spec::union` keeps the merged lists for the life of the process (`Box::leak`), because a `Spec` holds `'static` slices; the CLI runs one command per process.

## The worker and the visual judges on `wf dispatch`, and program options between a command and its first word (2026-10-09)

Plan: `docs/plans/rust-services/standard-crates.md`; owner instruction that nothing may use the TypeScript `rca` CLI. Branch `claude/wf-global-options` (PR #343). Tier that ran: integration (the real binary, the whole `wf` suite, 1,186 of 1,186, no skips) and unit (`args.rs`); every change below has a black-box test that was written first and red.

### Scope

- `wf work` (`worker_run.rs`): the implementer is `wf dispatch` run as a child, not `pnpm rca run`. The child is started in its own process group (`process_group(0)`) and a timeout kills the group (`killpg`), so a stuck opencode or its `sleep` does not outlive the unit.
- `wf visual` (`visual.rs`): the judges run as a pinned `wf dispatch --no-auto` child. The ladder file path is `clis/rca-core/data/model-ladders.json`.
- `wf dispatch` (`dispatch_run.rs`): accepts `--shape` and `--model` (a pin that is empty is no pin), the shape of a run the worker and the judges choose; the opencode child has `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` removed from its environment.
- `args::lift_leading_options` (`args.rs`, `main.rs`): `--json` and `--app <name>` given between a command and its first word (`wf browser --json open`) are moved behind the words, as `lift_globals` did before phase 4d. This is a regression of mine: after the clap move the shape `@lib/browser-client` runs for every call (`wf browser --json <cmd>`) was refused by clap. It was found while building the rustwright comparison (`docs/audits/benchmarks/rustwright-vs-wf-browser.md`), whose harness calls the SDK, not by a test; the fix has two unit tests and four black-box cases in `binary-smoke.integration.test.ts`, red before it. No black-box test of the SDK's real call shape existed; these four are it.

### Deliberate behaviour changes

- A unit or judge can no longer run on the claude or codex gateways (they were TypeScript and are deleted). The ladder's opencode rungs serve them.
- The opencode child never inherits an Anthropic credential, which is the repository's stated rule; the Node gateway selection used to be the only enforcement.
- `wf work` kills a timed-out implementer by process group; before, the TypeScript child was killed alone.

### Deviations from the instructions

- No benchmark of the worker change against the old TypeScript path: the old path is deleted and rebuilding it would cost more than the answer is worth. The after numbers are in the decision log entry of the same date.

## OTLP export on the SDK (2026-10-09, owner instruction)

`clis/wf/src/otlp.rs` (111 lines, `ExportLogsServiceRequest` assembled from `opentelemetry-proto` types) is gone. `clis/wf/src/log_export.rs` hands one record to the OpenTelemetry SDK and sends it with the official `opentelemetry-otlp` HTTP/JSON log exporter and its own blocking `reqwest` client (`reqwest-blocking-client`, `reqwest-rustls` with the native roots, as the services do); there is no transport code of ours. The export runs on a thread of its own because the blocking client may not be created or dropped on a Tokio worker. `log_emit.rs` and `caveat_guard/telemetry.rs` only fill a `Record`. New dependencies: `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` 0.30 (the services' versions) and `futures-executor`; `opentelemetry-proto` is no longer a direct dependency. A first version sent through a 20-line `ureq` adapter; the owner asked for the officially supported client instead, and the adapter was removed.

Tests first: the black-box `log-emit` suite (URL, bearer header, `application/json`, resource, scope, severity, attributes, the `HTTP 401` reason) and a golden test of the caveat guard's wire body (rewritten to compare the JSON on the wire, ignoring proto defaults) were green on the old code and stay green; `log_export` has four unit tests against a local `tiny_http` ingest.

Deliberate behaviour changes: none on the wire. `wf log emit` no longer bypasses the environment's proxy variables (`Via::Direct` is gone; the official client reads them), and the guard's export no longer goes through `cli-http`. A rejection is read from the exporter's error as `HTTP <status>`, as before.

Deviations: the SDK adds `opentelemetry` crates and a blocking `reqwest` client to a command that needed none (release binary 8,381,240 to 8,468,248 B, +86,008 B, +1.0 %). `wf log emit` against a local HTTPS ingest, 5 rounds of 10 runs interleaved: 10.39 ms before, 10.14 ms with the `ureq` adapter, 10.24 ms with the official client (same within noise); peak RSS 22.2 MB unchanged. The first benchmark server answered without a `Content-Length`, which the official client rejects as an unexpected EOF; the script now sends one. Records: `docs/audits/benchmarks/wf-otlp-sdk-{before,after,official-client}.json`, method `wf-otlp-sdk-bench.py`.

## The command surface without aliases (2026-10-10, owner instruction)

Deliberate behaviour changes:

- Every old top-level name (`verify`, `deploy`, `work`, `status`, `dispatch`, `agents`, `secrets`, `auth`, `vm-ops`, `tailscale`, `report-health`, `cli-bench`, `bench`, `cm6`, `knowledge`, `visual`, `metro`, `ios`) is an unknown command, exit 1. The table in `routes.rs` is the only routing; `surface.rs` and the alias notice are gone.
- `--app` is `--frontend`.
- `wf ios` and the local per-slot Metro (`wf metro`, the Metro claim in the slot registry) are removed. The registry's Android port pair lives under `port_claims.androidPortPair`; the legacy `metro_claims` key is still read and ignored. The browser daemon resolves `--path` against `WF_SHARED_METRO_URL` (default `http://metro-runner-main:20000`).
- `mla-tamagui` is a service of `wf ship` (kind: Metro server): `wf ship mla-tamagui [--clear-cache]` restarts the shared dev Metro; `--ref` is ignored, `--redeploy-only` and `--build-only` are refused. `wf ship --changed` (was `--plan`) emits `legs` only, with `mla-tamagui` first; Deploy has no `metro` job. `wf ship retire <service>` replaces `--retire`.
- `wf ship eas [--branch --initial-route --message --frontend]` publishes the EAS Update through `publish:eas` and needs `EXPO_TOKEN`; the tier-3 gate uses the same function. `wf ship --status eas` is not implemented.
- `wf android` passes the claimed serial to adb: `adb`, `open`, `tap`, `swipe`, `text`, `screenshot`, `install`, `launch`, plus `start|stop|status`; `cleanup`, `nuke` and `build` are removed.
- `vm` groups `exec logs ledger recover deps creds net terraform ops`; `vm logs` takes `--service <name>`. `secret login <tool>` (and `--remote`, `list|capture|done|launch-url|listener`) replaces `auth`; the approve guard and the exposure guard match the singular `secret` only. `repo` groups `bench [cli]`, `knowledge rank|locate|index|stale`, `sync` (agent config), `bundle` (the CodeMirror bundle), `ladder clear-cooldown|sync-pricing`. `knowledge for` and `knowledge which` became `rank` and `locate`.

Deviation from the plan: `repo sync` is the agent-config sync only; the editor bundle is `repo bundle`. `mcp-guard` stays a hidden top-level command. Measured: 803 black-box tests passing, cold start 3.64 ms median (baseline 3.6 ms), details in the decision log.

The tier-3 browser honours `WF_SOCKS_PORT` so `wf check --deployed` runs from a machine that reaches the tailnet only through tailscaled's SOCKS5 listener.

After a `wf ship mla-tamagui --clear-cache` the shared Metro compiles its web bundle on the first request, which takes longer than the 30 s navigation budget of the tier-3 browser; request the bundle once before `wf check --deployed`. Cold start on the verification head: median 2.98 ms (`wf --help`) and 3.24 ms (`wf ship --help`), details in the decision log.
