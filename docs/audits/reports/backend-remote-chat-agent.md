# backend/remote-chat-agent: the service ported from Node to Rust

Date: 2026-10-08. Owner decision: Rust first, Node only as a last resort. State at the end: the VM runs the Rust service, no Node process serves it, and every media capability is served by Rust or by a binary the Rust service spawns.

## Scope

- The deployed service is the crate `rca-service` (`backend/remote-chat-agent/rs`, 14,463 lines including its unit tests), a std-only thread-per-connection HTTP/1.1 and WebSocket server with no async runtime. It replaces the TypeScript Express service: the HTTP routes, the `/ws` protocol, the orchestration engine (decider, projector, file and memory stores, subscriptions, reactor), the opencode client and event translation, `POST /runs` (through `clis/rca-core`, shared with `wf dispatch`), the voice-edit DSL and planner, speech to text, document conversion, chat attachments, the Tamil check, the git, pull-request, worktree and viewer routes, the startup check, the vault lookup, BetterStack log and span export, and the heartbeat.
- Media, as decided: `sharp` became `resvg` and `image` in the binary; `shiki` became `tree-sitter-highlight` with compiled-in grammars; markdown went to a port of the original converter; PDF text is the `pdftotext` binary; DOCX is `zip` and `quick-xml`; OCR is the `tesseract` binary; speech to text is the faster-whisper sidecar or Groq through `curl`, else whisper.cpp behind a checksum-pinned static `ffmpeg`; `busboy` became a multipart reader in the crate. `playwright`, `diff2html` and `pdf-parse` were imported nowhere and are gone.
- The image (`Dockerfile`) builds the binary in its own stage and runs it as `CMD`; the compose file, the deploy plan (`wf deploy plan` also ships the service when `backend/Cargo.{toml,lock}` or the embedded Tamil face change) and the prod compose comments follow.
- Removed from the package: the TypeScript server, routes, engine, media code and the old TypeScript dispatch core (70 source files, 14,021 lines) and 59 test files; the dependencies only they used. The `rca` CLI and the rest of `src/` (the gateways, the plugins, the use cases, the unit, integration and contract tests, the TypeScript configs and the deploy script) were removed afterwards; see "The rest of the TypeScript, removed" at the end. What remains is the Rust crates (`rs`, `core`, `stt`, `opencode-server`), the black-box suite, the benchmark, the e2e tier and the data the tests replay.

## Before and after

Before is the Node production bundle (`node dist/bundle/index.mjs`) recorded before any port; after is the finished Rust binary. n=5 fresh processes, medians, one scripted session per process, the same harness (`tests/bench/service.benchmark.test.ts`). Records: `docs/audits/benchmarks/rca-pre-node.json`, `rca-pre-node-ws.json`, `rca-pre-node-runs.json` (before) and `rca-post-rust.json` (after). Local numbers are on an x86_64 developer sandbox; container numbers are on the VM (arm64).

| Measure | Node | Rust |
| --- | --- | --- |
| Cold start to first `/health` | 503.4 ms | 32.4 ms |
| Idle resident set | 133.6 MB | 6.5 MB |
| Resident set after 300 health checks | 139.5 MB | 6.6 MB |
| Peak resident set of the whole session (median, worst run) | 491.6 MB, 497.1 MB | 43.4 MB, 54.8 MB |
| `/health` p50 | 0.955 ms | 0.312 ms |
| Chat turn p50 / resident after (fake opencode, 100 ms delay in both) | 121.6 ms / 178.5 MB | 105.8 ms / 8.6 MB |
| One-shot run `/runs` p50 / resident after | 121.6 ms / 168.9 MB | 106.8 ms / 9.0 MB |
| Convert PDF p50 / resident after | 12.4 ms / 228.3 MB | 12.8 ms / 9.0 MB |
| Convert PDF from Chromium p50 / resident after | 16.0 ms / 232.7 MB | 12.8 ms / 9.1 MB |
| Convert DOCX p50 / resident after | 61.7 ms / 293.8 MB | 1.1 ms / 9.3 MB |
| Diff register and page p50 / resident after | 39.5 ms / 326.8 MB | 8.7 ms / 14.7 MB |
| Markdown preview p50 | 1.18 ms | 0.95 ms |
| TypeScript preview p50 | 1.21 ms | 0.62 ms |
| Tamil check p50 / resident after | 479.9 ms / 491.2 MB | 156.8 ms / 38.3 MB |
| Voice transcribe through the sidecar p50 | 6.6 ms | 9.7 ms |
| One WebSocket request p50 | 0.130 ms | 0.066 ms |
| A hundred WebSocket connections, per connection | 0.991 ms | 0.968 ms |
| A hundred held WebSocket connections, resident | 143.4 MB (isolated run) | 11.4 MB (isolated run, slice 6) |
| Artifact | 14,869,215 byte bundle, plus the media packages it needed installed | 17,671,312 byte binary |
| Image, unpacked (VM, arm64) | 904,610,891 bytes, 30 layers, 117 Debian packages | 837,936,988 bytes, 28 layers, 172 Debian packages |
| Install footprint in the image | `/app` 229,940 kB (service and `node_modules`) | `/usr/local/bin/rca-service` 16,608 kB |
| Container start to first answered `/health` (VM, 3 trials) | 1,496 / 1,183 / 1,155 ms | 393 / 307 / 296 ms |
| Container idle resident set, PID 1 (VM) | 134,528 kB | 5,180 kB |
| Processes in the container five seconds after start | 1 (`node dist/bundle/index.mjs`) | 1 (`/usr/local/bin/rca-service`) |
| Direct production dependencies of the package | 22 | 6 (plus the claude SDK, optional, CLI only) |
| Lockfile | | 1,192 lines fewer |
| Source in the package | 22,110 lines in 116 files | 8,089 lines in 46 files (the CLI) and 14,463 lines of Rust |

Said plainly: two routes are level or slower at the median because the Rust service spawns the tools Node ran in process (`pdftotext`, 12.8 against 12.4 ms; `curl` for the sidecar, 9.7 against 6.6 ms), each about 8 ms of process start. Idle memory rose from 5.8 to 6.5 MB when the heartbeat, exporter and vault registry came in. The first Rust image was 209 MB larger than Node's (1,114,084,028 bytes) because Debian's `ffmpeg` pulls 200 packages; the static build that the Node image's `ffmpeg-static` fetched replaced it. The whisper.cpp tree is 232 MB of the image, almost all of it the two models, which the VM does not use while `STT_URL` names the sidecar.

## Tests, by tier

1. Runbook checks against the deployed service, through its real entry point, from agent-shell on the VM over `remote-chat-agent-prod:8787`, on the final image (`sha-0dd11c76`): `/health` reports the deployed commit; `/api/agent-clis` lists opencode as the only available CLI; `POST /runs` is 401 without the secret and 400 with it; `/ws` is 401 without a token; an unknown path is 404; one real turn through `POST /api/chat` and the live opencode streamed `http-ok`; a WebSocket client created a thread, ran a turn, received the receipt and the reply, and deleted the thread. The sink returns the service's own lines for what the probes caused (`server.listening`, `health.heartbeat`, `ingestion.started`/`ended`, `ws.upgrade.unauthorized`). Every media tool in the image runs (static ffmpeg 6.0 converts to 16 kHz mono WAV, `pdftotext` 22.12, `tesseract` 5.3.0, `sqlite3`, whisper.cpp with both models). The process table is one process. Not run here: the e2e chat test against the deployed service (it needs the tier-3 runner); the equivalent turn above was run by hand.
   Fidelity against the Node image on the same samples, through the real routes, on the VM: speech to text identical on seven recordings (WAV, WebM, M4A); DOCX identical; a plain PDF has the same 563 words with one table's cells in a different reading order; a Chromium PDF has 114 words in Rust against Node's 126 because `pdfjs` splits ligatures and mangles Tamil that `pdftotext` returns exactly. No route fails a fidelity test in Rust's disfavour, so no Node worker exists. The Tamil raster differs from sharp's in 0.69% of pixels by more than 32/255 (mean 0.80/255), edge anti-aliasing.
   The diff viewer through both images (screenshots sent to the owner): file previews byte-identical; the diff page of the Node image keeps only the first token of every highlighted line (486 visible characters against Rust's 1,210 on a 30-line TypeScript diff), so Rust is better here. The cause is a non-greedy regular expression in the Node route (`/<span class="line"[^>]*>([\s\S]*?)<\/span>/g`) that stops at the first nested token `</span>`.
   The same black-box suite on both implementations, as committed at `24ebfbf`: Node bundle 287 passed and 17 skipped, Rust binary 304 passed and none skipped.
2. e2e: `health` and `convert` pass through the new `process` target, which spawns the Rust binary (4 tests); the `chat` e2e needs a real opencode and was not run. `wf verify` tier 3 now runs the e2e suite against the deployed service instead of a local Node copy.
3. Black-box integration through a real socket: 15 files, 304 tests, none skipped, against the Rust binary, which is now the harness's default (the same files passed on the Node bundle before it was removed, with the tests that pin the gap records and the Rust-only behaviours skipped there). Rust integration: `rs/tests/telemetry.rs` drives the real binary against a stand-in `curl`. The `wf` black-box files touched (`deploy`, `verify-dry-run`, `verify-tier3`, `report`): 53 tests pass.
4. Unit: 195 Rust tests in 4 suites, clippy clean with `-D warnings`; the DSL is golden-tested against the TypeScript original. The `rca` package: typecheck and lint clean; 35 files, 479 tests, 426 pass, 41 skipped, 5 fail. The five failures (`agentCliGatewayContract`, opencode-mock permission cases) fail identically on the tree before the removal and are recorded in `docs/plans/greenfield/progress.md`.

Benchmarks: n=5 for every row above; slice benchmarks and the decisions are in `docs/audits/2026-09-02-token-diet-decision-log.md`.

## Deviations from the instructions

- The goal's step 2 said to grow `clis/rca-edge` into the HTTP, SSE and WebSocket skeleton. That was not done: the service is a new crate (`rca-service`) beside its package, built the way `leetcode-backend` is (its own image stage on the backend cargo workspace), and `rca-edge` was retired as unused (`docs/audits/reports/clis-rca-edge.md`). The reason is the one the black-box suite gave: it was a sufficient oracle to replace the service whole, so the strangler front would have fronted a Node process for a few slices and then nothing. The reason was not written down when the choice was made; this is the record, and the owner can overrule it.
- The oracle was described as 151 passed and 6 skipped. It is 304 tests in 15 files because the goal also asked for black-box slices for voice, transcribe, plan, tamil and the WebSocket layer, and `/runs`, the startup check and the viewers were added before their ports. Each new file was written against the Node bundle first (the counts on Node are in the decision log under each slice). Git agrees for most: the viewers file (`9a91e81`), chat and control (`5aee168`) and the conversion, voice, Tamil and attachments slice (`00d5f7a`, 20:48) were committed before their ports (the voice routes at 21:08, conversion, attachments and the Tamil check at 21:22); the WebSocket, `/runs`, startup and inventory files were committed together with their ports (`8e9ca22`, `4f5bbc1`, `effbe7f`, `72f8ec8`), so for those the Node-first run is recorded in the log and not in the history; the original files (`http`, `routes`, `inventory`, `vcs`, `viewers`, `worktree`, `chat`, `control`) are inside the 304, and the tests that ran only on Rust (the original six and the ones added since) run unconditionally now.
- The original seven files, as committed when the oracle was 151 passed and 6 skipped (`5aee168`, 157 tests), run unchanged against the Rust binary: 133 pass. The 24 that do not are three deliberate changes (18 diff-viewer tests that register without `X-Bot-Secret`, which the owner's decision forbids; 2 that expect `claude` in the available-CLI list, which the deployed image never offered; 4 that rely on a fake opencode that only serves a service subscribing after the prompt). Detail in the decision log. The committed versions pass on both implementations.
- The goal said the four `docs/gaps/rca-*` records must pass. Twelve records exist; eight describe Node defects the port fixes, and each of the eight is named by a black-box test that runs on Rust: 14 tests in 6 files (`-t "rca-"`), all passing on the final binary. The other four are not about the port.

## Deviations from the Node service

Each is pinned by a Rust-only black-box test where it is a behaviour, and each is in the decision log with its slice.

- `claude` and `codex` are not ported. The production image was built with only `opencode` (the build prunes the others and the Node startup gate refuses them), so the deployed Node service served opencode alone; the Rust service reports both CLIs unavailable and refuses `AGENT_CLI=claude|codex` at start by name. The TypeScript `rca` CLI that carried the gateways has since been deleted, so nothing in the repository runs them; `wf work` and `wf visual` run on `wf dispatch`, which is opencode only.
- `POST /api/diff` requires `X-Bot-Secret`; Node let anyone register a diff and read any file through `expand`. Expand and file preview compare whole path components and refuse symlinks out of their root. No caller of the route exists in this repository, so a caller outside it must send the secret.
- A failed clone's message no longer carries the access token.
- Text from opencode arrives as the increments it sends (Node forwarded each snapshot as a new message, duplicating text for providers that send growing snapshots); a tool part starts once and completes once; a stream that ends before the turn settles is an error session; a turn that cannot start ends in an error the stream reports; a hard timeout is enforced by the stream itself; threads on disk are listed after a restart and a turn a crash left running is marked interrupted; a failed answer to a question reply is a 502.
- PDF text comes from `pdftotext` only; the `pdf-extract` fallback the media evaluation named is not shipped, so a missing `pdftotext` is a 422 on `/api/convert`.
- A picture attached for a model that can see now reaches it; the recording of a voice upload is written under the last segment of the client's filename.
- Errors that came from a library (an SVG that does not parse, a model reply that is not JSON) are the Rust library's text; a non-multipart upload is a 400 where Node answered 500; the printed letter in the Tamil check always uses the app's face; a pad over 16 megapixels at 2x is drawn smaller.
- `/ws`: an upgrade on another path is a 404, a refused upgrade closes the connection, `server.probe` and `models.list` are answered on their own threads, a second subscription under a held id replaces the first.
- The MCP server name is percent-encoded on the way to opencode.
- The vault lookup, the logger and the heartbeat follow the Node boot order; with registered variables unset the boot waits for the vault (about 3 s locally), as Node did.

## Known gaps

- Twelve `docs/gaps/rca-*` records are still open. Eight describe Node defects the Rust service fixes and tests (`rca-chat-event-stream-not-scoped-to-session`, `rca-chat-hangs-when-opencode-is-silent-or-down`, `rca-chat-native-image-attachment-dropped`, `rca-control-stop-targets-thread-id`, `rca-diff-expand-reads-any-file`, `rca-voice-filename-writes-outside-temp-dir`, `rca-ws-duplicate-subscription-id-leaks`, `rca-ws-list-threads-empty-after-restart`); a gap closes only through `/cf-compound`, which was not run. Four are not about this port and stay open (`rca-chat-turn-hangs`, `rca-mcp-list-shape-mismatch`, `rca-source-control-gh-unverified`, `rca-sse-chat-consumers`).
- Resolved afterwards: `wf work` and `wf visual` run on `wf dispatch` (Rust, `rca-core`) and the TypeScript `rca` CLI is deleted, so Node is no dependency of any service, of `wf work` or of `wf visual`. The claude and codex gateways are not ported and are gone with it.
- Node, pnpm and the 232 MB whisper.cpp tree stay in the image (worktree installs, `agent-deps replay`, the STT fallback); the models are the obvious next saving.
- `telemetry.rs` is a copy of `leetcode-backend`'s; a shared service crate is the right shape and is not done.
- `@lib/vm-request-auth` and `@lib/socks5-forward` have no consumer left in the workspace.
- A developer machine with the BetterStack tokens in its environment ships its test runs to the production sink under the same service name (seen while verifying), exactly as the Node service did.

## 2026-10-09: the service on standard crates (standard-crates plan, phase 3, slices A to D)

Plan: `docs/plans/rust-services/standard-crates.md`. Decision log: "remote-chat-agent on standard crates" and "remote-chat-agent's request validation on serde". Draft PRs #334 (axum, tokio, reqwest), #335 (`pulldown-cmark`), #336 (the `core` crate and flow tests), and the serde slice on `claude/rca-serde-errors` (#339 stack).

### Scope

The std-only service carried its own HTTP/1.1 server, WebSocket framing and SHA-1, multipart parser, `curl` for every outbound call, a fixed-window limiter, CORS, an OTLP exporter, a JSON logger, a regex markdown converter and validators that reproduced zod's issue JSON by hand. Now: `axum` 0.8 on `tokio` with `tower-http` (CORS, body limit), `tower_governor` (limiter), axum's `ws` and `multipart`, `reqwest` (rustls) for every outbound call including the opencode event stream, `tokio::process`, `service-kit` (`tracing` with OTLP export, shared with `leetcode-backend`), `pulldown-cmark`, `serde` with `serde_path_to_error` for every request a client sends, and the `base64`, `percent-encoding` and `uuid` crates. `zod.rs`, `ws/frame.rs`, `ws/sha1.rs`, `multipart.rs`, `curl.rs`, `limiter.rs`, `cors.rs` and the server loop are deleted.

### Before and after (n=5 medians, release builds, `bench:service`)

| Measure | Before (std-only) | After |
| --- | --- | --- |
| Binary | 17.67 MB | 21.85 MB |
| Cold start to `/health` | 32.54 ms | 34.5 ms |
| Idle RSS | 6.53 MB | 10.37 MB |
| `GET /health` p50 | 0.40 ms | 0.38 ms |
| Chat turn p50 | 105.52 ms | 104.8 ms |
| Voice transcribe p50 | 8.57 ms | 2.2 ms |
| WebSocket request p50 | 0.06 ms | 0.087 ms |
| WebSocket, 100 connections p50 | 0.90 ms | 0.81 ms (after the `TCP_NODELAY` fix; 44.0 ms before it) |
| Peak RSS over the session | 54.69 MB | 54.08 MB (slice C) |

The standard stack costs about 4 MB of binary and 4 MB of idle memory, the price the owner accepted; routes are unchanged within noise, and voice transcribe is about four times faster because the STT call is a pooled `reqwest` request and no longer a spawned `curl`. Records: `docs/audits/benchmarks/rca-pre-std.json`, `rca-post-std.json`, `rca-post-serde.json`.

### Tests, by tier

- Integration (the TypeScript black-box suite on the real release binary): 314 of 314, no skips. Expectations were changed first for the deliberate differences below and run red before the implementation.
- Flow: 8 in `rs/tests/flow_http.rs` (the real router and layers driven in-process over fake agents, a chat turn through to its SSE frames) and 7 in `core/tests/flow_orchestration.rs` (the real engine, reactor and ingestion over a fake agent, an in-memory log and a fake working tree).
- Contract: one `store_contract` runs against the in-memory store and the file store.
- Unit: 132 in the service crate, 37 in the core, one per validation rule asserting serde's exact message.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- E2E (the real VM, `wf vm exec` and `wf vm logs`): image `sha-66bf481c` built by the VM from the branch; `/health` reports that commit; the unauthenticated WebSocket upgrade is 401; convert returns the text of a `.docx` and refuses a text file; voice transcribes a synthesised recording (200, "Make the title bigger.") through the real STT sidecar; BetterStack returns the new `tracing` lines. The authenticated WebSocket handshake was run afterwards, once the approval guard allowed a vault fetch beside other commands (PR #342): from the sandbox through `wf browser tunnel remote-chat-agent`, with Node 22's built-in `WebSocket` client, the upgrade with the token is accepted (101), the first frame is `hello` (protocol 1, heartbeat 15000 ms), `ping` returns `pong` with the same `ts`, `server.probe` answers `ok` with `availableClis` `["opencode"]` and `selectedCli` `opencode`, a request with an empty id is `bad_envelope`, an unknown method is `unknown_method`, and the socket stays open (7 of 7; only kinds, flags and codes were printed, never the token). Details in the decision log.

### Deliberate behaviour changes (library behaviour wins)

- A preflight is answered 200 by `tower-http` (was 204); only an `OPTIONS` carrying `Access-Control-Request-Method` is a preflight; allowed headers are written lower-case.
- 429 is a token bucket from `tower_governor` with the library's body (was `{"error":"rate_limited"}`).
- `/ws` is a route: without an upgrade it is 400 (401 without the token); handshake refusals are axum's. An oversized body is refused by the body limit and no longer drained.
- The markdown preview is CommonMark: paragraphs inside blockquotes and list items, newlines between elements, raw HTML shown as text, and a `javascript:`, `data:` or `vbscript:` link target replaced by `#`.
- Log lines follow the `tracing` JSON shape; the core's events use snake_case fields.
- Request validation reports only the first problem, in serde's words, led by the path (`modelSelection.cli: unknown variant ...`). WebSocket `error.message` is that sentence; `POST /runs` and `POST /api/worktree` answer `{ "error": "<sentence>", "code": "INVALID_REQUEST" }`; `POST /api/edit-stats` answers `{ "error": "invalid_edit_stat", "message": "<sentence>" }`.
- A ping's `ts` must be an integer; a command's `createdAt` must be RFC 3339 in UTC with seconds and `Z`; an optional field given as `null` is read as absent; a body of the wrong shape is "invalid type: ..., expected a map".

### Deviations from the instructions

- Layers: `core` holds `domain` and `application` (ports `Backend`, `WorkingTree`, `Store`); the service crate holds `infrastructure` and `api`; only `main.rs` wires them. Outside the four layers still: the voice DSL and planner, attachments, the Tamil check, `/runs`, edit stats and the opencode proxy routes call concrete clients.
- Ids and timestamps are read ambiently inside `translate` and the engine; the engine and reactor spawn tokio tasks directly; `MemoryStore` sits beside its port.
- `/runs` calls `rca-core`'s synchronous `run_once` on a blocking thread.
- The `log::info`, `log::warn` and `log::error` call sites stay: `log.rs` is twenty-five lines over `tracing`, and some call sites build their metadata or event name at run time, which `tracing` macros cannot take as field names; rewriting them would change the shape of every line in BetterStack for no gain.
- A DSL command is internally tagged, so serde reports a failed range check at the command, not at the field.
- The engine stays synchronous (the state machine is microseconds of work; disk appends are short) with async edges, as the plan says.

### Known gaps

- The Node-era image contents (Node, pnpm, whisper.cpp) are unchanged.
- The `chat` e2e test needs a real opencode and the tier-3 runner; the equivalent turn was run by hand against the VM (see the E2E line above).

## The rest of the TypeScript, removed (2026-10-09)

Owner instruction: nothing may use the TypeScript code any more, so the migration is not finished while it is around. What was left after the service port was the `rca` CLI (`src/cli`), its claude and codex gateways, the plugins and use cases under `src/`, the unit, integration and contract tests of those, the gateway fixtures, the TypeScript build and lint configs, the deploy script, and the dependencies only they used. All of it is deleted: `backend/remote-chat-agent` now holds its Rust crates, the Dockerfile and compose files, the black-box, e2e and benchmark suites (TypeScript test code that drives the real binary over a socket, which is what the tier is), and the fixtures those suites replay. Together with the three mock packages and the two contracts packages below, the change removes 21,357 lines and adds 283 (195 files, `git diff --shortstat 11bd70ac6 7805f81f1`).

What moved or went with it:

- The ladder file and the pricing file moved to `clis/rca-core/data/` beside the crate that embeds the ladder; `wf`, the reporting service and the three Dockerfiles read them there.
- `backend/remote-chat-agent-mock`, `backend/leetcode-backend-mock`, `backend/leetcode-auth-cookie-capture-mock` (Prism servers over a generated OpenAPI document) and `backend/leetcode-backend-contracts`, `backend/leetcode-auth-cookie-capture-contracts` had no consumer and are removed. `backend/remote-chat-agent-contracts` stays: `features/coding-agent` in the app imports it in 39 places. Its OpenAPI builder (`src/openapi.ts`, the `zod-openapi` dependency) had no consumer and is gone; the comments that pointed at TypeScript route files point at `rs/src/api/routes/*.rs`.
- `wf work` runs its implementer through `wf dispatch` (a child process in its own process group, killed as a group on timeout) and `wf visual` runs its judges through a pinned `wf dispatch`; neither starts Node.
- The suites' own type errors, latent while they were excluded from the package's compile, are fixed (a `Buffer` request body, a `Blob` built from a `Buffer`, `.js` extensions in imports, the `jszip` types).

Deliberate behaviour changes (library wins, deliberate, owner-approved by the instruction above):

- `wf work` and `wf visual` can no longer use the claude or codex gateways. A unit that pinned one now pins an opencode model; the pinned model reaches `wf dispatch` as `--model`, the shape as `--shape`.
- `wf dispatch` removes `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` from the opencode child's environment. The repository's rule is that opencode never holds an Anthropic credential of any kind; the TypeScript gateway selection used to enforce it and nothing enforced it in Rust until a black-box test written first showed the key reaching the child (red), then the strip (green).

Tests, by tier. Integration (black box, real binaries): the `wf` suite 1,186 of 1,186, no skips; the remote-chat-agent black-box suite and the leetcode-backend suite against the release binaries, no skips. Unit: `cargo test` and `cargo clippy --workspace --all-targets -D warnings` clean. E2E and runbook: see the VM section of the decision log entry of the same date.

## Folder layout in the layers (2026-10-09, owner request)

`rs/src` held thirteen modules beside `api` and `infrastructure` and `core/src` an `ids.rs`; all now sit in a layer: `domain` (`clis`, `models`, `schema`, `md`, `util`, `voice_dsl`; core's `ids`), `application` (`attachments`, `runs`, `edit_stats`, `one_shot`, `voice_planner`, `startup`), `infrastructure` (`log`, `highlight`, `tamil`, `stt`). `truthy` moved from `api::routes` to `domain::util` so `infrastructure` no longer imports `api`. Pure moves and path rewrites: clippy clean, `cargo test --workspace` 229 in 18 suites, black box on the deploy-profile binary 314 of 314, image on the VM at `323b2a3d1` with WebSocket 7 of 7, convert and voice probes passing.

Deliberate behaviour changes: none (module paths only; routes, wire and logs unchanged).

Deviations: six `application` modules and fourteen `api` files still use adapters directly instead of ports; they are the `ignores` of the layer rules and `docs/plans/rca-hexagonal/plan.md`.

