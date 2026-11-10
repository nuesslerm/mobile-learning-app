# Port `remote-chat-agent` (rca) to Rust, library first, then the service

## Context
rca is two things, which is why it comes after `leetcode-backend` and why it blocks so much of `wf`:
- **A library.** `clis/workflow-cli` imports `@app/remote-chat-agent` (and `/run`, `/stats`) for the rung ladder, pool cooldowns, run plugins and the opencode runner. That seam is what keeps `dispatch`, `work`, `verify`, `knowledge`, `runbook` and `report` on the TypeScript.
- **A service.** 21.6k source lines, 95 test files: Express routes (`chat`, `chatCompat`, `runs`, `diff`, `vcs`, `sourceControl`, `worktree`, `convert`, `filePreview`, `voiceEdit`, `tamilCheck`, `codeAssist`, `control`, `question`, `plan`, `editStats`), a WebSocket layer, an event-sourced orchestration engine (`decider`, `projector`, `reactor`, built on `effect`), provider gateways (Claude, Codex, opencode, GitHub), file and memory thread stores, and media handling.

By size: `data` 6.6k, `routes` 4.3k, `lib` 2.4k, `orchestration` 2.4k, `plugins` 1.2k, `di` 1.1k, `contracts` 0.9k. Heavy runtime dependencies: `playwright`, `sharp`, `ffmpeg-static`, `tesseract.js`, `whisper-node`, `pdf-parse`, `pdfjs-dist`, `mammoth`, `shiki`, `diff2html`, `effect`, `@opencode-ai/sdk`. Those are where the memory is. The media surface is only about 1.2k lines, `playwright`, `diff2html` and `pdf-parse` are not imported anywhere in `src`, and the evaluation of 2026-10-08 (decision log) found a Rust or native-binary replacement for every capability that passes a fidelity test.

## Status (2026-10-08, end of day): done, deployed, Node removed
Both halves are Rust and the VM runs the service as `rca-service` (`backend/remote-chat-agent/rs`), deployed through `wf ship` and again by the `dev` pipeline (`sha-0dd11c76`, then the deploy leg of run 300). The container has one process, the Rust binary. Evidence, numbers and every deviation are in `docs/audits/reports/backend-remote-chat-agent.md` (one report per changed package sits beside it) and in the decision log.

Where it departed from this plan:
- **No strangler front.** The black-box suite (15 files, 304 tests, written against the Node bundle first and run unchanged on Rust) was the oracle, so the service was replaced in slices behind the suite and cut over once. `clis/rca-edge`, the front built for the strangler, was never started by any container and is retired.
- **Media is Rust or a spawned binary, as decided**: `resvg`/`image`, `tree-sitter-highlight`, `zip` + `quick-xml`, `pdftotext`, `tesseract`, a checksum-pinned static `ffmpeg`, whisper.cpp, `curl` for the sidecar and Groq, a multipart reader in the crate. The `pdf-extract` fallback is not shipped. No Node worker exists.
- **`claude` and `codex` are not ported.** The production image was built with `AGENT_CLIS=opencode`, which prunes both, so the deployed Node service served opencode alone; the Rust service refuses them by name. The gateways stay in the TypeScript `rca` CLI that `wf offload plan` and `wf check --visual` still call, so the package keeps 8,089 lines of TypeScript (the CLI and its tests) and 6 direct dependencies; the other 14,021 lines of server code, 59 test files and 16 dependencies are gone.
- **Production parity was added after the routes**: the vault lookup, BetterStack log and span export, the startup check and the heartbeat, which the black-box suite cannot see.
- **Debian's `ffmpeg` made the first image 209 MB larger than Node's**; the static build made it 67 MB smaller (837,936,988 against 904,610,891 bytes).

Numbers (n=5 medians on one harness; the VM figures are containers on arm64, 3 trials):

| | Node | Rust |
| --- | --- | --- |
| Cold start to `/health` | 503.4 ms | 32.4 ms |
| Idle resident set | 133.6 MB | 6.5 MB |
| Peak resident set after every media path ran | 491.6 MB | 43.4 MB |
| Tamil check p50 | 479.9 ms | 156.8 ms |
| DOCX conversion p50 | 61.7 ms | 1.1 ms |
| Container start to first answered `/health` (VM) | 1,183 ms | 307 ms |
| Container idle resident set (VM) | 131.4 MB | 5.1 MB |
| Image, unpacked (VM) | 904.6 MB | 837.9 MB |

Open: twelve `docs/gaps/rca-*` records (eight describe Node defects the Rust service fixes and tests; a gap closes only through `/cf-compound`), callers of `POST /api/diff` outside this repository must now send `X-Bot-Secret`, and moving `wf offload plan` onto `rca-core` plus porting the two gateways would remove the TypeScript package altogether.

## Strategy
1. **Library first.** Port the surface `wf` imports into a Rust crate `rca-core`: `plugins/ladder` (reads `model-ladders.json`, the rotation and escalation rules), `plugins/cooldowns` and the pool ledger (`.agents/tmp/pool-cooldowns.json`, learned reset times), `plugins/ledger`, `runWithPlugins`, the opencode run driver (spawns the `opencode` CLI; the mock is `libraries/hook-runtime/mock-opencode-bin`), and stats. Then the blocked `wf` groups can go native on top of it, which is where the CLI half of the migration ends.
2. **Service by strangler front**, exactly as in `leetcode-backend.md`: a Rust binary owns the port and proxies unported routes, including WebSocket upgrades, to the Node process.
3. **Media in Rust, no Node worker (decided 2026-10-08, owner: Rust first).** `sharp` becomes `resvg` + `tiny-skia` + `image` (0.69% of pixels differ on the Tamil check picture), `shiki` becomes `tree-sitter-highlight` with compiled-in grammars, `mammoth` becomes `zip` + `quick-xml` (byte-identical), PDF text becomes `pdftotext` spawned from Rust (exact on Tamil and ligatures, where `pdfjs` mangles them) with `pdf-extract` as the fallback, OCR becomes the `tesseract` binary (identical text, 37 MB against 136 MB), speech to text keeps its whisper.cpp binary, ffmpeg, sidecar and Groq paths called from Rust, and `busboy` becomes a small multipart parser. `playwright`, `diff2html` and `pdf-parse` are dropped. An on-demand Node worker is kept only for a route whose Rust output fails a fidelity test; none has so far. Numbers: `docs/audits/2026-09-02-token-diet-decision-log.md`, entry "remote-chat-agent media evaluation".

## Phases (each its own PR off `origin/dev`)
0. **Baseline.** Idle RSS and RSS under a scripted chat session; import-time RSS per dependency (extend `wf bench cli --deps` to the service's packages); per-route latency; image size. Decision log first.
1. **Black-box harness for the service.** Start the service as a process with `opencode` replaced by the mock binary, the provider gateways pointed at local mocks, and drive HTTP and WebSocket from the tests. The 95 existing tests are sorted: delete those a black-box test subsumes, keep pure-logic ones as Rust unit tests when their code is ported.
2. **`rca-core` crate** (ladder, cooldowns, ledger, run plugins, stats) with golden tests against the TypeScript output for the same ledger files and ladders. Repoint `wf offload ask --stats`, `work` and `status`.
3. **`wf` groups unblocked by `rca-core`:** `dispatch`, `work`, `knowledge`, `verify`, `runbook`, `report`, then the OTLP-dependent `log`.
4. **Service skeleton and front proxy** (HTTP, WebSocket passthrough, auth via `vm-request-auth-rs`, rate limit, body limits).
5. **Small routes first:** `control`, `question`, `plan`, `runs`, `editStats`, `diff`, `vcs`, `sourceControl`, `worktree`.
6. **Orchestration and stores.** Port `decider`, `projector`, `reactor` and the engine as plain Rust state machines with the same event shapes. The hard constraint is on-disk compatibility: `fileThreadStore` data written by the TypeScript must load in Rust and the other way round during the transition, tested with recorded thread files.
7. **Chat and WebSocket streaming** (`chat`, `chatCompat`, `ws`), then the provider gateways (opencode, Claude, Codex, GitHub).
8. **Media in Rust.** Port the convert, file preview, diff viewer, Tamil check and voice routes onto the crates and binaries above, each behind a black-box slice written first; add `pdftotext`, `tesseract` (with its English data) and `ffmpeg` to the image; measure the loaded resident set with media requests in flight.
9. **Cutover and re-benchmark.** Remove the Node resident process; update the Dockerfile and `wf ship --changed` legs (rca's leg also lists `clis/workflow-cli/bundle` and `redeploy-app.sh` as extra paths, which change when `wf` is Rust).

## Verification
Same tier order as the other plans: runbook checks against the deployed service, black-box through the real socket (including a WebSocket conversation), contract validation against `@app/remote-chat-agent-contracts`, then Rust unit tests for pure logic. A change that touches the run path is exercised with the mock opencode first, then one real dispatch through the ladder. Benchmarks per slice, n=1 smoke then n=5, logged in the decision log.

## Risks
- Event-sourced engine semantics (ordering, idempotent replay) are easy to get subtly wrong; the recorded-thread replay tests are the safety net.
- `effect` structures much of the TypeScript; a literal translation will be awkward. Port behaviour, not structure.
- Secrets and provider tokens flow through `@cli/secrets`; `secrets-rs` must exist (extracted from `clis/wf/src/secrets`) before the gateways.
- The runner is what executes every delegated task in this repo: a regression in `rca-core` stalls the delegation workflow itself. Land it behind the same fail-closed rules as the hook-called `wf` commands, and keep the TypeScript runner available until a full `wf offload plan` plan has run on the Rust one.

## Decisions needed
1. ~~Accept the on-demand Node media worker, or replace each media dependency first.~~ Answered 2026-10-08: replace first, Rust wherever it passes (see strategy step 3); a Node worker only as a tested exception.
2. Whether `rca-core` lands before or alongside the `leetcode-backend` cutover (proposed: after phase 2 of that plan proves the strangler pattern).
3. The shared crate layout (`libraries/*-rs`) and the cargo-in-Docker build on the VM, as in the `leetcode-backend` plan.

## Done when
`wf --help` and every `wf` group run without the TypeScript fallback, the service container holds no resident Node process, the black-box and runbook checks pass against the deployed Rust service, and the before/after table (idle and loaded RSS, cold start, latency, image size) is in the decision log.
