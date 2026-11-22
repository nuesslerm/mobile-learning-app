---
title: Claude Code stream-json session protocol, as the rca executor must speak it
date: 2026-11-22
category: architecture
module: backend/remote-chat-agent
problem_type: architecture_pattern
component: development_workflow
severity: medium
tags: [architecture, rca, rca-exec, claude-code, stream-json, control-protocol]
applies_when:
  - "writing or changing the Claude Code adapter that hosts a long-lived claude process at the executor"
  - "mapping Claude Code stdout frames to agent.threads events"
  - "answering a Claude Code permission request or an AskUserQuestion on the live stream"
related_components: [tooling]
---

# Claude Code stream-json session protocol

One `claude -p --input-format stream-json --output-format stream-json` process hosts one conversation. The executor starts one per thread, keeps it for the life of the thread, writes each phone message to its stdin and reads every event from its stdout. Approvals, steer and interrupt are frames on the same two pipes: no MCP approval tool, no process per turn. This note is what the adapter codes against. Codex works differently (one `app-server` process hosts many threads): see `codex-app-server-protocol.md`.

Sources, tagged on every fact below:

- **[bin]** the installed `claude 2.1.330` (`/opt/claude-code/bin/claude`, the one on `PATH`): `claude --help`, the zod schemas and `.describe(...)` texts embedded in the binary (read with `grep -a`; the protocol is documented in them), and live stdio probes. The probes ran with `env -i`, `HOME` and `CLAUDE_CONFIG_DIR` under `/dev/shm`, no credential, and only control requests on stdin: no user message, no model turn. Live-verified facts are marked **[live]**.
- **[sdk]** `/opt/node22/lib/node_modules/@anthropic-ai/claude-code/sdk-tools.d.ts` (package 2.1.42, an older install): tool input types only (`AskUserQuestionInput`, `ExitPlanModeInput`). The Agent SDK package (`@anthropic-ai/claude-agent-sdk`, with `sdk.d.ts`) is not installed anywhere in the repo or the machine; `references/t3code` and `references/orca` have no `node_modules`.
- **[t3]** `references/t3code/apps/server/src/orchestration-v2/Adapters/ClaudeAdapterV2.ts` (drives the SDK `query()`, which in turn spawns the CLI over stream-json) and the recorded transcripts `testkit/fixtures/*/claude_transcript.ndjson` (Claude Code 2.1.111 to 2.1.281). The transcripts are SDK-level: inbound `emit_inbound` frames are the CLI's stdout lines verbatim; the outbound `prompt.offer`, `permission.request`, `permission.response` and `query.interrupt` entries are SDK calls, which this note maps to wire frames using [bin].
- **[orca]** `references/orca/src` (`claude-model-list-probe.ts`, the SDK launch assertions in `orcad-packaged-structured-chat.integration.test.ts`, `claude-full-access-policy.ts`, `claude-result-outcome.ts`).

## Transport and framing

- **[live]** One JSON object per line (`\n`), UTF-8, both directions. Stderr is log text, not protocol. Stdout carries only frames in stream-json mode.
- **[live]** `-p` with `--output-format stream-json` requires `--verbose` (`Error: When using --print, --output-format=stream-json requires --verbose`). `--replay-user-messages` requires both formats to be `stream-json`.
- **[live]** Closing stdin (EOF) ends the process with exit code 0 once it is idle. A startup failure exits 1 and writes a `result` frame first (see Resume).
- **[bin]** `{"type":"keep_alive"}` may arrive at any time, in either direction: ignore it.
- **[bin]** Every control frame has this envelope; the two directions are symmetric (the CLI also sends requests, the host answers them):
  - request: `{"type":"control_request","request_id":"<id>","request":{"subtype":"…", …}}`
  - success: `{"type":"control_response","response":{"subtype":"success","request_id":"<same id>","response":{…}}}`
  - error: `{"type":"control_response","response":{"subtype":"error","request_id":"<same id>","error":"…","error_code":"…"}}`
  - withdrawal: `{"type":"control_cancel_request","request_id":"<id>"}` (a side no longer needs the answer to one of its own requests)
- **[bin]** Request ids are free strings chosen by the sender; echo the id you were sent. Ignore unknown fields and unknown frame types (the success payload also carries `pending_permission_requests`, `pending_user_dialog_requests`, `pending_elicitation_requests`).

## 1. The invocation

```
claude -p --input-format stream-json --output-format stream-json --verbose \
  --include-partial-messages --replay-user-messages \
  --permission-prompt-tool stdio --session-id <uuid> \
  --permission-mode default --model <alias|id> \
  [--mcp-config <json|file>… [--strict-mcp-config]] [--setting-sources user,project,local]
```

| flag | meaning, and whether the executor needs it |
| --- | --- |
| `-p` / `--print` | headless mode; required for the stream-json formats **[bin]** |
| `--input-format stream-json` | read user messages and control frames from stdin for the life of the process (`text` reads one prompt then stops) **[bin][live]**. Required |
| `--output-format stream-json` | write one frame per line **[live]**. Required; needs `--verbose` |
| `--verbose` | required by stream-json output **[live]** |
| `--include-partial-messages` | adds `stream_event` frames (text, thinking and tool-input deltas) **[bin]**. Needed for live typing on the phone; without it the first output is the complete `assistant` frame |
| `--replay-user-messages` | re-emits each stdin user message on stdout as a `user` frame with `"isReplay":true`, as the acknowledgement **[bin][orca]**. Needed to confirm that a message was taken; pair it with a `uuid` on the message |
| `--permission-prompt-tool stdio` | permission prompts reach the host as `control_request` `can_use_tool` over the same stdio; this is how the SDK runs when it has a `canUseTool` callback **[bin][orca]**. The value `stdio` is accepted by 2.1.330 **[live]** (the option is hidden from `--help`; `--permission-prompts host` is the default and is documented as "the SDK host or --permission-prompt-tool"). Any other value names an MCP tool and is not wanted here. Needed |
| `--permission-prompts none` | nobody is asked; whatever would prompt is denied **[bin]**. Not wanted |
| `--session-id <uuid>` | the conversation id, chosen by the host; must be a UUID (`Error: Invalid session ID. Must be a valid UUID.`) **[live]**. Use it on the first start of a thread, so the id is known before any frame arrives. A second start with an id that already has a transcript fails (`Session ID … is already in use.` **[bin]**): use `--resume` then |
| `--resume <uuid>` | reload the transcript of that conversation and continue it **[bin]**. Use on every restart of a thread. Combined with `--session-id` it needs `--fork-session` (`Error: --session-id with --continue or --resume also needs --fork-session.`) **[live]**: never combine |
| `--fork-session` | with `--resume`, start a new session id from the old transcript **[bin]**. Not used |
| `--permission-mode <m>` | initial mode **[bin]**: `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk`, `plan`; `default` is accepted too **[live]** (the control protocol names `default`, `acceptEdits`, `bypassPermissions`, `plan`, `dontAsk`, `auto`). Change it later with `set_permission_mode`, not by restarting |
| `--model <m>` | alias (`sonnet`, `opus`, …) or full id; change it later with `set_model` **[bin][live]** |
| `--mcp-config <json\|file>…` | MCP servers for this process, inline JSON strings or files; `--strict-mcp-config` ignores every other source **[bin]**. Needed only for tools the executor itself attaches (the approval tool is not one of them any more). A stdio server config is reported by `mcp_status` as `status:"pending"` until it connects **[live]**. Servers can be changed at runtime with `mcp_set_servers` **[bin]** |
| `--include-hook-events`, `--forward-subagent-text` | more frames (hook lifecycle; subagent text and thinking with `parent_tool_use_id` set) **[bin]**. Optional |
| `--no-session-persistence` | transcript not saved, so no resume **[bin]**. Never use |
| `--setting-sources`, `--settings`, `--add-dir`, `--append-system-prompt`, `--effort` | context and behaviour, as in interactive use **[bin]** |

- **[live]** A process started with `--resume <unknown uuid>` exits 1 after one frame: `{"type":"result","subtype":"error_during_execution","is_error":true,"num_turns":0,"session_id":"<the id asked for>","errors":["No conversation found with session ID: <id>"], …}`; stderr has the same text.
- **[orca]** What the SDK itself passes (assertion in Orca's packaged-server test, SDK 0.3.x): `--input-format stream-json --output-format stream-json --verbose --include-partial-messages --replay-user-messages --permission-prompt-tool=stdio --session-id=<uuid> --setting-sources=user,project,local`, plus `--dangerously-skip-permissions` for a full-access launch. Our argv above is the same set.
- **[bin]** Environment: `CLAUDE_CONFIG_DIR` selects the state directory (settings, transcripts; reported as `user_output_styles_dir` in `initialize`). Spawn with an explicit env, not the executor's own: a process started inside another Claude Code session inherits its variables.

## 2. stdin: what the host writes

### Initialize (once, right after spawn)

```json
→ {"type":"control_request","request_id":"1","request":{"subtype":"initialize"}}
← {"type":"control_response","response":{"subtype":"success","request_id":"1","response":{"commands":[…],"agents":[…],"output_style":"default","available_output_styles":[…],"models":[{"value":"default","resolvedModel":"claude-sonnet-5-5","displayName":"Default (recommended)","description":"Sonnet 5.5 · Efficient for routine tasks","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"],"supportsAdaptiveThinking":true,"supportsAutoMode":true}, …],"account":{"subscriptionType":"…","apiProvider":"firstParty"},"pid":28971,"claude_code_version":"2.1.330","current_permission_mode":"default","session_state":"idle","capabilities":["ui_surface_v1","reload_plugins_refuse_if_busy_v1", …],"fast_mode_state":"off", …}}}
```

- **[live]** A bare `{"subtype":"initialize"}` is answered with the object above (keys: `commands`, `agents`, `output_style`, `available_output_styles`, `models`, `account`, `pid`, `claude_code_version`, `current_permission_mode`, `session_state`, `capabilities`, `fast_mode_state`, `hook_on_failure_supported`, `feedback_mode`, and more). No `system/init` frame is sent for it: that frame comes with a turn.
- **[bin]** Optional fields of the request, all optional: `hooks`, `sdkMcpServers`, `mcpServers` (only under `--await-initialize`), `systemPrompt`, `appendSystemPrompt`, `agents`, `title`, `skills`, `promptSuggestions`, `forwardSubagentText`, `perTaskStopAffordance`, `rapidFollowupPreempt`, `supportedDialogKinds`, `jsonSchema`. The executor sends none of them except possibly `title`.
- **[bin]** `capabilities` (here and on `system/init`) is an open set for feature detection: `interrupt_receipt_v1` (the interrupt reply carries `still_queued`), `interrupt_cancel_queued_v1`, `ask_user_question_unanswered_v1`, `queued_notifications`, `reload_plugins_refuse_if_busy_v1`. The initialize reply lists only the members already settled at connect time (a subset); read `system/init.capabilities` for the rest.
- **[bin]** A repeated `initialize` (a reconnecting host) answers again and puts the requests still waiting for an answer in `response.pending_permission_requests` (same shape as a `can_use_tool` frame); **[orca]** reads that field for the same reason.
- **[live]** The unknown-subtype answer: `{"type":"control_response","response":{"subtype":"error","request_id":"r4","error":"Unsupported control request subtype: bogus_subtype","error_code":"unsupported_request"}}`.
- Not verified: whether a user message sent before any `initialize` works (the SDK always sends one first).

### User message (one JSON line per phone message)

```json
→ {"type":"user","message":{"role":"user","content":[{"type":"text","text":"fix the bug in parser.rs"}]},"parent_tool_use_id":null,"uuid":"6b0b2f0e-6d0e-4a0a-9a52-8d54b4b9a4de"}
```

- **[bin]** Schema: `type:"user"`, `message` (an Anthropic user message: `content` is a string or an array of content blocks `text`, `image` with a base64 `source`, `document`), `parent_tool_use_id` (required, `null` for the main thread), and optional `uuid`, `session_id`, `priority` (`"now"|"next"|"later"`), `shouldQuery` (`false` appends to the transcript without starting a turn), `isSynthetic`, `origin`, `timestamp`. **[t3]** T3 offers `{"type":"user","message":{"role":"user","content":"…"},"parent_tool_use_id":null}` (string content; a `uuid` in the lifecycle recordings) and `priority:"now"` for a steer. **[orca]** Orca's recorded message has `content:[{"type":"text","text":"Hello packaged server"}]`.
- **[t3]** Always send a `uuid` of your own: it is the key of everything the CLI reports back about that message (`command_lifecycle.command_uuid`, `result.user_message_uuids`, the replay echo). A message without a `uuid` gets no lifecycle frames. **[bin]**
- **[t3]** Slash commands and skills are plain text in the message (`/compact`, `/clear`); the CLI dispatches them unless the message sets `client_composed`.

### A second message while a turn is running

Three behaviours, selected by `priority`:

| `priority` | behaviour |
| --- | --- |
| absent (plain) | queued behind the running turn, FIFO: `command_lifecycle` `queued` at once, `started` when its own turn begins, `completed` after that turn's `result`. **[t3]** recorded (`claude_background_wake_before_queued_prompt`: prompt 3 is offered during a wake turn, shows `queued`, then `started` after that turn's `result`). This is the "queue" button of the phone UI |
| `"now"` | **steer**: aborts what the turn is doing (model streaming or tools) and runs the new message next, in the same process and conversation. **[t3]** recorded (`message_steering`, below). Running shell commands are moved to the background, not killed **[bin]** |
| `"later"` | explicit queue behind everything, immune to a rapid-follow-up preempt **[bin]** |

Recorded steer **[t3]** (`message_steering`, Claude Code 2.1.111), frames trimmed:

```json
→ {"type":"user","message":{"role":"user","content":"Respond with exactly: steering fixture initial response"},"parent_tool_use_id":null}
→ {"type":"user","message":{"role":"user","content":"Actually, respond with exactly: steering fixture observed"},"parent_tool_use_id":null,"priority":"now"}
← {"type":"system","subtype":"init","session_id":"05f72f7d-…","claude_code_version":"2.1.111","permissionMode":"bypassPermissions", …}
← {"type":"result","subtype":"error_during_execution","is_error":true,"stop_reason":null,"terminal_reason":"aborted_streaming","result":null,"num_turns":1, …}
← {"type":"system","subtype":"init","session_id":"05f72f7d-…", …}
← {"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"steering fixture observed"}], …}, …}
← {"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","terminal_reason":"completed","result":"steering fixture observed","num_turns":1, …}
```

- One steer therefore produces two `result` frames: an aborted one (`is_error:true`, `terminal_reason` `aborted_streaming` or `aborted_tools`) for the superseded segment, then the real one. **[t3]** T3 remembers that it steered, excludes the aborted result from "the turn ended" and bills its usage as a segment. The adapter must do the same: a `result` with an `aborted_*` reason right after a steer is not the end of the phone's turn. **[orca]** `claudeResultOutcome` reads an aborted reason as `cancellation`, not failure.
- **[t3]** T3 first moves foreground subagents to the background (`backgroundForegroundSubagents`, with a deadline) and only then offers the `priority:"now"` message; it refuses a steer when the turn already ended (a late steer would start a turn nobody asked for) and the orchestrator starts a normal follow-up turn instead.
- **[bin]** The CLI may also stop a turn by itself for a rapid follow-up (`system/turn_preempted`), only for a host that declared `rapidFollowupPreempt` in `initialize`. The executor does not declare it.
- **[bin]** `cancel_async_message` (a control request) withdraws a queued message by uuid; the reply says `cancelled:true|false`.

### How a turn ends

- **[t3]** `result` is the end of a turn. Every recorded turn ends with exactly one `result` frame (`subtype` `success`, or `error_during_execution`), followed by the `command_lifecycle` `completed` of its message.
- **[t3]** The process stays alive and idle after `result`. The next user message on the same stdin starts the next turn: a new `system/init` (same `session_id`) and so on. The `multi_turn` recording has two turns on one process with the same session id and two `init` frames.
- **[bin]** `result.user_message_uuids` lists the uuids of the messages that turn consumed (several messages can coalesce into one turn); `result.queued_turn_count` counts user sends still waiting. **[t3]** A `result` also arrives for turns nobody on the phone started: a background subagent finishing wakes the model (`origin:{"kind":"task-notification"}`, `user_message_uuids:null`, recorded in `claude_background_wake_before_queued_prompt`). Match results to phone messages with `user_message_uuids`, never by "the next `result`".

## 3. stdout: what the host reads

All frames carry `session_id` and most carry `uuid`. Sub-agent frames carry `parent_tool_use_id` (the tool_use that started them); main-thread frames have `null`.

| `type` / `subtype` | when | fields that matter | map to agent.threads |
| --- | --- | --- | --- |
| `system` / `init` | start of every turn **[t3]** | `session_id`, `model`, `permissionMode`, `cwd`, `tools[]`, `mcp_servers[{name,status}]`, `slash_commands[]`, `claude_code_version`, `capabilities[]`, `output_style`, `plugins[]`, `fast_mode_state` | thread opened/turn begins; take `session_id` (see section 7) |
| `system` / `status` | around model requests **[t3]** | `status:"requesting"`; also `{"status":null,"permissionMode":"acceptEdits"}` after `set_permission_mode` **[live]** | busy indicator; mode change |
| `system` / `session_state_changed` | **[bin]** | `state: idle \| running \| requires_action` | thread status (requires_action = waiting on a human) |
| `command_lifecycle` | per uuid-stamped message **[t3]** | `command_uuid`, `state: queued \| started \| completed \| cancelled \| discarded \| refused` | queue state of a phone message |
| `user` with `isReplay:true` | echo of a stdin message when `--replay-user-messages` **[orca]** | `message`, `uuid` | ack (the message left the pipe) |
| `stream_event` | with `--include-partial-messages` **[t3]** | `event` is the Anthropic stream event, see below | streaming deltas |
| `assistant` | one per finished content block **[t3]** | `message.content[]` of one block: `text`, `thinking` (`signature` is opaque), `tool_use {id,name,input}`; `message.usage`; `message.id` is shared by the blocks of one API message | assistant text, reasoning, tool call start |
| `user` (no `isReplay`) | tool results **[t3]** | `message.content:[{"type":"tool_result","tool_use_id","content","is_error"}]`, top-level `tool_use_result` (structured output) | tool result |
| `rate_limit_event` | when limits change **[t3][bin]** | `rate_limit_info:{status:"allowed"\|"allowed_warning"\|"rejected", resetsAt, rateLimitType, utilization, overageStatus, …}` | rate limit notice |
| `system` / `compact_boundary` | after compaction **[t3]** | compaction metadata | context compacted |
| `system` / `task_started`, `task_progress`, `task_updated`, `task_notification`, `background_tasks_changed` | subagents and background shells **[t3]** | `task_id`, `tool_use_id`, `description`, `patch.status`, `usage` | subagent / background task rows |
| `system` / `thinking_tokens`, `hook_started`, `hook_response`, `commands_changed`, `api_retry`, `informational`, `notification`, `permission_denied` | noise or notices **[t3][bin]** | | ignore, or surface `api_retry` |
| `result` | end of a turn | see below | turn completed / failed / interrupted |
| `control_request` | CLI asks the host | `can_use_tool`, `request_user_dialog`, `hook_callback`, `mcp_message`, … | approval / question |
| `control_response`, `control_cancel_request`, `keep_alive` | | | |

Stream events **[t3]** (with `--include-partial-messages`; `parent_tool_use_id:null` for the main thread):

```json
{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_…","role":"assistant","content":[],"usage":{…}}},"session_id":"…","parent_tool_use_id":null,"uuid":"…"}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}, …}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"compact"}}, …}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}, …}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{…}}, …}
{"type":"stream_event","event":{"type":"message_stop"}, …}
```

Other deltas seen: `thinking_delta {thinking}` (with `thinking-display` summarized), `signature_delta`, `input_json_delta {partial_json}` for a `tool_use` block (`content_block_start` carries `{"type":"tool_use","id":"toolu_…","name":"Agent","input":{}}`). Key a streaming block by `message.id` + `index`; the later `assistant` frame carries the same block complete.

Assistant tool call, from the denied-write recording **[t3]** (`tool_call_denied_write`, Claude Code 2.1.268):

```json
{"type":"assistant","message":{"model":"claude-sonnet-4-6","id":"msg_011C…","role":"assistant","content":[{"type":"tool_use","id":"toolu_016StH6rgvnkDxU2MWt46gYE","name":"Write","input":{"file_path":"/work/.claude-denied-write.txt","content":"approval fixture"},"caller":{"type":"direct"}}],"stop_reason":null,"usage":{"input_tokens":3,"cache_creation_input_tokens":7276,"cache_read_input_tokens":14316,"output_tokens":8}},"parent_tool_use_id":null,"session_id":"d745e7a5-…","uuid":"60cdca3f-…","wire_tool_inputs":{"toolu_016S…":{"file_path":"…","content":"approval fixture"}}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"User declined tool execution.","is_error":true,"tool_use_id":"toolu_016StH6rgvnkDxU2MWt46gYE"}]},"parent_tool_use_id":null,"session_id":"d745e7a5-…","uuid":"44c0feee-…","tool_use_result":"Error: User declined tool execution.","tool_result_meta":[{"id":"toolu_016S…","non_execution_kind":"permission-rule"}]}
```

Result, success **[t3]** (trimmed; 2.1.268):

```json
{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","terminal_reason":"completed","result":"write permission denied","num_turns":2,"duration_ms":10294,"duration_api_ms":11021,"total_cost_usd":0.0605,"usage":{"input_tokens":4,"cache_creation_input_tokens":7549,"cache_read_input_tokens":35908,"output_tokens":224,"output_tokens_details":{"thinking_tokens":69}},"modelUsage":{"claude-sonnet-4-6":{"inputTokens":4,"outputTokens":224,"cacheReadInputTokens":35908,"cacheCreationInputTokens":7549,"costUSD":0.0594,"contextWindow":200000,"maxOutputTokens":32000}},"permission_denials":[{"tool_name":"Write","tool_use_id":"toolu_016S…","tool_input":{…}}],"api_error_status":null,"queued_turn_count":0,"result_index":0,"session_id":"d745e7a5-…","uuid":"6addbf20-…"}
```

- **[t3][bin]** `is_error` is the failure flag, not `subtype`: a failed API call is `subtype:"success"` with `is_error:true`, `api_error_status:401` and `result:"Failed to authenticate. API Error: 401 …"` (recorded, `claude_result_is_error`; the next turn on the same process succeeded). `subtype` values: `success`, `error_during_execution` (also the aborted and the bad-resume result), `error_max_turns`, `error_max_budget_usd`, `error_max_structured_output_retries`.
- **[bin]** `terminal_reason` values: `completed`, `aborted_streaming`, `aborted_tools`, `max_turns`, `blocking_limit`, `rapid_refill_breaker`, `prompt_too_long`, `image_error`, `model_error`, `api_error`, `malformed_tool_use_exhausted`, `stop_hook_prevented`, `hook_stopped`, `tool_deferred`, `background_requested`, `budget_exhausted`, `structured_output_retry_exhausted`, `tool_deferred_unavailable`, `turn_setup_failed`.
- **[t3]** Usage: `usage` and `total_cost_usd` on `result` are the figures of that turn's last segment; the aborted segment of a steer has its own `result` (bill both). `modelUsage` is keyed by model id and also lists the small helper model Claude Code uses for titles. Each `assistant` frame has `message.usage` for the one API call. `modelUsage[model].contextWindow` gives the context size.
- **[t3]** Rate limit frame as recorded: `{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","rateLimitType":"five_hour","resetsAt":1779846000,"isUsingOverage":false,"overageStatus":"rejected","overageDisabledReason":"org_level_disabled"},"uuid":"…","session_id":"…"}` (some recordings carry only `{"status":"allowed"}`). The CLI throttles unchanged frames. `status:"rejected"` is the limit signal; how a hit limit shows on a `result` was not recorded.

## 4. The control protocol

Requests the host sends (all **[bin]** schema; **[live]** where marked). Each answers with `control_response` `success` (payload below) or `error`.

| `request.subtype` | fields | success payload |
| --- | --- | --- |
| `initialize` | see section 2 | session description **[live]** |
| `interrupt` | optional `cancel_queued:true`, `reason` | `{"still_queued":[uuid…]}` (plus `cancelled:[uuid…]` with `cancel_queued`) **[live]**: idle gives `{"still_queued":[]}` |
| `set_permission_mode` | `mode` | `{"mode":"acceptEdits"}` **[live]**, and a `system/status` frame `{"status":null,"permissionMode":"acceptEdits"}` follows **[live]**. `bypassPermissions` is refused with an `error` (`Cannot set permission mode to bypassPermissions because the session was not launched with --dangerously-skip-permissions`) unless the process was launched in that mode or with `--allow-dangerously-skip-permissions` **[bin]**: a thread that goes to full access after a process launched in another mode needs a new process (`--resume`), and the other way round is a control request |
| `set_model` | `model` (alias, id, or `null` for default) | `{"model":"claude-sonnet-5-5"}` the resolved id **[live]** |
| `set_max_thinking_tokens` | `max_thinking_tokens`, `thinking_display` | |
| `mcp_status` | | `{"mcpServers":[{"name","status","config","transport","scope","source",…}]}` **[live]** (`[]` with no servers) |
| `mcp_set_servers`, `mcp_reconnect`, `mcp_toggle`, `mcp_read_resource` | server names or configs | `{added,removed,errors}` for `mcp_set_servers` |
| `stop_task` | `task_id` | stops one background task or subagent |
| `cancel_async_message` | `message_uuid` | `{cancelled:boolean}` |
| `rewind_files` | `user_message_id` | restores files to that message |
| `get_context_usage`, `get_usage`, `get_session_cost`, `list_models`, `get_settings`, `apply_flag_settings`, `rename_session` | | informational or settings |

Requests the CLI sends to the host (the host must answer each with the same `request_id`, or the turn waits):

| `request.subtype` | what | answer |
| --- | --- | --- |
| `can_use_tool` | a tool call needs permission | `PermissionResult`, below |
| `request_user_dialog` | a structured dialog (`dialog_kind`, `payload`) | only if declared in `supportedDialogKinds`; the executor declares none, so the CLI falls back |
| `hook_callback`, `mcp_message` | only if the host registered SDK hooks or in-process MCP servers | not used |
| `elicitation` | an MCP server asks for input | shape not examined; answer with an `error` response until the phone handles it |

Answer an unknown request with an `error` response, never silence.

### Permission request and answer (no MCP approval tool)

The CLI writes, and the turn blocks until it is answered. Shape from the `can_use_tool` schema **[bin]** (snake_case on the wire); the field values below are from the recorded SDK-level request **[t3]** of the same call:

```json
← {"type":"control_request","request_id":"b3a9…","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"/work/.claude-denied-write.txt","content":"approval fixture"},"tool_use_id":"toolu_016StH6rgvnkDxU2MWt46gYE","display_name":"Write","description":".claude-denied-write.txt","permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}]}}
```

Schema, all **[bin]**: required `tool_name`, `input` (the tool's input object), `tool_use_id`; optional `permission_suggestions` (`PermissionUpdate[]`), `blocked_path`, `decision_reason` (human text, can carry ANSI escapes: strip), `decision_reason_type`, `title`, `display_name`, `description`, `agent_id` (set when a subagent asked), `requires_user_interaction`, `tool_kind`, `suppress_always_allow_rule`, `default_to_no`, `classifier_approvable`, `matched_ask_rule`. A tool name starting `mcp__<server>__` is an MCP tool; `mcp_server` names the server.

Answer allow **[bin]** (`updatedInput` optional by schema; **[t3]** always sends it, echoing the input, edited if the user edited):

```json
→ {"type":"control_response","response":{"subtype":"success","request_id":"b3a9…","response":{"behavior":"allow","updatedInput":{"file_path":"/work/.claude-denied-write.txt","content":"approval fixture"},"toolUseID":"toolu_016StH6rgvnkDxU2MWt46gYE","decisionClassification":"user_temporary"}}}
```

Answer allow for the rest of the session **[bin][t3]**: add `"decisionClassification":"user_permanent"` and `"updatedPermissions":[…]`, taking the request's `permission_suggestions` with `destination:"session"`, or `[{"type":"addRules","rules":[{"toolName":"Write"}],"behavior":"allow","destination":"session"}]` when it had none. `PermissionUpdate` variants: `addRules | replaceRules | removeRules {rules:[{toolName,ruleContent?}], behavior: allow|deny|ask, destination}`, `setMode {mode, destination}`, `addDirectories | removeDirectories {directories, destination}`; `destination` is `userSettings | projectSettings | localSettings | session | cliArg` (use `session`).

Answer deny **[bin]**, recorded as the SDK result in `tool_call_denied_write` **[t3]**:

```json
→ {"type":"control_response","response":{"subtype":"success","request_id":"b3a9…","response":{"behavior":"deny","message":"User declined tool execution.","toolUseID":"toolu_016StH6rgvnkDxU2MWt46gYE","decisionClassification":"user_reject"}}}
```

- The deny `message` goes to the model as the `tool_result` (`is_error:true`, "User declined tool execution."): the model sees why and can reply (the recording continues with "write permission denied"). Put the user's feedback text in `message` when there is one.
- `"interrupt":true` on a deny also aborts the turn **[bin]**; **[t3]** sends it for the phone's "cancel" decision, not for "decline".
- `decisionClassification` is `user_temporary | user_permanent | user_reject`, for the CLI's telemetry only.
- **[bin]** If the turn is interrupted while a request waits, the CLI sends `{"type":"control_cancel_request","request_id":"b3a9…"}`: close the pending approval on the phone, and send nothing for it. **[orca]** treats a cancel as an abort of the callback for the same reason.
- **[bin]** Do not hold approvals across a process death: a request that waits is lost with the process. After a restart the question is asked again by the model, not by the CLI.
- The `result.permission_denials[]` array lists the denied tool calls of the turn **[t3]**.

### Ask-user questions (AskUserQuestion) and plan approval

`AskUserQuestion` is an ordinary tool: it arrives as a `can_use_tool` request with `tool_name:"AskUserQuestion"` and is answered by an allow whose `updatedInput` carries the answers. There is no separate request type.

```json
← {"type":"control_request","request_id":"c41d…","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","tool_use_id":"toolu_01…","input":{"questions":[{"question":"Which library should we use for date formatting?","header":"Library","multiSelect":false,"options":[{"label":"date-fns","description":"Small, tree-shakeable"},{"label":"dayjs","description":"Moment-like API"}]}]},"requires_user_interaction":true}}
→ {"type":"control_response","response":{"subtype":"success","request_id":"c41d…","response":{"behavior":"allow","updatedInput":{"questions":[…the same array…],"answers":{"Which library should we use for date formatting?":"date-fns"}},"toolUseID":"toolu_01…"}}}
```

- **[sdk][bin]** `input`: `questions` has 1 to 4 items `{question, header (max 12 chars), options (2 to 4 of {label, description}), multiSelect}`; an "Other" free-text choice is offered by the UI and not listed in `options`. `updatedInput` is the input plus `answers: {<question text>: <answer string>}`; a multi-select answer is the labels joined with `", "`; free text is the typed string. Optional `response` (free text typed instead of choosing) and `annotations` (`{<question>: {unanswered: "skipped" | "replied_in_chat" | "dismissed"}}`, only when `system/init.capabilities` has `ask_user_question_unanswered_v1`). **[t3]** keys answers by the question text, and denies with "User cancelled tool execution." when the question is withdrawn.
- **[t3]** `ExitPlanMode` (plan mode) is also a `can_use_tool`: the plan markdown is in `input.plan` **[t3]**; allow = approve the plan, deny with a message = send feedback (the mapping of allow and deny to plan approval is T3's, not shown in a recording here).
- **[bin]** `requires_user_interaction:true` means: show the tool's own card, not a one-tap approve/deny.

### Interrupt

```json
→ {"type":"control_request","request_id":"9","request":{"subtype":"interrupt"}}
← {"type":"control_response","response":{"subtype":"success","request_id":"9","response":{"still_queued":[]}}}
← {"type":"result","subtype":"error_during_execution","is_error":true,"terminal_reason":"aborted_tools", …}
```

- **[bin]** The process keeps running; the conversation is intact and ready for the next user message. The interrupted turn ends with a `result` that has `is_error:true` and `terminal_reason` `aborted_streaming` (while the model was writing) or `aborted_tools` (while tools ran); on a clean interrupt the receipt is written before that `result`. A synthetic `user` frame `[Request interrupted by user]` (`isSynthetic`) lands in the transcript **[t3]** (seen in `claude_background_wake_before_queued_prompt`).
- **[bin]** `still_queued` lists the uuids of messages that survive the interrupt and will still run; with `cancel_queued:true` they are cancelled instead and listed under `cancelled` (each gets a `command_lifecycle` `cancelled`). The phone's stop button should send `cancel_queued:true` when it means "stop everything", and plain `interrupt` when queued messages are meant to run next.
- **[bin]** A plain interrupt ends running tools; a background task survives only if the host declared `perTaskStopAffordance`, otherwise the interrupt kills it. Stop one task with `stop_task`.
- **[t3]** T3's recordings (`turn_interrupt`, `turn_interrupt_restart`) end with the T3 query closed right after `query.interrupt`, so there is no recorded `result` after an SDK interrupt; the `aborted_*` result is established by the steer recording and the schema, not by an interrupt recording.
- **[orca]** A tool held by a plugin may never acknowledge the interrupt; then the supervisor closes the child. The executor needs a timeout after which it kills and restarts the process (resume, below).

## 5. Interrupt semantics and the process

- The process is never restarted to interrupt or steer. A steer is a `priority:"now"` user line; a stop is `interrupt`.
- Kill the process only when: the thread is closed or archived; an interrupt is not acknowledged within the timeout; the process crashed. Close politely first by closing stdin (exit 0 when idle **[live]**), then SIGTERM.
- **[t3]** Child exit with no `result` for the open turn is a crash of that turn: fail it with the last 8 KiB of stderr, keep the thread, and restart with `--resume` on the next message.

## 6. Resume after the process died

```
claude -p --input-format stream-json --output-format stream-json --verbose \
  --include-partial-messages --replay-user-messages --permission-prompt-tool stdio \
  --resume <session_id> --permission-mode … --model …
```

- **[t3]** Recorded (`turn_interrupt_restart`): a process interrupted while a Bash tool ran was closed; a new one with `--resume fb591f8f-…` (and no `--session-id`) answered the next prompt, `system/init` carried the same `session_id`, result `success`. The dangling tool call of the first process did not break the resume.
- **[live]** An id with no transcript: exit 1, `result` `error_during_execution`, `errors:["No conversation found with session ID: <id>"]`. Treat it as "start fresh with `--session-id`" only when the executor knows the thread has no turns yet; otherwise fail the thread loudly (the transcript is gone).
- **[bin]** Transcripts are kept by the CLI under `CLAUDE_CONFIG_DIR`; `--no-session-persistence` disables that. The CLI creates and reads them: the executor stores only the session id per thread (it is the thread's native id).
- **[bin]** After resume, pending state is not restored: queued messages, waiting permission requests and background tasks of the dead process are gone. History for the phone comes from the executor's own event log, not from replaying the transcript.

## 7. One conversation per process: confirmed

- **[t3]** One process, one conversation: every recorded multi-turn run keeps one `session_id` on all frames across turns on one process (`multi_turn`: two turns, two `init` frames, one id). `--session-id` and `--resume` each take exactly one id **[live]**; the combination needs `--fork-session` **[live]**.
- **[bin]** There is no thread-id parameter in any request or frame: nothing multiplexes conversations on one stdin. The sub-agents of a turn run inside the process and are tagged with `parent_tool_use_id`, they are not separate conversations.
- **[bin]** Exception to handle: the session id can change inside a process after `/clear` (a `conversation_reset` frame; `rename_session` is documented as "when this process has since moved to another session (/clear, …)"). The adapter keys the process by the executor's thread id and updates the stored native session id from `session_id` of each `system/init`.
- So the executor keeps `HashMap<thread, ClaudeProcess>`; a Codex-style "one process per workspace" is not possible for Claude.

## Minimal turn, end to end

```
spawn: claude -p --input-format stream-json --output-format stream-json --verbose
       --include-partial-messages --replay-user-messages --permission-prompt-tool stdio
       --session-id <uuid>|--resume <uuid> --permission-mode … --model …     (one per thread)
→ control_request initialize            ← control_response success           once per process
→ user {message, uuid, parent_tool_use_id:null}
← command_lifecycle queued/started      ← user{isReplay}  ← system init
← stream_event … / assistant … / user(tool_result) …
← control_request can_use_tool          → control_response allow|deny         (answer by request_id)
→ user {…, priority:"now"}              ← result{aborted_*} ← init ← … (steer)
→ control_request interrupt             ← control_response{still_queued} ← result{aborted_*}
← rate_limit_event
← result {subtype, is_error, terminal_reason, user_message_uuids, usage}
← command_lifecycle completed
```

## Not verified

- No model turn, no login and no credential were used. Every `assistant`, `stream_event`, `user` (tool result), `rate_limit_event`, `result` (success) and `can_use_tool` shape comes from the schemas embedded in 2.1.330 or from T3 recordings made with Claude Code 2.1.111 to 2.1.281; nothing here is a live 2.1.330 turn. Verified live against 2.1.330 with control frames only: framing, `--verbose` and `--replay-user-messages` requirements, `initialize`, `interrupt` when idle (`still_queued`), `set_permission_mode` (reply and `system/status`), `set_model`, `mcp_status` (with an inline `--mcp-config`), the unknown-subtype error, `--permission-prompt-tool stdio` accepted, `--session-id` validation, the `--session-id`+`--resume` refusal, the unknown-`--resume` result frame and exit 1, stdin EOF exit 0.
- The Agent SDK is not on disk. The SDK's exact spawn argv is known only from Orca's test assertion (SDK 0.3.x); the wire form of `permission.request` (snake_case fields, `request_id`) is derived from the binary's schema and the SDK-level T3 recordings, not from a captured raw `can_use_tool` line. The raw wire of an allow/deny answer is derived the same way.
- Whether `can_use_tool` frames are sent without `--permission-prompt-tool stdio` in stream-json mode (the help says the default `--permission-prompts host` means "the SDK host or --permission-prompt-tool"; an unhandled prompt is "denied automatically" when nobody can be asked). The adapter always passes the flag.
- Whether a user message written before `initialize` works; whether `initialize` is required at all for a host that only needs the defaults.
- The behaviour of a plain (no `priority`) user message arriving mid-turn beyond "queued FIFO, started after the running turn's result" (recorded). The binary mentions a running turn absorbing queued messages mid-turn (`queued_command` attachments, "mid-turn absorption"); whether the model sees a plain mid-turn message at the next tool boundary, or only in the next turn, was not shown in a recording.
- No recording of an SDK `interrupt` followed by a live `result` (T3 closes the query); no recording of `cancel_queued`, `control_cancel_request` or `stop_task`; their shapes are from the schema.
- The exact error text and effect of `--session-id` for an id that already has a transcript (only the string `Session ID … is already in use.` was found, not run).
- Whether `--resume` finds a transcript when the working directory differs from the original one (the CLI keys transcripts by project directory under `CLAUDE_CONFIG_DIR`; not tested). The executor must resume in the same `cwd` and `CLAUDE_CONFIG_DIR` until shown otherwise.
- How a process behaves after a very long idle, under memory pressure, or with many parallel processes on the A1 VM (footprint per process was not measured).
- `request_user_dialog`, `hook_callback`, `elicitation` flows, `rewind_files`, `reload_plugins`, `mcp_set_servers` effects: types read, no flow run.
- Token accounting across a resume and across a steer: T3 bills the aborted segment separately; the figures here come from its code and a single recording, not from a measured turn.
