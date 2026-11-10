# T3 Code client protocol, as read from source (pin v0.1.6)

Status: research, 2026-11-01. Nothing here was run; all of it is read from source.

Source: [pingdotgg/t3code](https://github.com/pingdotgg/t3code), tag `v0.1.6`, commit `2359643cffa6ba9ee84d6f2694b935de54a8013e` (committed 2026-10-31), MIT, Copyright 2026 T3 Tools Inc (`LICENSE:1-3`). Shallow clone into a scratch directory, files read only, nothing executed, clone deleted afterwards. Paths below are inside that tree.

Pin history: the first pass of this document was written against `v0.0.35` (commit `f925d639421844f02b3166d29281905dbba6d529`, 2026-08-26). Facts that did not change are kept and marked "unchanged since 0.0.35" (their old line numbers were not re-verified unless a new cite is given). Everything with a cite at this tag was re-read.

Newest stable tag, confirmed by `git ls-remote --tags` on 2026-11-01: `v0.1.6` (`2359643c...`). Newer tags exist only as previews (`v0.1.6-preview.20261030.3948`), which are not stable and not the target. Stable sequence seen: `v0.1.3`, `v0.1.4`, `v0.1.5`, `v0.1.6`.

Marks: **read** is what the cited lines say. **infer** is my conclusion from the lines. **unknown** is in section 12.

Version caveats:

- `apps/server/package.json:3` and `packages/contracts/package.json:3` say `0.1.5` at the `v0.1.6` tag; the descriptor's `serverVersion` is `packageJson.version` (`apps/server/src/environment/ServerEnvironment.ts:216`). Same pattern as before: the release job stamps the version (infer). What a real server reports is unknown until a run.
- The mobile app source at the tag is `apps/mobile`, default version `2.2.0` (`apps/mobile/app.config.ts:231`, overridable by `T3CODE_MOBILE_VERSION`). The 0.0.35 pin had app version `1.0.4`. `apps/mobile/README.md:3-5` still says "not distributed yet" (stale); the root README links the App Store app "T3 Code Remote" and the Play app (`README.md:3`).
- Effect moved from `4.0.0-beta.103` to `4.0.5` (`pnpm-workspace.yaml:70`). Wire framing facts in section 2.2 were read against the beta; re-verifying them against 4.0.5 is unknown item 1.

## 0. What changed in one paragraph

Between 0.0.35 and 0.1.6 T3 replaced its orchestration model with "orchestration V2" (runs, attempts, execution nodes, turn items, runtime requests, checkpoint scopes, lineage and context transfer; `packages/contracts/src/orchestrationV2.ts`, 4246 lines) and put a hard protocol gate in front of it: a server must advertise `orchestrationProtocolVersion: 2` in its descriptor and the socket upgrade must carry `orchestrationProtocol=2`, or the server answers 426. The V1 commands, events and read models are gone (`docs/orchestration-v2/README.md:60`). The RPC group grew from about 100 to 208 entries, scopes from 8 to 17, capability flags from 13 to about 60, and built-in providers from 5 to 10 drivers. The first call after connect is now `subscribeServerConfig`, not `server.getConfig`.

## 1. Shape of the system

| Fact | Source | Mark |
| --- | --- | --- |
| One runtime boundary: a client talks HTTP plus WebSocket to one execution environment (one T3 server). Environment id persisted at `<stateDir>/environment-id`. Default port 3773. | unchanged since 0.0.35 (`docs/internals/remote.md`) | read (old cite) |
| Schema library: Effect Schema, Effect `4.0.5`. Wire contracts in `packages/contracts/src/*.ts` (29243 lines in 100+ files); RPC declared with Effect `Rpc`/`RpcGroup`, HTTP with `HttpApi`. | `pnpm-workspace.yaml:70`; `packages/contracts/src/rpc.ts:36-40`; `environmentHttp.ts:528-836` | read |
| Server state is SQLite; the descriptor reports the highest migration (`databaseMigration`). | `environment.ts:172-174`; `ServerEnvironment.ts:207-218` | read |
| Client code shared by web, desktop and mobile: `packages/client-runtime` (connection registry, supervisor, RPC session, state). | `packages/client-runtime/src/` | read |
| Orchestrator architecture: app ids are primary, provider ids are refs; runs contain a root execution node plus tool, approval and subagent nodes; root-run completion is the only event that completes a user-visible turn; checkpoints attach to checkpointable scopes; replay-backed tests substitute only the provider transport. | `docs/orchestration-v2/README.md:20-42`, `testing-strategy.md:1-40` | read |

## 2. Transports

### 2.1 HTTP routes (one port)

| Method and path | Purpose | Source | Change |
| --- | --- | --- | --- |
| GET `/.well-known/t3/environment` | descriptor (section 4) | `environmentHttp.ts:529-531` | fields added |
| GET `/api/auth/session` | session state and auth descriptor, optional bearer | `:536-541` | unchanged |
| POST `/api/auth/browser-session` | credential to cookie (web) | `:543-548` | unchanged |
| POST `/oauth/token` | pairing credential to bearer, form-urlencoded, optional DPoP header | `:550-556` | unchanged shape; grants gain `permissions` (section 3) |
| POST `/api/auth/websocket-ticket` and `/api/auth/websocket-ticket/:use`, `use` = `rpc` or `media` | socket ticket | `:558-571`; `auth.ts:252-255` | `/:use` added |
| POST `/api/auth/pairing-token` | mint pairing credential | `:573-578` | unchanged |
| POST `/api/auth/pairing-links/revoke`, `/api/auth/clients/revoke`, `/api/auth/clients/revoke-others` | revoke | `:580-601` | the GET list routes of 0.0.35 are gone; listing moved to the RPC stream `subscribeAuthAccess` (`rpc.ts:1953-1958`; infer) |
| GET `/.well-known/oauth-protected-resource[/mcp]`, `/.well-known/oauth-authorization-server`, POST `/oauth/mcp/register`, GET `/oauth/mcp/authorize`, POST `/oauth/mcp/approval|decision|token` | OAuth for outside MCP clients | `:606-643` | new |
| GET `/api/orchestration/shell`, `/api/orchestration/threads/:threadId`, `.../transcript`, `.../bounded`, `.../history` | V2 read models over HTTP | `:683-716` | `snapshot` and `dispatch` of 0.0.35 removed; three thread routes added |
| GET `/api/projects`, POST `/api/projects/mutate` | project read and mutation | `:727-734` | new (projects left orchestration) |
| POST `/api/pull-requests/diff` | large PR diffs | `:743` | unchanged |
| POST `/api/connect/*`, `/api/t3-connect/*` | T3 Connect relay linking | `:759-811` | grown (link-proof, relay-config, link-state, reachability, unlink, preferences, health, mint-credential) |
| scheduled-task webhook GET and POST at `WEBHOOK_PATH` | inbound webhooks | `:833-836` | new |
| POST `/api/attachments/upload/<token>`, GET `/api/assets/<id>/<path>` | attachment upload, asset serving | unchanged since 0.0.35 (`http.ts:206-281`) | read (old cite) |
| `/mcp` | server MCP for provider sessions | `apps/server/src/mcp/` | grown (orchestrator tools, section 9) |
| `/api/preview-stream` | headless browser tabs, capability `serverBrowser` | `environment.ts:148` | new |
| GET `*` | static web client | unchanged | |

CORS also allows the header `x-t3-orchestration-protocol` (`apps/server/src/httpCors.ts:1-10`). Error bodies are tagged JSON as in 0.0.35 (unchanged), plus the 426 body `{code:"orchestration_protocol_incompatible", message, orchestrationProtocolVersion}` on the socket upgrade (`ws.ts:3291-3299`).

### 2.2 WebSocket

| Fact | Source | Mark |
| --- | --- | --- |
| Path `GET /ws`. **The upgrade must carry query `orchestrationProtocol=2` or the server returns HTTP 426 before authenticating.** Then `wsTicket=<ticket>` and the optional `client*` params. | `ws.ts:637-642,3285-3300`; `compatibility.ts:37-41` | read |
| Framing: Effect RPC, `RpcSerialization.layerJson`, one JSON value (or array) per message; Request, Ack, Interrupt, Ping, Eof from the client; Chunk, Exit, Defect, ClientProtocolError, Pong from the server; ack-paced streams. | `ws.ts:3348`; `rpc/session.ts:243`; frame shapes unchanged since 0.0.35 | read for the serializer; frame shapes not re-verified at Effect 4.0.5 |
| Each RPC has a required scope; the table is `RPC_REQUIRED_SCOPES` (client-guarded part in `clientRpcPermissions.ts:17-71`, the rest in `apps/server/src/auth/RpcAuthorization.ts:40-`; only its first 60 lines were read). Some scopes depend on the payload (`git.runStackedAction` with a thread and a PR action adds `orchestration:read`; `git.preparePullRequestThread` in worktree mode adds `orchestration:operate`). | `clientRpcPermissions.ts:77-97` | read |
| 27 RPCs are streams (`stream: true` count in `rpc.ts`). | `rpc.ts` | read |
| Errors: the method's tagged errors, always unioned with `EnvironmentAuthorizationError`. | `rpc.ts:615-2181` | read |

Client connect sequence at this tag (`packages/client-runtime`):

1. Fetch the descriptor; `orchestrationProtocolCompatibilityError` blocks the connection unless `descriptor.orchestrationProtocolVersion ?? 1` equals 2. A higher server number blocks with "client not supported", a lower one with "requires a newer server" (`connection/compatibility.ts:9-27`, `resolver.ts:318-324`).
2. Exchange the credential (`/oauth/token`), get a ticket, open `/ws?orchestrationProtocol=2&wsTicket=...` (`resolver.ts:324`).
3. Call `subscribeServerConfig`; the first `snapshot` event's `config.environment.environmentId` must equal the id the descriptor gave, or the connection errors with an environment mismatch (`rpc/session.ts:269-343`). `server.getConfig` is no longer the first call.
4. Probe with `server.probe` (always, no capability check) (`rpc/session.ts:407-413`).

## 3. Pairing and authentication

Unchanged since 0.0.35: 12 characters from `23456789ABCDEFGHJKLMNPQRSTUVWXYZ` (alphabet re-read at `PairingGrantStore.ts:254`), TTL 5 minutes (24 hours for desktop and dev startup, `:251-253`), pairing URL `<base>/pair#token=<credential>`, `t3code://` deep link, host plus code entry, token-exchange grant, 30-day bearer, 5-minute ticket. Changes:

| Item | Detail | Source | Mark |
| --- | --- | --- | --- |
| Scopes | 17 literals: `orchestration:read|operate`, `settings:write`, `providers:manage`, `environment:maintain`, `preview:operate`, `diagnostics:read`, `terminal:read|operate`, `filesystem:read|write`, `source-control:write`, `review:write` (legacy, grants nothing), `access:read|write`, `relay:read|write`. | `auth.ts:30-67` | read |
| Standard client set | `AuthStandardClientScopes` lists 16 (all but `review:write`). Whether a plain pairing grants all of them was not read (infer yes, by the name). | `auth.ts:149-166` | read / infer |
| Dual response | A grant is reported as `scopes` (the legacy subset of 8) plus `permissions` (everything); old clients read `scopes`. A denial carries `requiredScope` (the legacy parent) and `requiredPermission`. | `auth.ts:78-117` | read |
| Ticket use | `use: rpc|media` selects what the ticket opens; the socket uses `/api/auth/websocket-ticket/rpc` (infer from the path builder). | `auth.ts:252-255`; `authorization/remote.ts:190` | read / infer |
| Auth descriptor | adds optional `serverUpdateScope: "environment:maintain"`. | `auth.ts:179-187` | read |

## 4. Environment descriptor and config

Descriptor (`environment.ts:167-176`): `{environmentId, label, platform:{os, arch, machine?}, serverVersion, orchestrationProtocolVersion?, databaseMigration?, capabilities}`. `machine` is one of `server|cloud|linux|desktop|laptop|mac-mini|mac-studio` (`environment.ts:29-45`).

Capabilities (`environment.ts:72-164`): about 60 optional flags, all `optionalKey` except `repositoryIdentity` (default false). Grouped:

- Connection and files: `connectionProbe`, `attachmentUploads`, `questionAttachments`, `fileAttachments{maxUploadBytes}`, `ignoredFileListing`, `environmentIcon`, `serverBrowser`, `commandOutputStreaming`.
- Threads: `threadSettlement`, `threadAutoSettlement`, `threadAutoSettlementScope`, `threadAutoSettleOptOut`, `threadSnooze`, `threadPinning`, `threadPinReorder`, `threadActiveReorder`, `threadForceStop`, `threadStop`, `threadTitleRegeneration`, `threadVisitedTracking`, `threadRestartContinuation`, `threadProjectFromScratch`, `sideChats`, `inlineMessageContext`, `serverResolvedCommandContext`, `requiredWorktreeBootstrap`.
- Pull requests: `pullRequests`, `pullRequestChecks`, `threadPullRequests`, `threadPullRequestWatch`, `threadPullRequestLinking`, `pullRequestStackActions`.
- Projects: `projectSettingsOverrides`, `projectProviderInstanceOverrides`, `projectOrganization`, `projectCloneTracking`, `projectWorktreeCleanup`, `settledWorktreeCleanup`, `worktreesDirectory`.
- Admin and updates: `storageCleanup`, `storageCleanupRun`, `databaseStorage`, `otlpEndpointCheck`, `serverSelfUpdate` (`boot-service|respawn|desktop-managed`), `serverSelfUpdateProgress`, `npmReleaseLookup`, `serverInstallation`, `serverUpdateThreadContinuation`, `desktopAppUpdate`, `desktopAppChannelSwitch`.
- Other: `environmentThemes`, `usageLimitSources`, `usagePriceOverrides`, `usageModelAliases`, `providerWorkspaceSnapshotsOnDemand`, `agentActivityPublishing`, `peerLinks`, `mcpModeLimitHeader`, `linkFence`, `triage`, `localServerStop`.

The real server advertises nearly all of them (`ServerEnvironment.ts:219-`). The comments beside each flag say what a client hides when it is absent, so a minimal server advertises few and the flag list doubles as the list of features a client will not call.

`ServerConfig` (`server.ts:609-661`): required `environment, auth, cwd, keybindingsConfigPath, keybindings, issues, providers, availableEditors, observability, settings`; optional `remoteOpenTargets, directEndpoints`, flags `shellResumeCompletionMarker, shellThreadFamilies, shellOlderSettled, shellRevealInFileManager(+Kind), threadResumeCompletionMarker, threadSnapshotPagination, threadFind, threadFindProgressive, secretProviderSettings, secretReferences`, and `scratchWorkspaceRoot, newProjectsRoot, lastEventLoopStall, environmentThemes, usageLimitSources`.

`subscribeServerConfig` (opt-in payload `environmentThemes, usageLimitSources, usageLimitsCommand, workspaceSnapshotsOnDemand`, `rpc.ts:1862-1875`) streams `snapshot`, `keybindingsUpdated`, `providerStatuses`, `settingsUpdated`, `environmentThemesUpdated`, `usageLimitSourcesUpdated` (`server.ts:782-789`). `subscribeServerLifecycle` streams `{version:1, sequence, type: welcome|ready|legacyThreadMigration, payload}` (`server.ts:827-857`). `server.getSettings` and the background-policy RPCs no longer exist; settings arrive in `ServerConfig` and `settingsUpdated` and change through `server.updateSettings {patch}`, whose required scopes are computed from the patch (`requiredScopesForServerSettingsPatch`, imported at `RpcAuthorization.ts:10`).

## 5. Orchestration V2 (the core of the product)

Source: `packages/contracts/src/orchestrationV2.ts`; server `apps/server/src/orchestration-v2/` (decider, event store, command receipts and policy, checkpoint services, adapters, effect outbox and worker, context handoff). The Claude adapter alone is 11550 lines (`Adapters/ClaudeAdapterV2.ts`).

### 5.1 Methods (`orchestrationV2.ts:3549-3569`)

`orchestration.dispatchCommand`, `getTurnDiff`, `getFullThreadDiff`, `searchThreads`, `searchThread`, `searchThreadStream`, `getArchivedShellSnapshot`, `getThreadFamily`, `getWorktreeThreadIds`, `getOlderSettledThreads`, `getThreadProjection`, `getWorkflowScript`, `stopWorkflow`, `forceStopThread`, `getTurnItem`, `launchThread`, `subscribeShell`, `subscribeThread`, `subscribeCommandOutput`. Reads need `orchestration:read`; `dispatchCommand`, `launchThread`, `stopWorkflow`, `forceStopThread` need `orchestration:operate` (`RpcAuthorization.ts:42-60`; `clientRpcPermissions.ts:62-64`). Projects are no longer commands: `projects.mutate`, `projects.createNew`, `projects.createFromThread`, `projects.ensureScratch` (`rpc.ts:414-417`).

### 5.2 Commands, `OrchestrationV2Command`, 55 `type` values (`orchestrationV2.ts:2877-3348`)

- Thread lifecycle: `thread.create`, `.archive`, `.unarchive`, `.delete`, `.stop`, `.settle`, `.auto-settle`, `.unsettle`, `.snooze`, `.unsnooze`, `.auto-settle.set`, `.pin`, `.unpin`, `.pin.reorder`, `.active.reorder`, `.visit`, `.mark-unread`, `.metadata.update`, `.runtime-mode.set`, `.interaction-mode.set`, `.model-selection.set`, `.fork`, `.merge_back`, `.created.record`, `.title.regeneration.complete`, `.guest-share.close`, `.guest-message.dismiss`, `.user-input.dismiss`.
- Pull request linkage: `thread.pull-request.link|unlink|watch|sync`, `thread.pull-request-link.sync`.
- Messages and runs: `message.dispatch` (dispatch modes `start_immediately`, `defer_start`, `steer_active`, `restart_active`, `queue_after_active`), `run.interrupt`, `queued-message.promote-to-steer`, `queue.resume`, `queued-run.start|reorder|cancel|edit`, `prepared-run.release|progress|fail|retry`, `notification.delivery.accept`, `subagent.stop`.
- Requests and checkpoints: `runtime-request.respond`, `checkpoint.rollback`.
- Providers and delegation: `provider.switch`, `provider-session.detach`, `delegated_task.request|wake-policy|completion-delivery.acknowledge|completion-delivery.dispose`.

Every command carries `commandId`; receipts make dispatch idempotent (infer from `CommandReceiptStore.ts` and from `OrchestrationV2DispatchCommandError {commandId, commandType, message, detail?, cause?}`, `orchestrationV2.ts:3884-3893`). The result is `{sequence}` (`:3638-3641`). `launchThread` creates or reuses a thread with a workspace strategy and an optional first message and returns the projection (`:3606-3636`).

### 5.3 Events, `OrchestrationV2DomainEvent`, 45 types (`orchestrationV2.ts:1851-1995`)

`thread.created|archived|unarchived|deleted|settled|unsettled|snoozed|unsnoozed|pinned|auto-settle-set|unpinned|pin-reordered|active-reordered|visited|marked-unread|metadata-updated|pull-request-synced|runtime-mode-updated|interaction-mode-updated|model-selection-updated|provider-switched|guest-share-updated|pull-request-link-updated` (23); `run.created|updated|background-work-cancelled`; `run-attempt.created|updated`; `node.updated`; `subagent.updated`; `provider-session.attached|updated|detached`; `provider-thread.updated`; `provider-turn.updated`; `runtime-request.updated`; `message.updated`; `turn-item.updated`; `plan.updated`; `checkpoint-scope.created`; `checkpoint.captured`; `checkpoint.rollback-requested`; `context-handoff.updated`; `context-transfer.created|updated` (22).

Events are upserts of entities (`*.updated`), not the V1 request and response event pairs. Turn items (`OrchestrationV2TurnItem`, `:1583-`) have 26 `type` values: `notification, user_message, assistant_message, reasoning, proposed_plan, todo_list, user_input_request, file_change, command_execution, file_search, web_search, approval_request, checkpoint, run_interrupt_request, run_interrupt_result, system_notice, error, compaction, handoff, fork, run, node, provider_thread, thread_created, subagent, dynamic_tool`. A client skips unknown event types and unknown turn-item types while advancing its resume cursor (`:3821-3856`, `ForwardCompatibleUnion`).

### 5.4 Streams, snapshots, resume

| Stream | Frames | Source |
| --- | --- | --- |
| `orchestration.subscribeShell {afterSequence?, requestCompletionMarker?, omitIdleSubagents?, omitOlderSettled?}` | `{kind:"synchronized"}` (only if requested), `{kind:"snapshot", snapshot:{schemaVersion, snapshotSequence, threads, archivedThreads, projects, olderSettled?}, resolvedRepositoryIdentityRoots?}`, `{kind:"project.updated"|"project.removed"|"thread.updated"|"thread.removed", sequence, ...}`; thread items carry `location: active|archive`. | `orchestrationV2.ts:2199-2246,3649-3662` |
| `orchestration.subscribeThread {threadId, afterSequence?, requestCompletionMarker?, acceptBoundedSnapshot?, acceptDeferredWork?, acceptCompactTurnItems?, acceptDedupedBodies?, acceptCompactIds?}` | `{kind:"synchronized"}`, `{kind:"snapshot", snapshotSequence, projection, historyCursor?, hasMoreHistory?, latestLocalTurnOrdinal?, turnItemsOmitLocalVisible?, idTable?}`, `{kind:"event", sequence, event}`, plus a client-side `unknown-event` fallback for event types it does not know. | `orchestrationV2.ts:3747-3757,3858-3882` |

Every `accept*` and `omit*` field is an opt-in that changes the encoding (compact ids, deduplicated bodies, an id table); the server sets `idTable` only for a client that opted in (`:3760-3764`). Infer: a server that ignores every opt-in and always sends the plain full forms is valid for every client, at the cost of larger payloads. The shell resume rule is unchanged: replay when the gap is within 1000 events, else send a fresh snapshot (`ws.ts:583,1172`). The shell may leave out idle subagent children and older settled threads when the client asks and the server advertises `shellThreadFamilies` and `shellOlderSettled` (`server.ts:624-633`).

Read model: `OrchestrationV2ThreadProjection` = `{thread, runs, attempts, nodes, subagents, providerSessions, providerThreads, providerTurns, runtimeRequests, messages, plans, turnItems, checkpointScopes, checkpoints, contextHandoffs, contextTransfers, visibleTurnItems, updatedAt}` (`:1997-2022`). The shell row is `OrchestrationV2ThreadShell` (`:2058-`).

## 6. Providers

### 6.1 Wire model

`ServerProvider` (`server.ts:206-267`) keeps `instanceId`, `driver`, `enabled`, `installed`, `version`, `status`, `auth`, `checkedAt`, `models`, `slashCommands`, `skills` and adds `iconUrl`, `reportsContextWindow`, `supportedRuntimeModes`, `supportsConversationRollback`, `supportsTextGeneration`, `setup{canAuthenticate, canInstall, documentationUrl?}`, `nativeSessions{canList, canLoad, canResume, canDelete?}`, `configurableProviders`, `runtimePaths`, `installedVersion`, `updateRequiredModels`, `workspaceSnapshots`, `usageLimits`, `compatibilityAdvisory`, `updateState`. `ProviderDriverKind` is still an open slug (`providerInstance.ts:17`); unknown drivers must decode and render as unavailable. Provider management RPCs: `server.refreshProviders {instanceId?, cwd?, fresh?, refreshModels?, reloadSecrets?, workspaceSnapshotsOnDemand?, usageLimitsCommand?}`, `server.updateProvider`, `provider.auth.start|complete|respond|cancel|logout|subscribe`, `provider.install.start|cancel|subscribe|remove`, `provider.chatgpt.*`, `provider.consumeResetCredit`, `provider.uploadFeedback` (`rpc.ts:435-449,639-760`).

### 6.2 Built-in drivers (`apps/server/src/provider/builtInDrivers.ts:31-42`)

Ten: `codex, claudeAgent, cursor, grok, fx, opencode, antigravity, pi, muse, acpRegistry`. What a container needs for them is in plan.md section 5. "not stated" means the tag does not say and it was not researched elsewhere.

| Driver | Launch | Auth | Linux arm64 | Install | Licence | Source |
| --- | --- | --- | --- | --- | --- | --- |
| `codex` | `codex app-server` over stdio (JSON-RPC) | ChatGPT OAuth through T3 (managed runtime and token broker) or `codex login` (`~/.codex/auth.json`); `CODEX_HOME`, optional shadow home | yes: managed release `aarch64-unknown-linux-musl`, v0.156.1, 136,933,361 bytes, sha256 pinned | T3 downloads it, or `codex` on PATH | not in the tag (Apache-2.0 upstream, verify) | `CodexInstallation.ts:59-100,409`; `codexLaunchArgs.ts:13`; `docs/user/providers-codex.md` |
| `claudeAgent` | `@anthropic-ai/claude-agent-sdk ^0.3.276` spawns the `claude` executable (`pathToClaudeCodeExecutable` = settings binary path, else the SDK's own) | Claude Code login in `CLAUDE_CONFIG_DIR` (default `~/.claude`), `claude auth login`; or per-instance env `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL` for routers | the lockfile names `claude-agent-sdk-linux-arm64` | npm dependency of the server, or the Claude Code install | proprietary (Anthropic terms), not in the tag | `apps/server/package.json:36`; `ClaudeAdapterV2.ts:1583-1584`; `docs/user/providers-claude.md:12-38,97-118` |
| `opencode` | `opencode serve --hostname=... --port=...` (local), or an external `serverUrl` plus password | `opencode auth login`; `OPENCODE_SERVER_PASSWORD`; needs OpenCode >= 1.14.19, 2.0.18+ supported (the doc names a separate package `@opencode/cli`) | not stated in the tag (this repo already runs opencode on arm64) | `opencode` on PATH | MIT upstream (verify) | `provider-opencode/src/server/OpenCodeRuntime.ts:510`; `docs/user/providers-opencode.md:3-35`; `@opencode-ai/sdk ^1.3.15`, `package.json:78` |
| `cursor` | `@cursor/sdk` 1.0.38 local-agent runtime (not ACP); Node >= 22.13 | browser sign-in stored per instance, or `CURSOR_API_KEY`; paid Cursor plan | not stated | npm dependency (`package.json:37`); CLI executable `cursor-agent` | proprietary (assumed) | `docs/user/cursor.md:3-30`; `install.md:165,175` |
| `grok` | `grok agent stdio` over ACP (`--permission-mode auto`, `agent --always-approve stdio`) | `grok login` | not stated | Grok Build CLI from x.ai | proprietary (assumed) | `provider-grok/src/server/acpSupport.ts:47-68`; `install.md:166` |
| `fx` | `fx acp` over ACP | `fx login`; one login per machine shared by all instances | not stated | `curl -fsSL https://fx.sh/setup.sh \| bash` | unknown | `provider-fx/src/server/acpSupport.ts:230`; `docs/user/providers-fx.md:1-20` |
| `antigravity` | Google's Antigravity ACP agent binary, managed by T3 | Google browser sign-in from T3 settings (separate from the IDE) | yes: `agy-acp-server-1.4.1-linux-arm64.zip` | T3 downloads it from `dl.google.com` | proprietary (Google) | `antigravityRelease.ts:54-57`; `docs/user/providers-antigravity.md:1-12` |
| `pi` | `pi --mode rpc [--session <path> or --no-session]` | `pi` then `/login`, or the model provider's API-key env var on the instance; Node >= 22.19; Pi >= 0.80.5 | JavaScript, arch independent | `npm install -g --ignore-scripts @earendil-works/pi-coding-agent` | not in the tag (verify) | `provider-pi/src/server/mcpInjection.ts:258-266`; `docs/user/providers-pi.md:6-34` |
| `muse` | `muse serve [--trust-workspace] [--disable-sandbox]` (read-only adds `--disable-shell --disable-write`) | `muse login`, or `META_API_KEY` on the instance (the server's own `META_API_KEY` is ignored) | not stated | Muse Code from Meta | proprietary (assumed) | `provider-muse/src/server/sdk.ts:119-150`; `docs/user/providers-muse.md:1-27` |
| `acpRegistry` | any registry agent or local ACP command, spawned directly without a shell | the agent's own; registry agents are third-party code | per agent | registry at `cdn.agentclientprotocol.com`; binary, npx or uvx distributions into a managed directory | per agent | `docs/user/providers-acp.md:1-40`; `acpRegistry.ts:13`; `provider-acp-registry/src/server/AcpRegistrySupport.ts` |

All CLIs must be on the server's PATH or have a binary path set; installation, login and configuration belong to the environment's machine even when the client is a phone (`docs/user/install.md:154-177`). The adapter contract a driver implements under V2 is `packages/provider-core/src/server/ProviderAdapter.ts` (673 lines); the generic ACP path is `packages/provider-acp`. Test helpers shipped in the tag: `packages/provider-testing/src/{fakeCli,replayTranscript,replayWorkspace,replayGate,TestProviderHost}.ts`.

## 7. Terminals, files, git, checkpoints, pull requests

Unchanged since 0.0.35 unless noted. Terminal RPCs: `terminal.open|attach|observe|write|resize|clear|close` plus `subscribeTerminalMetadata` (`rpc.ts:468-474`); `observe` is new and `subscribeTerminalEvents` is gone; scopes `terminal:read` (observe) and `terminal:operate`. Files: `projects.listEntries|listIgnoredFiles|readFile|searchEntries|searchContents|writeFile`, `filesystem.browse|getMetadata` (scopes `filesystem:read|write`), `shell.openInEditor`, `agentSessions.scan|import` (import a provider's native session). Git and VCS: `vcs.pull|refreshStatus|listRefs|createWorktree|removeWorktree|createRef|switchRef|init`, `git.runStackedAction`, `git.resolvePullRequest`, `git.preparePullRequestThread`, streams `subscribeVcsStatus`, `subscribeWorktreeSetup`, `worktreeSetup.cancel` (writes need `source-control:write`). Review: `review.getBranchCommits|getDiffPreview|getDiffFileContents`. Project cloning: `projectClone.start|cancel|retry`, stream `subscribeProjectClones`. Source control: `sourceControl.lookupRepository|cloneRepository|publishRepository|signInStart|signInWait|signInCancel|appAccess|starred`. Pull requests: 35 `pullRequests.*` RPCs (full list in plan.md section 3). Checkpoints are first-class V2 entities (`checkpoint-scope.created`, `checkpoint.captured`, command `checkpoint.rollback`); the git ref scheme of 0.0.35 (`refs/t3/checkpoints/<b64url thread>/turn/<n>`) was not re-read at this tag (unknown item 6).

## 8. Versioning and compatibility (re-checked: the earlier "no version gate" finding is wrong at this tag)

- Hard gate on both sides: the descriptor field `orchestrationProtocolVersion` (`environment.ts:172`) must equal 2 for any client of this tag (`compatibility.ts:13-26`), and the WebSocket upgrade must send `orchestrationProtocol=2` (`ws.ts:637-642`). Constants: `ORCHESTRATION_PROTOCOL_VERSION = 2`, query param `orchestrationProtocol`, header `x-t3-orchestration-protocol` (`environment.ts:13-16`). A missing descriptor field means protocol 1 to a client, so a 0.0.35-era server is rejected by this client and a protocol-2 server is rejected by a 0.0.35-era client. The T3 CLI `pair` also compares protocols (`apps/server/src/cli/pair.ts:86,495`).
- Consequence for us: we implement exactly protocol 2. If T3 ships protocol 3, a store app that auto-updates will block against us ("client not supported") until we ship the new protocol. Nothing in the client negotiates down.
- Finer-grained compatibility is still by capability flags, optional schema fields, `ForwardCompatibleArray` and `ForwardCompatibleUnion` (`baseSchemas.ts`, `environment.ts:72-164`).
- Provider advisories (`compatibilityAdvisory`, `versionAdvisory`) warn about CLI versions; they do not block (`server.ts:264-265`).
- `serverVersion` is used to follow self-updates only (unchanged since 0.0.35).

## 9. MCP, settings, new product areas

- `/mcp` is the server's MCP endpoint for provider sessions plus an OAuth surface for outside MCP clients (`environmentHttp.ts:604-650`), orchestrator tools (`orchestratorMcp.ts`, 877 lines), thread metadata and worktree tools (`threadMetadataMcp.ts`, `worktreeMcp.ts`) and MCP apps (`mcpApps.callTool|toolInfo|readResource|updateModelContext`). Flags `mcpModeLimitHeader`, `linkFence`, `peerLinks`. Tool lists were not extracted.
- Settings: `ServerSettings` is in `settings.ts` (1799 lines); `server.updateSettings {patch}`; secrets can be references to secret providers (`secretProviderSettings`, `secretReferences`; a 1Password package exists).
- New areas (RPCs listed in plan.md section 3): scheduled tasks with webhooks, guest share links, peer links between environments, device farm (`device.*`), browser preview (`preview.*`, `/api/preview-stream`), triage, usage and usage limits, storage cleanup, resource telemetry, relay client and T3 Connect.

## 10. Clients

| Client | Fact | Source |
| --- | --- | --- |
| iOS | `apps/mobile`, Expo with native modules (dev client, no Expo Go), default version `2.2.0`, shares `client-runtime` and `contracts`. Pairing: pairing URL, `t3code://`, or host plus code (unchanged). App Store app "T3 Code Remote". Whether the store build equals the tag's `2.2.0` is unknown until the owner reads the version. | `apps/mobile/app.config.ts:231`; `README.md:3` |
| Web, desktop | web served by the server at the same origin or `app.t3.codes`; desktop is Electron with a local backend plus saved remote environments. | unchanged since 0.0.35 |
| Connect-critical RPCs | `subscribeServerConfig`, `server.probe`, `subscribeServerLifecycle`, `orchestration.subscribeShell`, `subscribeThread`, `dispatchCommand` appear in the shared runtime. The mobile source references only five method names directly (the rest go through `client-runtime`). Which other streams the phone opens right after connect (auth access, terminal metadata, scheduled tasks, resource telemetry, usage) was not traced. | `client-runtime/src/rpc/session.ts`; `state/{server,shell,orchestration}.ts` |

## 11. Delta since 0.0.35

Effect on a server implementer: **Blocker** = a client refuses or cannot render without it; **Work** = needed for parity, not for connecting; **Drop** = no longer needed.

| # | Area | 0.0.35 | v0.1.6 | Effect |
| --- | --- | --- | --- | --- |
| 1 | Version gate | none | descriptor `orchestrationProtocolVersion` must be 2; socket needs `orchestrationProtocol=2` else 426 (`compatibility.ts:9-41`, `ws.ts:3291-3299`) | Blocker: descriptor and upgrade handling first |
| 2 | Orchestration model | V1: projects and threads, per-turn events, `thread.turn.start`, `thread.message-sent`, `thread.activity-appended` | V2: runs, attempts, nodes, turn items, runtime requests, checkpoint scopes, lineage; 55 commands, 45 events, 26 item types (`orchestrationV2.ts`) | Blocker and the largest work: a new decider and projector; V1 knowledge is obsolete |
| 3 | RPC set | about 100 | 208 group entries (`rpc.ts:1972-2181`), 27 streams | Work: parity scope doubled |
| 4 | Removed RPCs | `server.getSettings`, `subscribeTerminalEvents`, `server.getBackgroundPolicy`, `subscribeBackgroundPolicy`, `server.reportHostPowerState` | absent from `rpc.ts` | Drop |
| 5 | Connect handshake | `server.getConfig` first, `server.probe` if capability | `subscribeServerConfig` first, environment id must match, `server.probe` always | Blocker |
| 6 | Shell and thread streams | `snapshot`, `project-upserted`, `thread-upserted`, `thread-removed`; thread `snapshot{thread}` | `synchronized`, `snapshot{schemaVersion,...}`, `project.updated`, `thread.updated{location}`; thread `snapshot{projection}`, `event`, opt-in compaction | Blocker |
| 7 | Projects | commands `project.create|meta.update|delete` in orchestration | `projects.mutate`, `projects.createNew|createFromThread|ensureScratch`, HTTP `/api/projects*` | Blocker for creating a project |
| 8 | Scopes | 8 | 17; grants carry `permissions` plus legacy `scopes`; denials carry `requiredPermission` | Work |
| 9 | Tickets | one kind | `/api/auth/websocket-ticket/:use`, `rpc` or `media` | Work |
| 10 | Descriptor | 13 capability flags | about 60 flags, `orchestrationProtocolVersion`, `databaseMigration`, `platform.machine` | Blocker for the version field; the rest Work |
| 11 | `ServerConfig` | `environment, auth, cwd, keybindings*, issues, providers, availableEditors, observability, settings` | adds `directEndpoints`, `shell*`, `thread*`, `secret*`, `scratchWorkspaceRoot`, `newProjectsRoot`, `environmentThemes`, `usageLimitSources`; config stream gains theme and usage-source events | Work |
| 12 | Providers | 5 drivers (codex, claudeAgent, cursor, grok, opencode) | 10 drivers; install, auth, update and native-session RPCs; ACP registry | Work, large |
| 13 | HTTP | snapshot and dispatch routes, GET lists of pairing links and clients | removed; added thread transcript, bounded and history, `/api/projects*`, MCP OAuth, preview stream, webhooks | Work |
| 14 | Effect | `4.0.0-beta.103` | `4.0.5` | Re-record goldens; frame details unverified |
| 15 | iOS app | 1.0.4 | 2.2.0 at the tag | The old pin cannot serve the current store app |
| 16 | New product areas | none | scheduled tasks, guest shares, peer links, devices, preview, MCP apps, triage, usage, relay client | Work, mostly stubs or declared errors where the feature needs T3's cloud or a desktop |

## 12. Unknowns (need a recording of the real server at v0.1.6)

1. Wire-exact bytes at Effect 4.0.5: request id format, header list, Ping interval, tagged error encoding, `Chunk` batching.
2. Exactly which RPCs and streams the iPhone opens after connect, in which order, and which of them need a non-error answer.
3. Mandatory fields of `ServerSettings`, `keybindings`, `observability`, `issues` in practice (decoders have defaults).
4. Payload shapes of each turn-item type and of provider runtime events as the real server emits them for a plain text turn, an approval, a question, a file change and a command.
5. How the published `t3@0.1.6` stamps `serverVersion` and which capabilities a stock server advertises.
6. Checkpoint ref naming and diff semantics under V2.
7. Pairing credential single-use enforcement and failure codes on reuse and expiry.
8. The remainder of the scope table beyond the first 60 lines of `RpcAuthorization.ts`.
9. Terms of use of T3's hosted services (`app.t3.codes`, relay, Clerk), and Apple's and T3's terms for third-party backends.
10. DPoP and the T3 Connect flow: not needed unless the relay is used.

## 13. What `wf t3 serve` answers (phase 1 and 2, flow-tested)

Which RPCs the iOS app calls beyond the connect path is still unknown (unknown 2); this table is by what this server does, so a call the app makes is either real or a typed `UnsupportedOperationError {_tag, message, method}` (declared failure, the socket lives on). `wf t3 serve` logs `rpc.unsupported tag=...` for each, which is the list to read after a phone session. Frames marked **recorded** equal the goldens in `tooling/t3-conformance` after normalization (flow test `conformance`, 0 differences; 519 before the pass). Frames marked **unverified** were designed from the source reading: no model turn was recorded.

| Group | Tags | Status | Shape evidence |
| --- | --- | --- | --- |
| Connect | `server.probe` (`{}`), `server.getConfig`, `subscribeServerConfig` (`{version:1,type:"snapshot",config}`), `subscribeServerLifecycle` (`welcome`, `ready`) | real | recorded |
| Shell, thread reads | `orchestration.subscribeShell`, `subscribeThread`, `getThreadProjection` | real (archive leaves the shell as `thread.removed` location `active`) | recorded for snapshot, event, removed, updated |
| Thread commands | `thread.create`, `archive`, `unarchive`, `delete`, `metadata.update`, `runtime-mode.set`, `interaction-mode.set`, `model-selection.set`, `pin`, `unpin`, `visit` | real | create and archive recorded; the rest unverified rows (the recorded event row) |
| Run commands | `message.dispatch` (mode `start_immediately`), `run.interrupt`, `thread.stop` | real on the execution host (rca `/exec/v1`) | unverified: message, run and turn-item entities and the shell fields a run sets |
| Other commands of the 55 | the rest of the union (settle, snooze, fork, queue modes, runtime requests, ...) | declared: typed `OrchestrationV2DispatchCommandError` "not supported"; a type outside the union is the decoder's `Die` (union text recorded) | recorded for the defects |
| Projects | `projects.mutate` (`project.create|update|archive`; answers the project row) | real | recorded for create |
| Project files | `projects.listEntries`, `readFile`, `writeFile`, `filesystem.browse` | real through the host, payload keys `cwd` or `projectId`, `relativePath` | unverified |
| Streams that wait on features | `subscribeAuthAccess`, `subscribeTerminalMetadata`, `subscribeVcsStatus`, `scheduledTasks.subscribe`, `subscribeProjectClones`, `subscribePreviewEvents`, `subscribeDeviceState`, `server.subscribeResourceTelemetry`, `provider.*.subscribe`, ... | declared (error, not an open empty stream) | none |
| Terminals | `terminal.*` | declared: needs the rca pty routes (phase 4); whether the iOS app opens them is unknown | none |
| VCS, git, review, pull requests, source control, clones | `vcs.*`, `git.*`, `review.*`, `pullRequests.*`, `sourceControl.*`, `projectClone.*` | declared | none |
| Settings, providers, usage, storage, diagnostics, scheduled tasks, peer links, MCP apps, preview, devices, relay, self-update | the remaining rows of the scope table | declared | none |
| Unknown tag (not in the scope table) | | `Die` "Unknown request tag: <tag>" | recorded |

The list of declared RPCs is not typed by hand: `known_tags()` of the scope table minus `IMPLEMENTED_RPCS` is what the flow test `every_rpc_of_the_scope_table_this_server_does_not_build_...` calls, so a tag cannot drop out of the table.

Capabilities: the descriptor lists all 56 recorded keys; only `connectionProbe`, `threadPinning`, `threadVisitedTracking`, `threadStop`, `threadRestartContinuation` are true (`fileAttachments`, an object, is omitted). Named divergences from the recording (the `IGNORED` and `OMITTED` tables in the conformance test): label, platform, serverVersion, paths, `auth.policy` (`remote-reachable`), `auth.sessionMethods` (bearer only), `localTracingEnabled`, provider list (`opencode` ready with the ladder's models, `claudeAgent` and `codex` disabled until the image carries them).

## 14. rca `/exec/v1` (what `wf t3` called to execute; retired in M3)

**Retired.** These routes were removed in M3 and answer 404: `wf t3 serve` reaches rca through its agent API (`agent.threads`, `file.list`, `file.read`) with a device bearer, and the service holds no workspace (`docs/plans/rca-parity/plan.md` section 2, `docs/plans/rca-client/architecture.md`). The table below is what existed.

Additive to rca; the mobile contract (`/ws`, `/api/chat`, `/runs`) is untouched. Auth: `X-Bot-Secret: <CHAT_AGENT_API_SECRET>` on every route, the `/runs` gate (refused when rca has no secret); the front's `--rca-url http://host:port` plus that secret from the environment selects it (`wf t3 serve --help`).

| Route | Body or query | Answer |
| --- | --- | --- |
| `GET /exec/v1/catalog` | | `{drivers:[{driver, installed, version}]}` for `opencode`, `claudeAgent`, `codex` (the CLI found on PATH, `--version`) |
| `POST /exec/v1/turns` | `{threadId, runId, instanceId, model, cwd, prompt, runtimeMode, interactionMode}` | `application/x-ndjson`, one object per line: `{type:"text"|"reasoning", delta}`, `{type:"tool.started", id, name, input}`, `{type:"tool.finished", id, output, ok}`, and last `{type:"done"}` or `{type:"error", message}` |
| `POST /exec/v1/turns/{thread}/interrupt` | | `{interrupted: bool}`; kills that thread's provider process |
| `GET /exec/v1/fs/list` | `root`, `path` | `{entries:[{name, directory, size}]}` |
| `GET /exec/v1/fs/read` | `root`, `path` | `{contents}` |
| `PUT /exec/v1/fs/write` | `{root, path, contents}` | `{ok:true}` |

`cwd` and `root` must lie under rca's workspace root (`RCA_EXEC_ROOT`, default `/workspace`); `..` and symlink escapes are refused with 400 `{error}`. A turn runs the provider CLI as a child process (`claude -p --output-format stream-json`, `codex exec --json`, `opencode run --format json`) in `cwd`; each CLI's JSON lines are translated to the neutral events in rca's core, never emitted as T3 payloads. Terminals, git, checkpoints, leases and provider install routes of plan.md 4.2 are not built.
