# Phase 0 report: recorder, real `t3@0.1.6` goldens, image estimate

Status: done 2026-11-01. Scope: [plan.md](./plan.md) section 8 phase 0 (recorder and goldens, image estimate, store-app runbook). Wire facts being checked: [protocol.md](./protocol.md).

## 1. Verdict

- The real server ran. `t3@0.1.6` is a 7 KB npm launcher that downloads a self-contained executable; it started headless on loopback with no provider credentials, and the phase-1 session was recorded through the recording proxy. Nothing in the goldens is faked or hand-written.
- Goldens: `tooling/t3-conformance/goldens/v0.1.6/` (section 3). The recorder, the scripted client and a mock server are in `tooling/t3-conformance/` with 12 unit tests (Given/When/Then, `node --test`), all green; `tsc --noEmit` and `oxlint --deny-warnings` are clean.
- protocol.md held up on the connect path. It is wrong or silent on the points in section 5; the most important for phase 1: archiving a thread emits `thread.removed`, not an archive-location upsert; unknown tags and undecodable payloads are `Die` defects; `server.getConfig` still exists; the descriptor has 56 capability keys.
- The image estimate (section 6) says baking Claude Code and Codex adds about 737 MB unpacked, over plan.md's own 600 MB threshold. All numbers are estimates: no docker here.

## 2. How the real server was run

| Step | What |
| --- | --- |
| Version | `npm view t3 dist-tags`: `latest` 0.1.6, `preview` 0.1.6-preview.20261030.3948, `nightly` 0.1.7-nightly.20261101.4035. 0.1.6 matches the stable tag in protocol.md. `t3 --version` prints `t3 v0.1.6`. |
| Install | `npm install --prefix <new scratch dir> t3@0.1.6`: 56 KB, no dependencies. The package is `bin/t3.mjs` only (read before running). On first start it fetches `t3-0.1.6-<platform>.tar.gz` from the GitHub release, checks it against the release `SHA256SUMS` and unpacks it into `$T3CODE_HOME/runtime/versions/0.1.6`. Used: `linux-x64`, 123,152,548 bytes, unpacked 370 MB (binary 160 MB, web client, `lib/`, `resource-monitor/`). `linux-arm64` is 118,909,565 bytes. The package's own `t3code.databaseMigration` is 76. |
| Run | `env -i PATH=/usr/bin:/bin HOME=<scratch> T3CODE_HOME=<scratch> node t3.mjs serve --host 127.0.0.1 --port 13773 --no-browser <scratch workspace>`; pairing credentials minted with `t3 auth pairing create --json` (default scope set). |
| Isolation | An empty environment (confirmed from `/proc/<pid>/environ`: PATH, HOME, T3CODE_HOME, T3CODE_NPM_LAUNCHER_PATH only), loopback bind, a throwaway home. A first attempt inherited the sandbox's environment and the server read local Claude configuration into its provider list; those goldens were discarded and everything was re-recorded under the empty environment. The committed goldens were scanned for the sandbox's account values, tokens and paths: none. |
| Cleanup | Server stopped; the install, the T3 home, the workspace and every log (they held pairing codes) deleted. |

Not done: no model turn (no provider is authenticated), no arm64 run (this host is x86_64), no browser or phone client. The real client's request-id generator was not observed on the wire; my client numbers requests from `"1"` and the server echoes them.

## 3. What was recorded

Committed files under `tooling/t3-conformance/goldens/v0.1.6/`. Format: one JSON object per line, `{n, t, kind, ...}`; `t` is milliseconds since the proxy started (diffs ignore it, see `withoutTiming`). `kind` is `http` (method, path, headers, bodies, status), `ws-open`, `ws` (`dir` `c2s`/`s2c`, `opcode`, `frame` the parsed JSON) or `ws-close`.

| File | Lines | Content |
| --- | --- | --- |
| `descriptor.jsonl` | 1 | `GET /.well-known/t3/environment` |
| `protocol-gate-426.jsonl` | 1 | upgrade to `/ws` without `orchestrationProtocol`: HTTP 426 |
| `auth.jsonl` | 5 | token exchange, a second exchange with the spent credential, one with a wrong credential, `GET /api/auth/session`, ticket request |
| `rpc-phase1.jsonl` | 41 | the phase-1 sequence over `/ws?orchestrationProtocol=2&wsTicket=...`: Ping, `subscribeServerConfig`, `server.probe`, `subscribeServerLifecycle`, `orchestration.subscribeShell`, `projects.mutate` (project.create), `orchestration.dispatchCommand` (`thread.create`), `orchestration.subscribeThread`, `thread.archive`, interrupts, close |
| `rpc-phase1.variant-order.jsonl` | 41 | a second fresh-server run of the same session; differs only in frame order (section 5, item 12) |
| `rpc-errors.jsonl` | 12 | unknown tag, `server.getConfig` (the full 54 KB `ServerConfig`, a ready-made fixture), a `dispatchCommand` with a missing key, one with an unknown command type, Eof, close |
| `manifest.json` | | tag, npm version, normalized descriptor, granted scope, per-call facts |

Normalization (`src/normalize.ts`): UUIDs become `<uuid:n>` (stable by first appearance), ISO timestamps `<ts>`, sequences `<seq>`, access tokens, pairing credentials, tickets and CSRF values are replaced by name and as learned secrets everywhere they recur (paths, headers, bodies), trace ids `<trace>`, `expires_in` and fiber ids constants, absolute paths (workspace, T3 home) placeholders, volatile headers dropped, `Authorization` and cookies masked. Raw sequence values were therefore lost; from a probe against the same server: lifecycle `welcome` 1, shell `snapshotSequence` 0 then 1, `project.updated` 1, `thread.updated` 2, the archive 3 (the dispatch results were `{sequence: 2}` and `{sequence: 3}`).

Re-record: `T3_PAIRING_CREDENTIAL=<code> node src/cli.ts record --upstream http://127.0.0.1:13773 --workspace <abs dir> --out goldens/v0.1.6 --path '<t3-home>=<abs>' --tag v0.1.6 --npm-version 0.1.6` against a fresh server (the session creates a fixed project id, so a used database fails the create). Determinism: two fresh-server runs matched line for line except the ordering in item 12.

## 4. The real descriptor (`descriptor.jsonl`, `manifest.json`)

| Field | Value |
| --- | --- |
| `environmentId` | random UUID per database |
| `label` | the host name (`vm` on the recording host; not a fixed string) |
| `platform` | `{os: "linux", arch: "x64"}`; no `machine` key |
| `serverVersion` | `"0.1.6"` (the release stamps it; the repo's `package.json` says 0.1.5 at the tag, as protocol.md inferred) |
| `orchestrationProtocolVersion` | `2` |
| `databaseMigration` | `76` |
| `capabilities` | 56 keys: 54 are `true`, `agentActivityPublishing` is `false`, `fileAttachments` is `{maxUploadBytes: 52428800}`. The full list is in `descriptor.jsonl`. |
| Grant (`POST /oauth/token`) | 200, `token_type: "Bearer"`, `issued_token_type: "urn:ietf:params:oauth:token-type:access_token"`, `expires_in` 2591999 (30 days minus 1 s), `scope` the 16 permissions as one space-separated string (all but `review:write`) |
| `/api/auth/session` scopes | legacy `scopes` (7): `orchestration:read`, `orchestration:operate`, `terminal:operate`, `relay:read`, `access:read`, `access:write`, `relay:write`; `permissions` (16); `sessionMethod: "bearer-access-token"`; `auth`: `{policy: "loopback-browser", bootstrapMethods: ["one-time-token"], sessionMethods: ["browser-session-cookie","bearer-access-token","dpop-access-token"], sessionCookieName: "t3_session_<port>_<hash>", serverUpdateScope: "environment:maintain"}` |

The token exchange form is: `grant_type=urn:ietf:params:oauth:grant-type:token-exchange`, `subject_token=<code>`, `subject_token_type=urn:t3:params:oauth:token-type:environment-bootstrap`, `requested_token_type=urn:ietf:params:oauth:token-type:access_token`, optional `client_label`, `client_device_type`, `client_os` (read from the web client bundle, then confirmed on the wire in `auth.jsonl` n=0).

## 5. Corrections and additions to protocol.md

Citations are `file n=<line number>` in the goldens above.

| # | protocol.md says | Recorded | Where |
| --- | --- | --- | --- |
| 1 | Capability flags "about 60"; real server advertises "nearly all" (section 4) | 56 keys, 54 `true`, one `false`, one object | `descriptor.jsonl` n=0 |
| 2 | Legacy `scopes` is "the subset of 8" (section 3) | 7 for a grant without `review:write`; the 8th is probably `review:write` (infer). The token response `scope` carries the full 16, not the legacy subset. | `auth.jsonl` n=0, n=3 |
| 3 | Pairing single-use enforcement unknown (unknown 7) | A spent credential and a wrong credential both give 401 `{_tag:"EnvironmentAuthInvalidError", code:"auth_invalid", reason:"invalid_credential", traceId}`; the two cases are indistinguishable. Expiry was not tested. | `auth.jsonl` n=1, n=2 |
| 4 | Ticket from `/api/auth/websocket-ticket[/:use]` (infer `rpc`) | `POST /api/auth/websocket-ticket/rpc` with the bearer: 200 `{ticket, expiresAt}`. The ticket is `base64url(JSON {v:1, kind:"websocket", sid, aud:"rpc", iat, exp})` plus `.` plus a signature, valid 300 s (seen in a probe; tickets are scrubbed in the goldens). | `auth.jsonl` n=4 |
| 5 | 426 body `{code, message, orchestrationProtocolVersion}` before auth | Confirmed, sent even with an invalid ticket. `message` is "Update this client to one that supports orchestration protocol 2."; headers `access-control-allow-origin: *`, `content-type: application/json`. | `protocol-gate-426.jsonl` n=0 |
| 6 | Framing "not re-verified at Effect 4.0.5" (unknown 1) | Verified. One JSON object per WebSocket text message (never an array in what was recorded). Client: `{_tag:"Request", id:"<string>", tag, payload, headers:[]}` (the official client also adds `traceId`, `spanId`, `sampled`, per its bundle), `{_tag:"Ack", requestId}`, `{_tag:"Interrupt", requestId}`, `{_tag:"Ping"}`, `{_tag:"Eof"}` (accepted, no reply). Server: `{_tag:"Chunk", requestId, values:[...]}`, `{_tag:"Exit", requestId, exit}`, `{_tag:"Pong"}`. `exit` is `{_tag:"Success", value}` or `{_tag:"Failure", cause:[...]}` where `cause` is a flat array of reasons. `Defect` and `ClientProtocolError` frames were not provoked. | `rpc-phase1.jsonl` n=1..40 |
| 7 | Chunk batching unknown | `values` batches: the lifecycle stream sends `welcome` and `ready` in one Chunk. Every Chunk was acked by the client; ack pacing was not tested without acks. | `rpc-phase1.jsonl` n=9 |
| 8 | (silent) unknown tag, bad payload | Not a typed error: `Exit` `Failure` with `cause:[{_tag:"Die", defect:"<message>"}]`. Unknown tag: `"Unknown request tag: <tag>"`. Missing key: `"Missing key\n  at [\"commandId\"]"`. Unknown command type: the schema's "Expected {...} \| {...}" union text. HTTP-level and socket stay up. | `rpc-errors.jsonl` n=1, n=5, n=7 |
| 9 | (silent) interrupt | After `Interrupt`, each stream ends with `Exit` `Failure` `cause:[{_tag:"Interrupt", fiberId}, ...]` repeated 7 to 9 times for the same fiber id; the count varies, so a replayer must not compare its length. | `rpc-phase1.jsonl` n=37..40 |
| 10 | `server.getConfig` "no longer the first call" (section 4) | It still exists and answers `Success` with the full `ServerConfig` (23 keys: `environment, auth, cwd, keybindingsConfigPath, keybindings, issues, providers, availableEditors, remoteOpenTargets, directEndpoints, observability, settings, shellResumeCompletionMarker, shellThreadFamilies, shellOlderSettled, threadResumeCompletionMarker, threadSnapshotPagination, threadFind, threadFindProgressive, secretProviderSettings, secretReferences, scratchWorkspaceRoot, newProjectsRoot`). `subscribeServerConfig`'s first value is `{version:1, type:"snapshot", config}`. `server.probe` answers `Success` `{}`. | `rpc-errors.jsonl` n=3; `rpc-phase1.jsonl` n=4, n=7 |
| 11 | Lifecycle payloads not specified | `welcome` payload `{environment, cwd, projectName}`; `ready` payload `{at, environment}`; both `{version:1, sequence, type, payload}`. | `rpc-phase1.jsonl` n=9 |
| 12 | Shell stream frames (section 5.4) | First Chunk is `{kind:"snapshot", snapshot:{schemaVersion:2, snapshotSequence, threads, archivedThreads, projects}}` with no `synchronized` and no `olderSettled`. The snapshot after a project exists adds `resolvedRepositoryIdentityRoots`. **Ordering is racy:** after the first project create the server sent an extra `snapshot` and a `project.updated`; in one run the snapshot came first (n=16, n=18), in the other `project.updated` came first and the snapshot after the next command (`variant-order` n=16, n=20). A replayer must accept both. | `rpc-phase1.jsonl` n=12..18; `rpc-phase1.variant-order.jsonl` |
| 13 | Archived threads carry `location: archive` (section 5.4) | Archiving a thread produced `{kind:"thread.removed", sequence, location:"active", threadId}` on the shell stream and no archive-location upsert (that may need `archivedThreads` to be asked for; not tested). The thread stream got `{kind:"event", sequence, event:{id:"event:thread:<threadId>:command:<commandId>:<uuid>", threadId, providerInstanceId, occurredAt, type:"thread.archived", payload:<full thread row>}}`. | `rpc-phase1.jsonl` n=29, n=31 |
| 14 | Result shapes not specified | `projects.mutate` (`project.create`) answers `Success` with the project row (`id, title, workspaceRoot, repositoryIdentity, faviconPath, projectIcon, defaultModelSelection, defaultThreadEnvMode, autoPull, scripts, pinnedAt, archivedAt, createdAt, updatedAt, deletedAt`); `dispatchCommand` answers `{sequence}`. The payloads accepted: `{type:"project.create", commandId, projectId, title, workspaceRoot}`; `{type:"thread.create", commandId, threadId, projectId, title, modelSelection:{instanceId, model}, runtimeMode:"full-access", interactionMode:"default", branch:null, worktreePath:null, createdBy:"user", creationSource:"mobile"}`; `{type:"thread.archive", commandId, threadId}`. The command is the payload itself (no wrapper). | `rpc-phase1.jsonl` n=14..28 |
| 15 | (silent) | `thread.create` accepted `modelSelection.instanceId:"codex"` although codex is not installed: the server does not validate the provider at creation. `runtimeMode` values: `approval-required`, `auto-accept-edits`, `auto`, `full-access`; `interactionMode`: `default`, `plan` (web client bundle). | `rpc-phase1.jsonl` n=20..22 |
| 16 | Provider list shape (section 6.1) | With no credentials and an empty home the config lists 9 instances (`codex, claudeAgent, cursor, grok, opencode, antigravity, fx, muse, pi`; `acpRegistry` is a driver but not an instance). `codex`: enabled, not installed, status `error`. `claudeAgent`: installed, version 2.1.316, status `ready`, auth `{status:"authenticated", type:"Claude API", label:"Claude Api Subscription"}`, 52 built-in slash commands. The other seven: disabled. That the Claude agent reports authenticated with an empty home is unexplained here; do not rely on it. | `rpc-errors.jsonl` n=3 |
| 17 | Pairing token from the server start | The startup banner prints `Connection string`, `Token`, `Pairing URL`, `App link` (`t3code://pair?pairingUrl=...`) and a QR code; `t3 auth pairing create [--scope ...] [--ttl ...] [--label ...] [--base-url ...] [--json]` mints more against the same state directory. | server banner, `t3 auth pairing create --help` |

Unknowns of protocol.md section 12 after this run: 1 closed except the official client's id generator; 3 closed for a default server (`rpc-errors.jsonl` n=3 is the sample); 5 closed (items 1 and 4); 7 partly (expiry untested); 2, 4, 6 and 8 still open (they need the phone, a model turn, and the rest of the scope table).

## 6. Image impact of baking Claude Code, Codex and OpenCode into rca (estimate)

Everything below is an **estimate** from the registry and from the existing `Dockerfile`; nothing was built or measured. Sizes come from `npm view <pkg> dist.unpackedSize` and a byte count of the registry tarball, for the glibc linux-arm64 platform packages (the base image is `node:22.14.0-slim`, Debian, so not the musl builds). The launcher packages (`@anthropic-ai/claude-code` 190 KB, `@openai/codex` 14 KB, `opencode-ai` 8 KB) are negligible; the weight is the platform binary each pulls.

| CLI (version) | Package | Compressed (tarball) | Unpacked | Compression |
| --- | --- | --- | --- | --- |
| Claude Code 2.1.316 | `@anthropic-ai/claude-code-linux-arm64` | 152,572,458 B | 328,460,138 B | 2.2x |
| Codex 0.166.0 | `@openai/codex@0.166.0-linux-arm64` | 164,402,913 B | 408,635,948 B | 2.5x |
| OpenCode 1.19.5 | `opencode-linux-arm64` | 77,922,636 B | 211,209,349 B | 2.7x |
| Claude + Codex | | 316,975,371 B | 737,096,086 B | |
| All three | | 394,898,007 B | 948,305,435 B | |

Notes on the numbers: T3's own managed Codex asset (v0.156.1, protocol.md 6.2) is 136,933,361 B compressed, so the npm tarball of 0.166.0 is 20 percent larger; pin whichever release the T3 front is tested against. OpenCode already runs in its own container in the plan; baking it into rca would add a third copy and is not recommended unless rca spawns it directly.

| Quantity | Estimate | Basis |
| --- | --- | --- |
| Added image size, Claude + Codex | about 0.74 GB unpacked, about 0.32 GB compressed layers | table above |
| Added image size, with OpenCode | about 0.95 GB unpacked | table above |
| Existing rca image | not measured; guess 0.7 to 0.9 GB (Node slim about 0.22 GB, whisper models about 0.22 GB, static ffmpeg about 0.07 GB, tesseract and poppler about 0.1 GB, the service binary tens of MB) | the layers of `backend/remote-chat-agent/Dockerfile`; each figure is a guess |
| Against plan.md 5.2 thresholds (more than 600 MB, or a cold build over 6 minutes, moves `claude` to managed) | Claude + Codex exceeds the size threshold by about 137 MB; either one alone stays under it (328 MB, 409 MB) | arithmetic |
| Cold build time added | about 1 to 3 minutes: downloads (this sandbox fetched 152 MB in 1 s, 164 MB in 17 s, 78 MB in 0.7 s; the VM's link will differ), checksum and unpack of 0.7 GB, then compressing the new layer at tens of MB/s on 2 cores | download timings measured here, the rest guessed |
| Warm build | no change when the lock file is unchanged (layer cache hit, and `wf ship` skips the build when the input hash is unchanged) | plan.md 5.2 |
| Cache export | `mode=max` (`Dockerfile:23-24` per plan.md) would also export the provider layers: about 0.32 GB more to push and to import on a recreated VM | estimate |

Recommendation, unchanged in kind from plan.md 5.1 but now with numbers: bake the two CLIs the T3 front must drive from rca (`claude`, `codex`) only if the owner accepts the 0.74 GB; otherwise bake `claude` and keep `codex` managed on the volume (its 409 MB then costs nothing in the image and a provider bump needs no rebuild). That is a decision for the owner, not a result of this phase.

Cache-friendly layout (a proposal; no Dockerfile was touched):

1. One stage per CLI, each `FROM base`, each independent of the Rust sources and of the other CLIs, so bumping one version rebuilds one stage and one layer: `providers-claude`, `providers-codex` (and `providers-opencode` if ever wanted).
2. Inside a stage: fetch the registry tarball with `curl` (not `npm install -g`), verify it against the `dist.integrity` (sha512) pinned in one new lock file `backend/remote-chat-agent/providers.lock.json` that is the only input the stage copies, extract only the platform binary with `tar -xzf ... --strip-components=1 package/<binary>`, and select the package by `uname -m` as the ffmpeg stage already does. Keep the archive out of the layer (download and extract in one `RUN`).
3. In `runtime`: one `COPY --from=providers-<name> --chmod=755 /opt/providers/<name> /opt/providers/<name>` per CLI, above `COPY --from=whisper` and `COPY --from=rcabuilder` (the service binary stays the last, most frequently changing copy), then symlinks to `/usr/local/bin` in a tiny layer. `COPY --chmod` avoids the doubled layer a later `chmod -R` would create. Never `chown -R` the trees.
4. Order by change frequency: apt layer, providers (change with the lock file), whisper and ffmpeg (rare), `wf` symlink and entrypoint, the service binary.
5. Verification stays as plan.md 5.2: a deployment-verification probe runs `claude --version` and `codex --version` on arm64 in the built image.

## 7. Runbook: read the iPhone app's version and point it at the phase-1 server

For the owner, from the phone; about five minutes. This is plan.md section 11 Q1 and Q2.

Read the version (before anything else, because the protocol gate is hard):

1. On the iPhone: open the T3 Code app, then Settings, then About. Write down the version. If there is no About entry: App Store, tap your picture, scroll to the app, and read the version under "What's New"; in TestFlight the version and build are on the app's page.
2. Say whether it came from the App Store or TestFlight (TestFlight apps show an orange dot on the home screen icon).
3. Compare: the pinned source is `apps/mobile` at tag `v0.1.6`, default version `2.2.0` (`apps/mobile/app.config.ts:231`, protocol.md section 10). Equal to `2.2.0`: protocol 2, our target. A higher number: the protocol may already be 3, and the app will show "client not supported" against a protocol-2 server. A lower number: it may speak protocol 1 and show "requires a newer server".
4. Optional, for bring-up: iOS Settings, App Store, switch App Updates off, so the app does not move to a new protocol under us.

Point it at the server (phase 1; the commands below exist only once phase 1 ships):

1. The iPhone and the machine running the server must both be on the tailnet (Tailscale app on, signed in).
2. On a tailnet machine run `wf t3 pair`; it prints a 12-character code (alphabet `23456789ABCDEFGHJKLMNPQRSTUVWXYZ`, valid 5 minutes) and the host.
3. Reachability check first: in Safari on the phone open `http://<host>:3774/.well-known/t3/environment`. Expect JSON with `"orchestrationProtocolVersion":2` and `"serverVersion"`. If this does not load, the problem is the network, not the app.
4. In the app: add a connection (the "+" or "Add environment" entry), choose host plus code, enter `http://<host>:3774` and the code. Plain HTTP is accepted by the app (ATS arbitrary loads, unchanged since 0.0.35 per plan.md; not re-verified at 2.2.0).
5. Expect: the environment appears, then one project and one thread created by the headless oracle client; open the thread. `wf t3 doctor` then lists which RPC tags the phone called that the server answered with a stub (closes protocol.md unknown 2).
6. If it fails, send: the app version, the exact message text, and a screenshot. The server side of what happened is in its log; the connect order a healthy client follows is `rpc-phase1.jsonl` n=0..13.

## 8. Files

Created, all under the two owned paths:

- `tooling/t3-conformance/package.json`, `tsconfig.json`
- `tooling/t3-conformance/src/`: `wsframe.ts` (WebSocket frame codec), `rpc.ts` (Effect RPC frames and a small client), `normalize.ts`, `golden.ts` (JSONL writer, `withoutTiming`), `proxy.ts` (recording HTTP and WebSocket proxy, transparent at the TCP level, strips `permessage-deflate`), `session.ts` (the scripted phase-1 session), `mock-server.ts` (offline stand-in, not T3), `cli.ts` (`record`, `mock`)
- `tooling/t3-conformance/test/tooling.test.ts` (12 tests: normalizer, frame parser, and the proxy plus session against the mock: both directions recorded, no secret survives, 426 and reuse refusal recorded, runs diff clean)
- `tooling/t3-conformance/goldens/v0.1.6/` (7 files, 216 KB)
- `docs/plans/t3-parity/phase0-report.md` (this file)

Workspace note: `pnpm-workspace.yaml` already matches `tooling/*`, so the package is a workspace member; `pnpm-lock.yaml` has the bare importer `tooling/t3-conformance: {}` and `pnpm install --lockfile-only --frozen-lockfile` passes. The package has no dependencies and runs on the repo's Node 22.22 through built-in type stripping. It writes with `process.stdout`/`stderr`, not `console`.
