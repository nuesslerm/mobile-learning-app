# T3 parity

Status: consolidated 2026-11-02 from the former `t3-code` plan (npm T3 inside agent-shell, rca beside it; superseded) and the `t3-compat` plan (T3-compatible Rust server on rca as the execution host). Wire facts: [protocol.md](./protocol.md), written against T3 Code `v0.1.6` (commit `2359643c`). Real-server recordings and the image estimate: [phase0-report.md](./phase0-report.md). Section numbers cited by those two files point here.

Goal: the T3 Code iOS app pairs with a server of ours (`wf t3 serve`) and everything the app uses works, with rca (`backend/remote-chat-agent`) as the host of every execution. Direction history: the first spike kept the npm T3 (`t3code-serve.sh`, port 3773, pinned 0.0.35) and added a `wf rca` adapter; the owner then chose full parity against `v0.1.6` with our own server. The npm T3 on 3773 stays until phase 10; the adapter idea is dropped.

> **Retired tooling (mimic X5, 6 Nov 2026).** `tooling/t3-conformance` keeps only `t3-pin.json`, `allowlist.json` and `goldens/`. The recorder, scripted client, mock server, normaliser, differ and `t3-conformance-live` named below, and `clis/t3-serve/tests/flow/{conformance.rs,support/conformance.rs}`, are replaced by `wf mimic` (cassette set `clis/t3-serve/cassettes/t3-v0.1.6`, driver `tooling/mimic-drivers/src/t3`, replay `clis/t3-serve/tests/flow/mimic_t3.rs`); see `docs/plans/mimic/plan.md` X5.

## 1. Status

| Phase | State | What exists, and at which tier |
| --- | --- | --- |
| 0. Capture | DONE | Goldens recorded from the real `t3@0.1.6` in `tooling/t3-conformance/goldens/v0.1.6/` (descriptor, 426 gate, auth, `rpc-phase1` and its variant order, `rpc-errors` with the full 54 KB `ServerConfig`); recorder, scripted client, mock server, 12 unit tests (`tooling/t3-conformance/src/*.ts`, `test/tooling.test.ts`). Protocol corrections in phase0-report.md section 5. |
| 1. Pair and list | DONE at flow tier | `wf t3 serve` and `wf t3 pair` (`clis/t3-serve`: crates `contracts`, `core`, `store`, `http`, root with `verbs.rs`, `wiring.rs`, thin binary `t3-serve`): descriptor, `/oauth/token`, session, ticket, the `/ws` gate (426), config, probe, lifecycle, shell and thread streams, thread and project lifecycle commands, SQLite with one global sequence and hashed secrets. Every other RPC answers a declared `UnsupportedOperationError`. |
| Conformance replay | DONE at flow tier | `clis/t3-serve/tests/flow/conformance.rs` replays the goldens through the binary and diffs after normalization: 519 differences down to 0, the remainder declared in `IGNORED` (12 entries), `OMITTED`, `SHAPE_ONLY` with reasons. |
| 2. First turns | DONE at flow tier with doubles | Message, file and turn RPCs in `clis/t3-serve`; rca `/exec/v1` (`backend/remote-chat-agent/core/src/exec/{domain,ports,service}.rs`, routes `rs/src/api/routes/exec.rs`, flow test `rs/tests/flow/exec.rs`): sessions, turns, NDJSON neutral events, a process port with a fake provider. protocol.md section 14 lists what exists. |
| 3 to 10 | NOT STARTED | Section 8. |

Owner decisions already recorded (1 Nov), all binding:

| Question | Decision |
| --- | --- |
| Claude Code in rca | Option B: installed in rca, isolated (own `CLAUDE_CONFIG_DIR` outside the shared home, env allowlist for the child, no per-instance `ANTHROPIC_API_KEY`). Policy text of `infra/oracle-cloud/CLAUDE.md` and rca's startup check get amended in phase 3. `claude-max` ladder rungs stay disabled. |
| Where the front runs | New service in rca's compose project, sharing the rca tailscale sidecar netns, port 3774. |
| Meaning of parity | Everything the iOS app uses is real; the rest is declared-unsupported (well-formed declared error, capability flag absent). |
| Image budget | +6 min cold build, +600 MB; cached provider stage, warm builds unchanged. |
| Providers first | Claude Code, Codex, OpenCode; cursor, grok, pi, muse, fx, antigravity later as managed downloads. |
| iOS auto-update | Stays on. The pin cannot freeze the phone (section 7). |
| Out of scope | Push notifications, Live Activities, Android app. |
| Terms and branding | Accepted for a private setup: tailnet only, no redistribution of T3's bundle. |

NOT DONE and UNVERIFIED, said plainly:

- Message, file and turn RPC shapes in `clis/t3-serve/contracts` are our own design. They were never recorded from a real turn (phase 0 had no authenticated provider); the contracts file marks them "recorded later". Only connect, auth, shell, thread lifecycle, project create and the error path are backed by goldens.
- Provider CLI flags (`claude -p --output-format stream-json`, `codex exec --json`, `opencode run --format json`) are unverified against the real CLIs; only the fake provider has run.
- `wf t3` has never run against a real rca: flow tests use doubles; no deployment verification exists.
- The provider CLI image stage (`providers.lock.json`, Dockerfile) is not started; no image was built or measured (the phase 0 numbers are estimates, no docker was available).
- The iPhone pairing runbook (section 9) needs the owner; the phone has never touched our server. The app's version is unread.
- The terms of pointing the store app at a third-party server (Apple, T3 Tools) and of a Claude subscription token behind a phone-reachable harness are unread.
- Whether the real launcher skips its download when `runtime/versions/<v>` is pre-populated is unverified (step 1 of section 7 proves it).
- No arm64 run of the real T3 exists (the recording host was x86_64).

## 2. Findings that still shape the plan

1. Orchestration V2 (runs, attempts, nodes, turn items, runtime requests, checkpoint scopes, lineage); the contract file is 4246 lines. `t3-core` carries our own V2 decider and projector.
2. Hard gate: descriptor `orchestrationProtocolVersion: 2` and `/ws?orchestrationProtocol=2`, else 426 (protocol.md 8).
3. Handshake: descriptor, token, ticket, socket with the protocol query, `subscribeServerConfig` first (its `environmentId` must match the descriptor), then `server.probe`.
4. Surface: 208 RPC group entries, 27 streams, 55 commands, 45 events, 26 turn item types, 10 drivers, 17 scopes, 56 capability keys (recorded). Cloud-only and desktop-only parts are answered with declared errors.
5. V2 compaction options are opt-in, so a server that never uses them is valid; capability flags hide unbuilt features.
6. rca's mobile `/ws` is a plain JSON envelope borrowing T3 names, not T3-shaped. Its `Backend` port is the execution seam; the ladder pools `claude-max`, `openai`, `gemini`, `codex` are `enabled: false` today (`libraries/rca-core/plugins/ladder/data/model-ladders.json`), so no Anthropic or Codex rung runs anywhere.
7. `HOME=/workspace/home` is shared by opencode, rca and agent-shell and already holds `~/.claude` and `~/.codex`; provider state must not live there (section 5.3).
8. Shared volume: one uid 1001, `agent-workspace` (decision in `docs/solutions/infra/sharing-a-docker-volume-across-containers-needs-one-uid.md`). The compose files name it `workspace` (rca project) and `agent-workspace` (agent-shell); check at implementation time that both resolve to one Docker volume. New state under `/workspace/tools/t3-compat/{state.sqlite,web,logs,managed}`; never two `t3-serve` on one state directory; rca's worktree TTL cleanup must skip a path a T3 thread names as `worktreePath` (lease route, phase 6).

## 3. Parity checklist

Targets: **real** (matches goldens), **stub** (valid minimal answer, schema-checked), **declared** (declared error, flag absent). The per-tag file is to be generated: `wf t3 conformance coverage` reads the pinned contracts (`WS_METHODS`, `ORCHESTRATION_V2_WS_METHODS`, command, event, turn item unions) and writes `clis/t3-serve/tests/coverage.json` rows `{tag, kind, target, phase, test, status}`; a unit test fails when file and contracts disagree (not built; the generator settles 209 hand-counted constants against 208 group entries). Proving tests: G golden replay, S schema decode with T3's contracts, O oracle client, U unit, E browser e2e, R iPhone runbook, D deployment verification.

| Group | Tags (summary) | Target | Phase |
| --- | --- | --- | --- |
| Connect, auth access | `server.probe`, `getConfig`, `subscribeServerConfig`, `subscribeServerLifecycle`, `subscribeAuthAccess`, revoke routes | real | 1 (done, flow) |
| Orchestration reads and writes | `subscribeShell`, `subscribeThread`, `getThreadProjection`, `dispatchCommand`, `launchThread`, `stopWorkflow`, `forceStopThread`, search RPCs, `getTurnItem`, `subscribeCommandOutput` | real | 1 (shell, thread, dispatch lifecycle), 2 (turns), 5 (rest) |
| Diffs, review | `getTurnDiff`, `getFullThreadDiff`, `review.*` | real | 4 |
| Projects, files | `projects.mutate|createNew|createFromThread|ensureScratch`, `listEntries|readFile|writeFile|search*`, `filesystem.*` | real | 1 (`mutate`), 4 |
| Terminals | `terminal.open|attach|observe|write|resize|clear|close`, `subscribeTerminalMetadata` | real | 4 |
| VCS, git, clone, source control | `vcs.*`, `git.runStackedAction|resolvePullRequest|preparePullRequestThread`, `projectClone.*`, `sourceControl.*` | real (headless gh); sign-in flows declared | 4, 6 |
| Pull requests | 35 `pullRequests.*` | real via `gh`; capability advertised only when all pass | 8 |
| Attachments, assets, native sessions | `attachments.*`, `assets.*`, `agentSessions.scan|import` | real | 3, 5 |
| Settings, providers, usage, scheduler | `server.updateSettings`, keybindings, `provider.*`, `scheduledTasks.*`, usage, storage, diagnostics | real or stub first | 7 |
| Peer links, MCP apps | `peerLinks.*`, `mcpApps.*` | stub then real | 9 |
| Preview, devices, relay, self-update, open-in-editor | `preview.*`, `device.*`, `cloud.*`, `server.updateServer*`, `shell.openInEditor` | declared | 1 (as errors) |
| Commands | 23 thread lifecycle, run commands (`message.dispatch` with 5 modes, `run.interrupt`, queue, steer), runtime requests, checkpoint rollback, lineage (`thread.fork`, `merge_back`, `provider.switch`), delegation, PR link | real | 1 (create, archive, unarchive, delete, metadata.update), 2 (dispatch, interrupt), 3 to 9 |
| Events, turn items | 23 thread events, 22 execution events, 26 turn item types (each emitted only when a driver can produce it) | real | follows its command |
| HTTP | descriptor, session, `/oauth/token`, ticket, pairing, revokes, orchestration and project snapshots, upload and asset routes | real first nine; MCP OAuth stub; connect and preview stream declared | 1 to 9 |

Exact tag lists live in the pinned contracts (`orchestrationV2.ts:2877-3348` commands, `1851-1995` events, `rpc.ts:407-613` RPCs at `v0.1.6`).

## 4. Architecture

Hexagonal, one pure core, ports owned by the core, one composition root (`CLAUDE.md`, "Code architecture").

| Crate (under `clis/t3-serve`) | Layer | Owns |
| --- | --- | --- |
| `contracts` | pure | wire types, Effect RPC codec, ack pacing, RPC-to-scope table, capability list (`wire.rs`) |
| `core` | domain and application | V2 decider and projector, pairing, sessions, tickets, command engine with receipts, resume-gap rule; ports `EventStore`, `AuthStore`, `SettingsStore`, `AgentApi` (was `ExecutionHost`, replaced in rca-parity M3), `Clock`, `Ids` |
| `store` | adapter | SQLite, WAL, global sequence, receipts |
| `http` | driving adapter | axum routes, `/ws` with the protocol gate, CORS, web bundle, signed upload URLs |
| root `t3` | composition root | verbs `serve`, `pair`, later `doctor`, `conformance record|replay|check|coverage`; thin binary `t3-serve`; `adapters/` holds the rca client |

Processes: `t3-serve serve` (protocol front only: HTTP, WS, auth, SQLite, projections) as a new compose service in the rca project beside the tailscale sidecar; rca (`:8787`) hosts provider CLIs, ptys, git, files, checkpoints; the opencode container stays as is; agent-shell is operator and client (`wf t3 pair|doctor|conformance`). Auth: T3's own flow (protocol.md 3), hashed rows, `wf t3 pair [--ttl --label --scope]` prints URL and QR as text, narrowest default scopes (the standard set includes `settings:write`, `providers:manage`, `environment:maintain`, `access:write`); tailnet only, never LAN or Funnel; plain `http://host:3774` is accepted by the app; front-to-rca uses `CHAT_AGENT_API_SECRET` via `wf secret env`, never on a command line. No registry entry added so far; candidate `T3_COMPAT_SIGNING_KEY` otherwise generated into the state dir, mode 0600.

### 4.1 rca as the execution host (`/exec/v1`, strict auth like `/runs`)

The mobile contract of rca (`/ws`, `/api/chat`, `/runs`) stays frozen under `backend/remote-chat-agent/tests/blackbox`; everything here is additive. Built (flow tier): sessions, turns, `GET /sessions/{thread}/events?after=` NDJSON with a cursor, interrupt; `cwd` and `root` must lie under `RCA_EXEC_ROOT` (default `/workspace`), `..` and symlink escapes give 400. Not built: `GET /catalog` of real CLIs, `/respond`, `/steer`, terminals (pty with replay buffer), `/fs/*`, `/git/*`, `/checkpoints/*`, `/providers/{driver}/install|login|logout|status`, `POST /leases`, signed attachment tokens, the `gh` calls behind the PR RPCs. rca never emits T3 payloads: provider JSON lines become neutral events in rca's core and `adapters/` maps them to V2 turn items.

## 5. Provider CLIs and the Claude credential

### 5.1 What goes where

| Driver | Where | Size | Notes |
| --- | --- | --- | --- |
| `opencode` | existing container, rca dials `OPENCODE_SERVER_URL` | existing | no change; baking a third copy not recommended |
| `codex` | image or managed (owner's call) | 164 MB compressed, 409 MB unpacked (npm 0.166.0, arm64, estimate); T3's own asset v0.156.1 is 137 MB | `CODEX_HOME` shared today; ChatGPT login via pasted-URL flow |
| `claudeAgent` | image | 152 MB compressed, 328 MB unpacked (2.1.316, estimate) | option B isolation below |
| `pi`, `cursor`, `grok`, `fx`, `muse`, `antigravity`, `acpRegistry` | managed on the volume, pinned by version and sha256, never in the image | unknown | `pi` needs Node >= 22.19 (image has 22.14.0); arm64 builds of several unverified; ACP agents are third-party code: allowlist |

Claude plus Codex adds about 737 MB unpacked, 137 MB over the 600 MB budget: either alone fits. Decision still to take in phase 3: bake `claude`, keep `codex` managed (recommended), or accept the overshoot.

### 5.2 Dockerfile plan (written, not applied)

1. New `backend/remote-chat-agent/providers.lock.json`: per CLI `{version, url per arch, sha256 or npm dist.integrity}`; the only input of the stage, so it is part of the `wf ship` image input hash by construction.
2. One stage per CLI (`providers-claude`, `providers-codex`), `FROM base`, before the cargo-chef stages, independent of Rust sources: fetch the registry tarball with curl, verify, extract only the platform binary (select by `uname -m` as the ffmpeg stage does), download and extract in one `RUN`.
3. Runtime: `COPY --from=providers-<name> --chmod=755 /opt/providers/<name> ...` above the whisper and service-binary copies, symlinks in a small layer, `ENV PATH=/opt/providers/bin:$PATH`, `ENV RCA_EXEC_ROOT=/workspace`; never `chown -R`.
4. Budget: cold stage under 6 minutes, image growth under 600 MB, measured at the first build; over either, `claude` moves to managed. The existing `mode=max` cache export also pushes the provider layers (about 0.32 GB more).
5. Verification is deployment tier, not flow: each CLI answers `--version` on arm64 in the built image. Until then the catalog reports `installed:false` and a turn answers 400 "could not start: not found".

### 5.3 Claude credential (option B, decided)

Facts: policy "opencode never holds an Anthropic credential of any kind" (`infra/oracle-cloud/CLAUDE.md:222-242`); rca serves opencode alone and its startup refuses `AGENT_CLI` of `claude` or `codex`; the registry has an optional `CLAUDE_CODE_OAUTH_TOKEN` (account-wide, unscopeable); T3's Claude driver uses the CLI's own login or per-instance `ANTHROPIC_*` variables. Rule under B: `claude` runs as a child of rca only, with `CLAUDE_CONFIG_DIR` on its own volume mounted into rca alone and outside the shared home; the child env is an allowlist, the token is injected into that child only, instance settings carrying `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` or `ANTHROPIC_BASE_URL` are rejected by validation; the opencode child env stays scrubbed. Test (phase 3 gate): the opencode and `claude` child environments and mounts never overlap. The same isolation question applies to `codex` (`~/.codex/auth.json`). Whether opencode reads `~/.claude` was not checked. Rejected options: A no Claude, C separate `rca-claude` service, D Claude in agent-shell, E reading of the rule alone.

## 6. Conformance as built

### 6.1 Recorder, goldens, replay

`tooling/t3-conformance`: a transparent HTTP and WebSocket recording proxy (`src/proxy.ts`, frame codec `wsframe.ts`, Effect RPC framing `rpc.ts`, scripted session `session.ts`, `normalize.ts`, `golden.ts`, offline `mock-server.ts`, CLI `cli.ts` with `record` and `mock`). Goldens are JSONL `{n,t,kind,...}` per scenario plus `manifest.json` (tag, npm version, normalized descriptor, scope). Normalization maps uuids, timestamps, sequences, tokens, tickets, trace ids, absolute paths to placeholders and masks `Authorization` and cookies. The replayer is `clis/t3-serve/tests/flow/conformance.rs`: client frames to our server, normalized diff, allowlist `IGNORED` (with reason), `OMITTED`, `SHAPE_ONLY`; an unlisted difference fails; `T3_CONFORMANCE_REPORT=1` prints all. Known real-server behaviour the replayer must tolerate: shell snapshot and `project.updated` order is racy (`rpc-phase1.variant-order.jsonl`), interrupt repeats an `Interrupt` cause 7 to 9 times, unknown tags and bad payloads are `Die` defects.

Scenarios still to record against a real server: `auth-*` (expiry, scope denial, revoke), `turn-*`, `approval`, `question`, `queue-*`, `steer`, `fork`, `switch`, `rollback`, `turn-diff`, `files-*`, `terminal-*`, `vcs-*`, `git-*`, `clone`, `pr-*` (fake `gh`), `settings-*`, `provider-*`, `scheduled-*`, `diagnostics-*`, `search`.

### 6.2 Schema check and CI cost

Planned, not built: `tooling/t3-schema-check` decodes every server frame with the pinned tag's `@t3tools/contracts` (sparse checkout, effect 4.0.5, Node 24 via `mise`, nothing beyond type-stripped imports executed). Budgets (estimates): replay at most 3 minutes, schema check at most 3, normal push at most +4 minutes on a cache hit, +10 cold; recording against real T3 never in CI today (section 7 changes that).

## 7. Real T3 conformance and tracking the latest stable

### 7.1 Verified facts (checked 2 Nov 2026)

- npm `t3` dist-tags: `latest` 0.1.6 (published 31 Oct), `preview`, `nightly` 0.1.7-nightly (a new one every few hours). 7 stable releases in 16 days: the pin goes stale within days.
- T3's `release.yml`: a plain `x.y.z` tag is stable (`cli_dist_tag latest`, `make_latest true`); nightlies every 30 minutes; previews are manual.
- The iOS App Store app "T3 Code" 2.2.0 (bundle `com.t3tools.t3code`) speaks orchestration protocol 2.
- The app checks the server descriptor's `orchestrationProtocolVersion` by strict equality against its own constant (`client-runtime/src/connection/compatibility.ts`; `packages/contracts` `environment.ts` constant is 2 at `v0.1.6` and on `main`). The server answers HTTP 426 on `/ws` without `orchestrationProtocol=2`.
- There is no semver link between app build and server version, and the app takes OTA updates on every merge to T3's `main` (`mobile-eas-production.yml`). So the app can get ahead of the stable server: "latest stable = what the iOS app talks to" is only partly true.
- On a protocol 3 bump the app says "requires a newer server" and pairing fails until we answer protocol 3 with goldens that pass.

Consequence: two lanes. **Stable lane, blocking**: the highest stable `x.y.z`; our `conformedStable` follows it. **Early-warning lane, non-blocking**: `main` and the nightly, reading only the protocol constant, the descriptor key set and the contract tags (sparse tag fetch, nothing executed); a change there opens a note in `drift/` before stable ships it.

### 7.2 Design

Pin file `tooling/t3-conformance/t3-pin.json`: `version`, `commit`, `protocol`, `databaseMigration`, per platform (`linux-x64`, `linux-arm64`) tarball `bytes` and `sha256`, npm `integrity`.

Runtime cache: fetch the release tarball `t3-<v>-linux-{x64,arm64}.tar.gz` and `SHA256SUMS`, verify against the pin, store in the build-cache bucket as a new layer `wf cache t3-runtime` (prefix `t3/<version>/`, 60 days, current and previous only; 123 MB tarball, 370 MB unpacked, so extract into a temp dir and delete). Pre-populate `T3CODE_HOME/runtime/versions/<v>` so the launcher downloads nothing (UNVERIFIED that it skips the download).

Run: under `env -i` with loopback bind, a fresh server and home per run (the phase 0 lesson: an inherited environment leaked local Claude configuration into the provider list), pairing via `t3 auth pairing create --json`.

One scripted client (`tooling/t3-conformance/src/live.ts`, extending `session.ts`) runs against real t3 and against `t3-serve`, both through the recording proxy, normalized by the single normalizer, diffed with a shared `tooling/t3-conformance/allowlist.json`. The Rust `IGNORED`, `OMITTED`, `SHAPE_ONLY` move out of `clis/t3-serve/tests/flow/conformance.rs` into that file, read by both the Rust replayer and the TS diff, so one list judges both tiers. The diff emits a typed report: `{from, to, protocol: {before, after}, descriptorKeys: {added, removed}, rpc: {added, removed, changed}, frames: [{scenario, n, path, severity}]}`, severity `none | drift | breaking`.

Goldens become a cache keyed by T3 version: `goldens/<version>/`, recorded when missing, replayed otherwise (`v0.1.6` is the first entry).

Tiers: (1) unit (normalizer, allowlist, report, pin parser); (2) flow replay against committed goldens (exists); (3) integration with the real oracle: new Nx task `t3-conformance-live`, cached by `t3-pin.json`, `src`, `allowlist.json`, `goldens/` and the `clis/t3-serve` crates, added to the pre-push gate stages; (4) deployment verification against the deployed t3 front, plus the real linux-arm64 t3 on the VM (closes the arm64 gap); (5) owner phone runbook (section 9). Extension to message, file and turn RPCs uses a fake opencode provider (`backend/remote-chat-agent/tests/blackbox/fakeOpencode.ts`, T3's opencode driver takes an external `serverUrl`) or T3's `packages/provider-testing` `fakeCli`; one scenario module per commit; a real model turn never in CI.

### 7.3 Tracker

`wf t3 track check|bump` (deterministic, JSON out, no prompts), fired every 6 hours by a Claude Code Remote Routine (pattern of `docs/runbooks/test-report-routine.md`; this repo has no working GitHub cron). Stable rule: the highest plain `x.y.z` git tag equals npm `latest` and, on the VM where `gh` exists, `releases/latest` has `prerelease=false`; otherwise the answer is `pending` and nothing moves.

On a new stable version V: fetch and verify the tarball (7.2); record goldens for V; run the live diff; run the contract diff by sparse tag fetch (nothing executed); write `docs/plans/t3-parity/drift/<V>.md` and `<V>.json`; commit to `dev`; notify the owner through the Better Stack channel. `conformedStable` in `t3-pin.json` advances only when the diff is empty or fully allowlisted; otherwise the pin stays and the report is the work list. Deploy gate: a flag by default, blocking only `breaking` drift, and only for changes touching the `clis/t3-serve` crates or `/exec/v1`.

### 7.4 Ordered landable steps

Each step lands on `dev` alone, with Given/When/Then tests, and says which tier ran.

| # | Step | Tests (Given/When/Then) | Risk |
| --- | --- | --- | --- |
| 1 | Pin file plus `wf cache t3-runtime` and a pre-populated runtime dir | Given the pin and a cold cache, when the layer is fetched, then sha256 matches the pin and a second call downloads nothing; Given a tampered tarball, then it is refused and removed. Runtime check: with `runtime/versions/0.1.6` filled, real t3 starts with the network blocked (proves the launcher skips the download) | launcher may insist on downloading: then serve the tarball from a local release mirror |
| 2 | Shared `allowlist.json`; Rust and TS both read it | Given the allowlist, when the flow replay runs, then still 0 differences; Given an entry removed, then the replay fails naming it | allowlist grows silently: every entry carries a reason and an owning phase |
| 3 | Live harness `live.ts` and Nx task `t3-conformance-live` | Given real t3 and `t3-serve` on loopback, when the scripted client runs through the proxy, then the report is empty or allowlisted; a negative probe (a changed descriptor key) shows `breaking` and proves the request arrived | flaky ordering (known racy shell frames): compare by request id as the normalizer does |
| 4 | `wf t3 track check|bump` | Given a fake npm and git tag source, when `check` runs, then `new-stable`, `pending` or `current` with the next step; Given tag and npm disagreeing, then `pending`; `bump` writes pin and goldens only on an empty diff | npm and GitHub disagree for hours after a release: that is what `pending` is for |
| 5 | Routine every 6 hours | Given a fake new stable, when the Routine fires, then a `drift/<V>.md` and `.json` commit lands on `dev` and one notification is sent, once | a Routine fire that never runs: the report notes the last successful check time |
| 6 | Deployment-verification tier, incl. real linux-arm64 t3 on the VM | Given the deployed front and rca, when the live client runs over the tailnet, then pair, config and shell snapshot match goldens; real arm64 t3 starts from the cached runtime | VM disk and 370 MB per version: current and previous only |
| 7 | Provider-double scenarios (message, file, turn RPCs), one per commit | Given a fake opencode, when a turn is dispatched on real t3 and `t3-serve`, then frame shapes match after normalization; this records the first real turn shapes and corrects our own design | T3 may need extra setup for the external `serverUrl`; fall back to `fakeCli` |
| 8 | Owner runbook | section 9 | needs the owner and the phone |

### 7.5 Open questions of this section, with recommendations

1. Block or flag drift at deploy? Recommend flag by default, block only `breaking` drift on changes touching `clis/t3-serve` or `/exec/v1`.
2. Which iOS build runs on the owner's phone (store 2.2.0, a newer OTA, TestFlight)? Recommend reading it first (section 9, step 1); it decides whether stable or `main` is the lane that matters.
3. A real model turn ever? Recommend once, by hand, by the owner, to validate shapes the doubles cannot; never in CI.
4. Terms of pointing the store app at a third-party server: unread. Recommend the owner reads them before phase 10 flips 3773; private tailnet use meanwhile.
5. An Android emulator as an extra client oracle? Recommend no: Android is out of scope and the CI emulator is for the app's own tests.

## 8. Phases

| Phase | Deliverable | Exit |
| --- | --- | --- |
| 0, 1, 2 | Done (section 1) | flow tier only |
| 3. Codex and Claude, requests | `providers` image stage; `Backend` implementations for `codex` and `claude`; approvals, questions, `runtime-request.respond`, queue and steer; attachments; option B enforced | same flow on three drivers; credential isolation test green; image thresholds met |
| 4. Terminals, files, diffs, checkpoints | rca pty, fs, git read, checkpoints; `terminal.*`, file RPCs, `subscribeVcsStatus`, `getTurnDiff`, `checkpoint.rollback`, `review.*` | terminal usable on phone and web; turn diff equals upstream's golden |
| 5. Lifecycle breadth | settle, snooze, pin, auto-settle, visit, fork, merge_back, `provider.switch`, search, `launchThread`, native session import | every thread command and event real or documented |
| 6. Git write and clones | worktrees, refs, pull, init, stacked actions, clone, worktree lease with rca | worktree-per-thread end to end |
| 7. Settings, scheduler, diagnostics | settings, keybindings, provider management, scheduled tasks, usage | settings round-trip; scheduled task fires a fake turn |
| 8. Pull requests | 35 `pullRequests.*`; capability only when all pass | goldens with a fake `gh` |
| 9. Remaining providers, long tail | managed `cursor`, `grok`, `pi`, `muse`, `fx`, `antigravity`, ACP registry; guest share, peer links, MCP apps | each driver passes against a fake CLI |
| 10. Cutover and drift | second pinned version path, flag-guarded replacement of the npm T3 on 3773, retire `t3code-serve.sh` | owner decision; flag off returns to today without data loss |

Rule for every phase: advertise a capability flag only when its feature passes goldens. Each phase becomes a `units.jsonl` through `/cf-plan`; serde types, RPC stubs and the coverage generator are mechanical (`wf offload plan`); the V2 decider and projector, auth, credential isolation and the rca refactor stay on Claude. The section 7 steps run alongside, step 1 to 3 before phase 3.

## 9. Verification ladder and the owner's phone

Order of importance and of reporting: runbook, browser e2e, deployment verification, integration, flow, unit; a report names the type that ran. Today only flow and unit ran (plus the mock-backed tooling tests); nothing has run at integration, deployment or runbook tier.

| Type | What |
| --- | --- |
| Runbook (owner, phone) | below; recorded; `wf test publish` bundle |
| Browser e2e | T3 web bundle served by `t3-serve` in CI Chromium via `wf browser`: pair, list, open thread, send turn, terminal; rca and providers doubled |
| Android e2e | none planned |
| Deployment verification | real calls over the tailnet to the deployed front and rca: pair, `subscribeServerConfig`, shell snapshot, a thread, one turn on a cheap rung; each installed CLI `--version` |
| Integration | built `t3-serve` and rca binaries as black boxes with fake provider CLIs; section 7 live oracle |
| Flow | goldens replayed through `t3-serve` with in-memory store and rca doubled; wrong pairing code gives 401 `auth_invalid` and the store shows the attempt; no protocol query gives 426; a ticket older than 5 minutes is refused |
| Unit | decider, projector, resume-gap rule, scope table total against the contracts, coverage file equals tag list, pairing alphabet, Exit encoding |

Every `wf t3` verb returns stable JSON, never prompts, carries evidence and the next step; `wf t3 doctor` (not built) reports bound address, rca reachability, store writable, advertised capabilities and the last N unknown RPC tags the phone called.

iPhone runbook (owner; recording per step; the emulator is CI-only and no iOS simulator exists):

1. Read the app version and source (Settings, About; App Store or TestFlight). Equal to `2.2.0` means protocol 2; newer may mean protocol 3 (section 7.1). Turning iOS app updates off during bring-up is optional, the owner decided auto-update stays on.
2. Tailnet reachability: Safari opens `http://<host>:3774/.well-known/t3/environment` and shows `orchestrationProtocolVersion: 2`.
3. Pair by typing host and the 12-character code from `wf t3 pair` (alphabet `23456789ABCDEFGHJKLMNPQRSTUVWXYZ`, 5 minutes), then by QR.
4. List and open a thread created by the oracle client; background and foreground the app.
5. After phases 2 and 3: send a message, watch streaming, interrupt, answer an approval, per driver.
6. After phase 4: terminal, rotate, resize, keyboard rule (focus raises, Done lowers).
7. Each phase: read `wf t3 doctor` and attach the RPC tags the app called that we stubbed. Failure report: app version, exact message, screenshot. Full steps and the healthy connect order (`rpc-phase1.jsonl` n=0..13) in phase0-report.md section 7.

## 10. Terms, rollout, risks

- Licensing: T3 Code is MIT (T3 Tools Inc). The web bundle is built in CI from the pinned tag and not committed; goldens keep the notice beside them. Provider CLIs have their own terms (Claude Code, Cursor, Grok Build, Muse, Antigravity assumed proprietary; Codex and opencode open source upstream, licences to verify). We do not redistribute the image. Name: `t3-serve` is internal; the descriptor `label` is ours. T3's hosted services (app.t3.codes, relay, Clerk) are not used. Terms for the store app against a third-party server and for a subscription token behind a phone-reachable harness: not read.
- Rollout: compose flag `T3_COMPAT_ENABLED` (default false), `T3_COMPAT_PORT=3774`; rca `/exec/v1` is additive and the black-box suite stays green; deploy through the CI legs (`wf ship`), never `/redeploy` by hand.

| Risk | Mitigation |
| --- | --- |
| Store app or `main` raises the protocol; hard gate blocks us | section 7 lanes and tracker; the descriptor advertises only the protocol we pass goldens for |
| Parity is large; some needs T3's cloud or desktop | targets and capability flags; declared list approved |
| Effect wire details differ from the beta reading | goldens at the pinned tag; codec in one crate |
| Our own turn/file/message shapes are wrong | section 7.4 step 7 records real shapes first; nothing advertised until then |
| rca image grows and builds slowly on 2 cores | cached stages, thresholds in 5.2, managed providers |
| Shared `HOME` holds several providers' credentials readable by paired clients | per-provider config dirs, narrow scopes, tailnet only; known exposure |
| Execution moves into rca, which also holds ladder and gh credentials; T3 default runtime mode `full-access` | enforce and log in rca; least-scope pairing |
| Memory on the 12 GB VM (opencode 5 GB cap, gh-runner 8 GB) | small front; `mem_limit` and process cap for ptys and CLIs |
| Real T3 needs Node 24 and network; untrusted upstream source | `mise exec`, throwaway dirs, `env -i`, contracts-only imports, runtime cache, nothing executed from a clone |

## 11. Open questions for the owner

Settled: target version, where tools run, parity meaning, front placement, Claude option B, providers, image budget, auto-update, out-of-scope list, terms. Remaining:

1. Store app version and source (runbook step 1; recommend doing it before phase 3).
2. Which providers beyond Claude Code, Codex and OpenCode have accounts (cursor, grok, pi, muse, fx, antigravity)? Phase 9 builds only those.
3. HTTPS through `tailscale serve` or plain `http://` over the tailnet (recommend plain, the app accepts it).
4. End state: run beside the npm T3 until phase 10 then flip 3773 (recommended) or never both.
5. Ladders as pseudo models `ladder:*` under an `opencode` instance or a custom driver slug (rendering unverified); selectable `claude-opus-5` and `claude-fable-5-1`?
6. `codex` in the image (+409 MB) or managed on the volume (recommended).
7. Section 7.5 questions 1 to 5.
8. Phone time per phase and where recordings live (repo small, verification bucket large; pairing codes redacted).

Superseded `t3-code` open questions that stay relevant to the VM: whether `t3code-serve` runs today on 3773, `/workspace` ownership and `safe.directory` in `/workspace/home/.gitconfig`, tailnet ACL for `:3774`, `:8787`, `:4096`, and whether `CHAT_AGENT_API_SECRET` is set in production (the WS is open when unset).
