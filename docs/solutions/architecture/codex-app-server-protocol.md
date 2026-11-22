---
title: Codex app-server protocol, as the rca executor must speak it
date: 2026-11-22
category: architecture
module: backend/remote-chat-agent
problem_type: architecture_pattern
component: development_workflow
severity: medium
tags: [architecture, rca, rca-exec, codex, app-server, json-rpc]
applies_when:
  - "writing or changing the Codex adapter that hosts a Codex app-server process at the executor"
  - "mapping Codex notifications to agent.threads events"
  - "answering a Codex approval or user-input request"
related_components: [tooling]
---

# Codex app-server protocol

One `codex app-server` process hosts many threads. The executor starts one per workspace and CLI, keeps it for the life of the workspace, and carries each phone message to it and each notification back. This note is what the adapter codes against. It replaced the one-process-per-turn runner and `exec resume`, which are gone: the first implementation is below.

Sources, tagged on every fact below:

- **[bin]** the installed `codex-cli 0.171.0`: `codex app-server generate-ts --experimental` and `generate-json-schema --experimental` (generated into `/dev/shm/codex-schema`, not committed; regenerate with the two commands), plus one live handshake run (`initialize`, `thread/loaded/list`, a second `initialize`, a bad `turn/steer`, an unknown method) in a scratch `CODEX_HOME`. No login, no thread, no model turn.
- **[t3]** `references/t3code`: `packages/effect-codex-app-server/src/{client,protocol}.ts`, `apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts`, and the recorded wire transcripts `apps/server/src/orchestration-v2/testkit/fixtures/*/codex_transcript.ndjson` (recorded against Codex 0.156.1).

Fields marked "experimental" appear only with `experimentalApi: true` in `initialize`; T3 always sets it.

## Transport and framing

- **[bin]** `codex app-server` listens on stdio by default. `--listen` also accepts `unix://PATH`, `ws://IP:PORT`, `off`. Use stdio: the child's stdin and stdout are the channel, stderr is log text.
- **[bin][t3]** One JSON object per line (`\n`), UTF-8. **No `"jsonrpc":"2.0"` field**, either direction.
  - request: `{"id":1,"method":"…","params":{…}}`
  - notification (no `id`): `{"method":"…","params":{…}}`
  - response: `{"id":1,"result":{…}}`
  - error: `{"id":1,"error":{"code":-32600,"message":"…"}}`
- **[bin]** Ids are string or integer. Client ids and server-request ids are separate spaces (server requests start at `0` in the transcripts). Reply to a server request with the same `id` it carried.
- **[bin]** Server notifications carry an extra `emittedAtMs` next to `params`; ignore it.
- **[t3]** Lines can be large (T3 bounds one at 128 MiB). Stderr is not protocol; keep a tail for crash messages.
- **[bin]** Env: `CODEX_HOME` selects the state directory (reported back in `initialize`). Startup prints a bubblewrap ERROR line on stderr when `bwrap` is not on PATH; it is also sent as a `configWarning` notification and is not fatal.

## Handshake

```json
→ {"id":1,"method":"initialize","params":{"clientInfo":{"name":"rca-exec","title":"rca-exec","version":"0.1.0"},"capabilities":{"experimentalApi":true,"optOutNotificationMethods":["turn/diff/updated"]}}}
← {"id":1,"result":{"userAgent":"rca-exec/0.171.0 (Ubuntu 24.4.0; x86_64) linux (rca-exec; 0.1.0)","codexHome":"/home/u/.codex","platformFamily":"unix","platformOs":"linux"}}
→ {"method":"initialized"}
```

- **[bin]** `InitializeParams = {clientInfo:{name,title|null,version}, capabilities: InitializeCapabilities|null}`. Capabilities: `experimentalApi: boolean`, `requestAttestation: boolean`, `optOutNotificationMethods?: string[]|null` (exact method names to suppress for this connection), `mcpServerOpenaiFormElicitation?`, `extensions?`.
- **[bin]** Result: `userAgent`, `codexHome`, `platformFamily`, `platformOs`. The CLI version is the part of `userAgent` after the first `/`.
- **[bin]** A second `initialize` on the same process fails with `{"code":-32600,"message":"Already initialized"}`. **[t3]** T3 treats that error as success (an interrupted first attempt can leave the server initialized) and serialises initialize behind a semaphore, because one process serves many threads.
- **[bin]** Right after initialize the server sends unsolicited `configWarning` and `remoteControl/status/changed`. Ignore unknown notifications.
- **[bin]** Unknown method gives `-32600` `Invalid request: unknown variant …`. A bad thread id gives `-32600` `invalid thread id: …` (ids are UUIDs, UUIDv7 as generated).

## Client requests (the ones the adapter needs)

All **[bin]** names and params; **[t3]** shows what T3 actually sends.

### thread/start

```json
→ {"id":2,"method":"thread/start","params":{"cwd":"/work/repo","model":"gpt-5","approvalPolicy":"on-request","sandbox":"workspace-write","config":{"tools.update_plan.enabled":true}}}
← {"id":2,"result":{"thread":{"id":"01a0d60e-f7cc-7a50-bdff-90174e2c3b56","sessionId":"…","preview":"","ephemeral":false,"modelProvider":"openai","model":"gpt-5","status":{"type":"idle"},"path":"/home/u/.codex/sessions/2026/09/24/rollout-….jsonl","cwd":"/work/repo","cliVersion":"0.171.0","source":"vscode","name":null,"turns":[]},"model":"gpt-5","modelProvider":"openai","serviceTier":null,"cwd":"/work/repo","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"workspaceWrite","writableRoots":[],"networkAccess":false,"excludeTmpdirEnvVar":false,"excludeSlashTmp":false},"reasoningEffort":null}}
```

- Params (all optional): `model`, `modelProvider`, `serviceTier`, `cwd`, `approvalPolicy`, `approvalsReviewer`, `sandbox` (`SandboxMode`), `config` (dotted-key map of config overrides), `serviceName`, `baseInstructions`, `developerInstructions`, `ephemeral`, `sessionStartSource`, `threadSource`. **[bin]**
- Thread id is `result.thread.id`. The same `Thread` object arrives again in a `thread/started` notification. **[bin][t3]**
- **[t3]** T3 sends only `cwd`, `model` and a `config` map. A per-thread env var for child processes goes in `config` as `shell_environment_policy.set.<NAME>`, because the process env is shared by all threads.
- Enums **[bin]**: `AskForApproval = "untrusted" | "on-request" | "never" | {"granular":{sandbox_approval,rules,skill_approval,request_permissions,mcp_elicitations}}`; `SandboxMode = "read-only" | "workspace-write" | "danger-full-access"`; `ApprovalsReviewer = "user" | "auto_review" | "guardian_subagent"`.

### thread/resume

```json
→ {"id":2,"method":"thread/resume","params":{"threadId":"01a0d5ef-62bd-7c20-861b-0528ff7d86bd","excludeTurns":true,"cwd":"/work/repo","model":"gpt-5","config":{…}}}
← {"id":2,"result":{ …same shape as thread/start…, "collaborationMode":…, "turnsBackwardsCursor":null,"itemsBackwardsCursor":null}}
```

- **[bin]** Params: `threadId` plus the same overrides as `thread/start` (no `ephemeral`), and `excludeTurns?: boolean` (return metadata only; page history with `thread/turns/list` and `thread/items/list`). Resolution order for a thread not running in this process: `history` > non-empty `path` > `threadId`. For a thread already running in this process the call rejoins it. Prefer `threadId`.
- **[t3]** T3 resumes with `excludeTurns: true` plus `cwd`, `model`, `config`. If the error message matches `session \S+ is archived` or `codex unarchive`, it calls `thread/unarchive {threadId}` and retries once.
- **[t3]** An active writer elsewhere surfaces as a request error (T3 maps it to a "thread in use" error); the exact message is not captured here (see Not verified).

### turn/start

```json
→ {"id":3,"method":"turn/start","params":{"threadId":"01a0…","input":[{"type":"text","text":"fix the bug","text_elements":[]}],"cwd":"/work/repo","model":"gpt-5","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"workspaceWrite","writableRoots":[],"networkAccess":false},"effort":"high","summary":"detailed","collaborationMode":{"mode":"default","settings":{"model":"gpt-5"}}}}
← {"id":3,"result":{"turn":{"id":"01a0d60e-f90f-7f23-b6ba-9ca7ee20b5cd","items":[],"itemsView":"notLoaded","status":"inProgress","error":null,"startedAt":null,"completedAt":null,"durationMs":null}}}
```

- **[bin]** Params: `threadId`, `input: UserInput[]` required; optional `clientUserMessageId`, `cwd`, `approvalPolicy`, `approvalsReviewer`, `sandboxPolicy` (a `SandboxPolicy` object, not the `SandboxMode` string), `model`, `serviceTier`, `effort`, `summary`, `outputSchema` (JSON Schema for the final message), `additionalContext` (experimental, map of `{value, kind, name?}`), `collaborationMode` (experimental, `{mode:"plan"|"default", settings:{model, reasoning_effort|null, developer_instructions|null}}`). Overrides stick for the turn and later turns.
- **[bin]** `SandboxPolicy` variants: `{"type":"dangerFullAccess"}`, `{"type":"readOnly","networkAccess":bool}`, `{"type":"externalSandbox","networkAccess":…}`, `{"type":"workspaceWrite","writableRoots":[],"networkAccess":bool,"excludeTmpdirEnvVar":bool,"excludeSlashTmp":bool}`. **[t3]** T3 sends `workspaceWrite` with only `networkAccess:false` and `writableRoots:[]`; the rest default.
- **[bin]** `UserInput` variants: `{"type":"text","text","text_elements":[]}`, `{"type":"image","url"|"fileId"}`, `{"type":"localImage","path"}`, `{"type":"audio",…}`, `{"type":"localAudio","path"}`, `{"type":"skill","name","path"}`, `{"type":"mention","name","path"}`.
- Turn id is `result.turn.id`. The result returns before `turn/started`; `startedAt` is `null` until that notification. **[t3]** A turn queued behind another has `startedAt:null` and must wait for `turn/started` before it can be interrupted.
- **[t3]** Modes used by T3: approval-required = `untrusted` + `readOnly`; auto-accept-edits = `on-request` + `workspaceWrite`; full-access = `never` + `dangerFullAccess`.

### turn/steer

```json
→ {"id":4,"method":"turn/steer","params":{"threadId":"01a0…","expectedTurnId":"01a0d5ed-670e-7032-9a87-707c736491a6","input":[{"type":"text","text":"Actually, respond with exactly: x","text_elements":[]}]}}
← {"id":4,"result":{"turnId":"01a0d5ed-670e-7032-9a87-707c736491a6"}}
```

- **[bin]** Params `threadId`, `expectedTurnId` (required precondition: fails if it is not the active turn), `input`, optional `clientUserMessageId`. Result `{turnId}`. **[t3]** recorded in `message_steering`; one `turn/started` only: the same turn continues and a second `userMessage` item (the steer text) is emitted, then a second agentMessage.
- **[bin]** The not-steerable case is a `codexErrorInfo` of `{"activeTurnNotSteerable":{"turnKind":…}}` (shape only; message text not captured).

### turn/interrupt

```json
→ {"id":5,"method":"turn/interrupt","params":{"threadId":"01a0…","turnId":"01a0d60e-f90f-…"}}
← {"id":5,"result":{}}
… thread/status/changed {"status":{"type":"idle"}} …
… turn/completed {"turn":{"status":"interrupted", …}}
```

- **[bin][t3]** The response is `{}` and arrives before `turn/completed` (recorded in `turn_interrupt_mid_tool`). The turn then completes with `status:"interrupted"`.
- **[t3]** A command that is still running stays alive: T3 follows with `thread/backgroundTerminals/terminate {threadId, processId}` using the `processId` of the `commandExecution` item, result `{terminated:boolean}`. `thread/backgroundTerminals/{list,clean}` also exist. **[bin]** (experimental)

### Others the adapter may want

All **[bin]**: `thread/list` (`cursor`, `limit`, `sortKey`, `sortDirection`, `sourceKinds`, … gives `{data:Thread[], nextCursor, backwardsCursor}`), `thread/loaded/list` (`{data:string[], nextCursor}`; verified live, empty on a fresh process), `thread/read {threadId, includeTurns?}` gives `{thread}`, `thread/turns/list`, `thread/items/list`, `thread/unsubscribe {threadId}` gives `{status:"notLoaded"|"notSubscribed"|"unsubscribed"}` (**[t3]** used so the shared server can shut a native thread down when idle), `thread/archive`, `thread/unarchive`, `thread/fork`, `thread/name/set`, `thread/compact/start`, `thread/revert`, `model/list`, `account/read`, `thread/goal/{set,get,clear}`, `review/start`.

## Server notifications

Every notification below carries `threadId`; item and turn notifications also carry `turnId`. Route by `threadId`: this is how one process serves many threads. **[bin]** shapes; **[t3]** recordings confirm order.

Observed order for a simple turn **[t3]** (`simple` fixture): `thread/started` → `thread/status/changed {active}` → `turn/started` → `item/started` (userMessage) → `item/completed` (userMessage) → `item/started` (agentMessage) → `item/agentMessage/delta` × n → `item/completed` (agentMessage) → `thread/tokenUsage/updated` → `thread/status/changed {idle}` → `turn/completed`. Interleaved noise to ignore: `mcpServer/startupStatus/updated`, `thread/settings/updated`, `account/updated`, `account/rateLimits/updated`, `configWarning`, `remoteControl/status/changed`.

| method | params | meaning |
| --- | --- | --- |
| `thread/started` | `{thread}` | thread created or loaded |
| `thread/status/changed` | `{threadId, status}` where status is `{type:"notLoaded"\|"idle"\|"systemError"}` or `{type:"active", activeFlags:("waitingOnApproval"\|"waitingOnUserInput")[]}` | idle/active, blocked-on-human flags |
| `turn/started` | `{threadId, turn}` | turn running, `turn.startedAt` set |
| `turn/completed` | `{threadId, turn}` | `turn.status` is `"completed"\|"interrupted"\|"failed"` (`"inProgress"` only before); `turn.error` is `{message, codexErrorInfo, additionalDetails, misalignment}\|null`; `turn.items` is empty (items arrived as notifications), `durationMs`, `completedAt` |
| `item/started` | `{item, threadId, turnId, startedAtMs}` | `item` is a `ThreadItem`, see below |
| `item/completed` | `{item, threadId, turnId, completedAtMs, contentTokenCount?}` | final item state |
| `item/agentMessage/delta` | `{threadId, turnId, itemId, delta}` | assistant text chunk; append by `itemId` |
| `item/reasoning/summaryTextDelta` | `{…, itemId, delta, summaryIndex}` | reasoning summary text |
| `item/reasoning/summaryPartAdded` | `{…, itemId, summaryIndex}` | new summary part |
| `item/reasoning/textDelta` | `{…, itemId, delta, contentIndex}` | raw reasoning text |
| `item/plan/delta` | `{…, itemId, delta}` | proposed-plan text (experimental; the completed `plan` item is authoritative) |
| `turn/plan/updated` | plan step list | todo/plan update |
| `item/commandExecution/outputDelta` | `{…, itemId, delta}` | command stdout/stderr chunk |
| `item/fileChange/patchUpdated` | `{…, itemId, changes: FileUpdateChange[]}` | patch contents while a `fileChange` item runs (`outputDelta` for fileChange is deprecated and no longer emitted) |
| `thread/tokenUsage/updated` | `{threadId, turnId, tokenUsage:{total, last, modelContextWindow}}` | `total`/`last` are `{totalTokens, inputTokens, cachedInputTokens, cacheWriteInputTokens, outputTokens, reasoningOutputTokens}` |
| `error` | `{error:{message, codexErrorInfo, additionalDetails, misalignment}, willRetry, threadId, turnId}` | provider error; `willRetry:true` means Codex retries, `false` is final and `turn/completed` with `status:"failed"` follows |
| `serverRequest/resolved` | `{threadId, requestId}` | a server request was answered (or cancelled); fires after your response |
| `model/rerouted` | model change | model substituted |
| `thread/closed`, `thread/name/updated`, `thread/archived` | thread lifecycle | |

`codexErrorInfo` is a string (`"usageLimitExceeded"`, `"rateLimitExceeded"`, `"contextWindowExceeded"`, `"unauthorized"`, `"serverOverloaded"`, `"internalServerError"`, `"sandboxError"`, `"badRequest"`, `"other"`, …) or an object (`{"httpConnectionFailed":{"httpStatusCode":n|null}}`, `{"responseStreamDisconnected":{…}}`, `{"activeTurnNotSteerable":{"turnKind":…}}`). **[bin]**

### ThreadItem (`item.type`) **[bin]**

All have `id`. Mapping to agent.threads: `userMessage` → user text; `agentMessage` → assistant text; `reasoning` → thinking; `commandExecution` and `fileChange` and `mcpToolCall` and `dynamicToolCall` and `webSearch` → tool items.

- `userMessage {id, clientId, content: UserInput[]}`
- `agentMessage {id, text, phase: "final_answer"|…|null, memoryCitation, delivery, questions, moderation}`: `text` is complete only on `item/completed`.
- `reasoning {id, summary: string[], content: string[]}`
- `plan {id, text}`
- `commandExecution {id, command, cwd, processId, source, status, commandActions[], aggregatedOutput|null, exitCode|null, durationMs|null}`: `status` is `"inProgress"|"completed"|"failed"|"declined"`. `command` is the full shell line (`/bin/bash -lc "…"`). `commandActions[]` is `{type:"read"|"listFiles"|"search"|"unknown", command, …}`. Started item has `aggregatedOutput/exitCode/durationMs` null; the completed item fills them. **[t3]** recorded.
- `fileChange {id, changes:[{path, kind:{type:"add"|"delete"|"update", move_path}, diff}], status}`: `status` is `"inProgress"|"completed"|"failed"|"declined"`.
- `mcpToolCall {id, server, tool, status, arguments, result|null, error|null, durationMs}`
- `dynamicToolCall {id, namespace, tool, arguments, status, contentItems|null, success|null, durationMs}`
- also `webSearch`, `imageView`, `imageGeneration`, `collabAgentToolCall`, `subAgentActivity`, `contextCompaction`, `enteredReviewMode`, `exitedReviewMode`, `hookPrompt`, `functionCallOutput`, `sleep`.

## Server requests (approvals and questions)

These arrive as requests with an `id`; the turn blocks until the client answers. The thread shows `activeFlags:["waitingOnApproval"]` or `["waitingOnUserInput"]` meanwhile. Answer with `{"id":<same id>,"result":{…}}`. After the reply the server emits `serverRequest/resolved`. **[t3]** T3 answers `acceptForSession` itself when the user already granted that kind for the session (a local allowance per thread and request kind kept by T3, not by Codex).

### item/commandExecution/requestApproval **[bin][t3]**

```json
← {"id":0,"method":"item/commandExecution/requestApproval","params":{"kind":"command","threadId":"01a0…","turnId":"01a0…","itemId":"exec-02c3…","startedAtMs":1790297002138,"environmentId":"local","reason":"May I create …?","command":"/bin/bash -lc \"printf … > f.txt\"","cwd":"/work/repo","commandActions":[{"type":"unknown","command":"printf … > f.txt"}],"proposedExecpolicyAmendment":["/bin/bash","-lc","printf … > f.txt"],"availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["/bin/bash","-lc","…"]}},"cancel"]}}
→ {"id":0,"result":{"decision":"accept"}}
```

- Params: `kind: "command"|"humanCommand"|"writeStdin"`, `threadId`, `turnId`, `itemId`, `startedAtMs`, optional `approvalId` (set for sub-command and stdin callbacks, a UUID; use `approvalId ?? itemId` as the request key), `environmentId`, `reason`, `command`, `cwd`, `commandActions`, `networkApprovalContext`, `proposedExecpolicyAmendment: string[]`, `proposedNetworkPolicyAmendments`, `availableDecisions` (experimental; offer only these).
- Response `{decision}`: `"accept"` | `"acceptForSession"` | `"decline"` | `"cancel"` | `{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":string[]}}` | `{"applyNetworkPolicyAmendment":{"network_policy_amendment":…}}`.

### item/fileChange/requestApproval **[bin][t3]**

```json
← {"id":0,"method":"item/fileChange/requestApproval","params":{"threadId":"…","turnId":"…","itemId":"exec-114e…","startedAtMs":1790297014297,"reason":null,"grantRoot":null}}
→ {"id":0,"result":{"decision":"accept"}}
```

- `decision`: `"accept"` | `"acceptForSession"` | `"decline"` | `"cancel"`. Diff content is in the `fileChange` item (`item/started`, `item/fileChange/patchUpdated`), not in this request.

### item/permissions/requestApproval **[bin][t3]**

```json
← {"id":3,"method":"item/permissions/requestApproval","params":{"threadId":"…","turnId":"…","itemId":"…","environmentId":"local","startedAtMs":…,"cwd":"/work/repo","reason":"…","permissions":{"network":{"enabled":true},"fileSystem":{"read":null,"write":["/data"]}}}}
→ {"id":3,"result":{"permissions":{"network":{"enabled":true},"fileSystem":{"read":null,"write":["/data"]}},"scope":"turn"}}
```

- Grant: echo `params.permissions` back (or a subset) with `scope: "turn"|"session"`. Deny: `{"permissions":{},"scope":"turn"}` (this is what T3 sends). Optional `strictAutoReview: boolean`.

### item/tool/requestUserInput (experimental) **[bin][t3]**

```json
← {"id":0,"method":"item/tool/requestUserInput","params":{"threadId":"…","turnId":"…","itemId":"call_wGA0…","questions":[{"id":"schema_preference","header":"Fixture","question":"Strict or flexible?","isOther":true,"isSecret":false,"options":[{"label":"Strict schemas (Recommended)","description":"…"},{"label":"UI flexibility","description":"…"}]}],"isBlocking":true,"autoResolutionMs":null}}
→ {"id":0,"result":{"answers":{"schema_preference":{"answers":["Strict schemas (Recommended)"]}}}}
```

- `options` may be `null` (free text). `answers` is keyed by question `id`, each value `{answers: string[]}`; the answer is the option `label` or free text. T3 drops ids that were not asked. `autoResolutionMs` is deprecated; use `isBlocking`.

### mcpServer/elicitation/request **[bin]**

Params `{threadId, turnId|null, serverName, environmentId, mode, …}` with `mode` one of `"form"` (`message`, `requestedSchema`), `"url"` (`message`, `url`, `elicitationId`), `"openai/form"`, `"openaiForm"`, `"openai/userVerification"`. Response `{action:"accept"|"decline"|"cancel", content: JSON|null, _meta: JSON|null}`. For a headless executor, answer `{"action":"decline","content":null,"_meta":null}` unless the phone UI handles it.

### Other server requests the client must answer or reject **[bin]**

`item/tool/call` (dynamic tool, only if the client registered dynamic tools), `account/chatgptAuthTokens/refresh`, `attestation/generate` (only when `requestAttestation` is true), and the legacy `execCommandApproval` / `applyPatchApproval`. The legacy pair take `{decision}` with `ReviewDecision = "approved" | "approved_for_session" | "abort" | {"denied":{"rejection":string}} | …` and carry `conversationId` and `callId` instead of `threadId` and `itemId`; T3 still registers handlers for them. Answer an unhandled server request with an error response, never silence. **[t3]** T3 replies `{"code":-32601,"message":"Method not found: <method>"}` for a method it has no handler for.

## Process and failure behaviour

- **[t3]** Child exit is the only crash signal; stdout closing precedes the exit status by a few ms (T3 waits 250 ms for the exit code and keeps the last 8 KiB of stderr for the message). All pending requests fail on exit.
- **[t3]** A notification that does not decode against the generated schema is dropped with one warning per distinct shape, so a CLI newer than the adapter keeps working. Do the same: ignore unknown fields and unknown methods.
- **[t3]** Generated schemas drift per release: T3 patches `turn/start` params to add `collaborationMode` and `additionalContext`, which the stable schema omits. Generate with `--experimental` when typing params.
- **[bin]** Thread rollout files live under `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl` (`thread.path`); `ephemeral:true` threads are not written.

## Minimal turn, end to end

```
spawn: codex app-server            (stdio, one per workspace and CLI)
→ initialize / ← result / → initialized          once per process
→ thread/start {cwd,model,config}  ← {thread.id}   once per conversation
  or thread/resume {threadId,excludeTurns:true,cwd,model,config}
→ turn/start {threadId,input,…}    ← {turn.id}
← item/agentMessage/delta …        ← item/completed …
← (server requests: answer by id)
→ turn/steer {threadId,expectedTurnId,input}     while the turn is active
→ turn/interrupt {threadId,turnId}               ← {} then turn/completed{interrupted}
← thread/tokenUsage/updated        ← turn/completed {status}
```

## How the service speaks it

The service keeps the live process and the executor only carries its lines (a relay and a recorder: nothing about a conversation is decided at the executor).

- **Executor.** `session_open {session, program: "codex", cwd?}` starts `codex app-server` in the workspace's directory (or the worktree's) with the executor's home, answers `{kind: "session.started"}` as an event once the process has its input, streams its stdout, and ends when the process does (`{exit_code, cancelled}`). `session_send {session, line}` writes one line. A cancel of the open action kills the process group, so what the CLI started dies with it, and a process left holding the output after the CLI exited is killed once the output has been quiet (`clis/rca-exec/src/adapters/sessions.rs`, `drive` in `local_executor.rs`).
- **Service.** `cli-driver` holds the protocol (`domain/codex.rs`: the requests, the lines, the notifications as neutral events, the approval answers) and `CodexSessions` (`application/codex_sessions.rs`), one server per workspace with every thread of the workspace in it. The `Linker` port starts the process: `HostLinker` over `session_open` and `session_send` in `group-agent`, `Processes` in `host` for a process on the service's own machine.
- **Turn.** `thread/start` (the thread's id is announced as the session the engine keeps) or `thread/resume` by the stored id, then `turn/start` with the approval policy and sandbox of the thread's mode (`approval-required`: `untrusted` and `readOnly`; `auto-accept-edits`: `on-request` and `workspaceWrite`; `full-access`: `never` and `dangerFullAccess`; a plan: `never` and `readOnly`). `agentMessage` deltas are the text (a message that arrives only as its completed text is said whole), a `commandExecution` is a call named `command`, a `fileChange` is one call per change named for its kind (`create_file`, `Edit`, `delete_file`, which the log turns into `file.changed`), an `mcpToolCall` is `mcp__<server>__<tool>`, reasoning summaries are reasoning.
- **Steer and stop.** `turn.steer` is `turn/steer` on the live process, and the turn goes on as the same turn; a refused steer is an error in the log and a refusal. `turn.interrupt` is `turn/interrupt`; the turn ends `interrupted` when the server says so, and a server that does not end it within 15 seconds is killed.
- **Approvals.** Command, file change and permission requests of the server are `approval.requested` events (`tool` `command`, `file_change`, `permissions`), answered by `approval.answer` on the live process; `accept_for_session` is the server's own `acceptForSession`. A request of any other kind is refused (`mcpServer/elicitation/request` is declined, the rest are a method not found), because the service does not opt into `experimentalApi`, so `item/tool/requestUserInput` is never sent.
- **Life of the process.** Closed after `RCA_CODEX_IDLE_SECS` (default 600, 0 never) with no turn running, replaced when it dies, and every thread of a dead process is resumed by its stored conversation id on its next turn. A turn that was running when the process died ends failed with why; it is never run again.
- **Not routed through the model gateway** (`docs/gaps/codex-session-not-through-gateway.md`), and **not yet run against the real CLI** (`docs/gaps/codex-live-session-not-run-against-the-real-cli.md`; the `codex-session-proof` job, `docs/solutions/testing/real-codex-session.md`, makes the first run on `dev`).

## Not verified

- No model turn, login, or thread was run here: every notification, item and approval shape comes from the 0.171.0 generated schema or the 0.156.1 T3 recordings, not from a live 0.171.0 turn. Verified live against 0.171.0: stdio newline JSON without `jsonrpc`, the `initialize` result, `Already initialized` (-32600), `thread/loaded/list`, the unknown-method and bad-thread-id errors.
- The 0.156.1 recordings predate 0.171.0: `availableDecisions` (experimental) is in both the 0.171.0 schema and the recordings, but `Thread` has gained fields since (`canAcceptDirectInput`, `daybreakEnabled`, `environments` appear in the recording, not the stable 0.171.0 TS). Treat unknown fields as ignorable.
- What `decline` versus `cancel` do to the turn (names and shapes only; semantics not in the schema or a recording).
- Exact error text for: `turn/steer` on a finished turn or wrong `expectedTurnId` (only the `activeTurnNotSteerable` info shape is known), `thread/resume` of a thread with another active writer, `thread/resume` of an id with no rollout, `turn/start` while another turn is active (T3 relies on it being queued; the queue behaviour is not shown in a recording).
- Whether `thread/resume` for a thread already loaded in the same process re-emits `thread/started` (the schema says it rejoins; no recording).
- Whether `approvalPolicy` and `sandbox` on `thread/start` (not per-turn) take effect without a per-turn override: T3 sets them per turn only; the response of `thread/start` shows server defaults (`on-request` + `workspaceWrite` in the recording).
- Behaviour with several threads active at once in one process under load (T3 does this; no recording of interleaved turns across threads was read).
- `item/tool/call` dynamic tools, `attestation/generate`, auth refresh: only types read, no flow.
- `ws://` and `unix://` listen transports: from `--help` only, not run. The `app-server daemon` and `proxy` subcommands were not examined.
