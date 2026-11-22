---
title: opencode server protocol, as the rca executor must speak it
date: 2026-11-22
category: architecture
module: backend/remote-chat-agent
problem_type: architecture_pattern
severity: medium
component: development_workflow
tags: [architecture, rca, rca-exec, opencode, http, sse]
applies_when:
  - "writing or changing the opencode adapter that hosts an opencode server process at the executor"
  - "mapping opencode SSE events to agent.threads events"
  - "answering an opencode permission or question request"
related_components: [tooling]
---

# opencode server protocol

One `opencode serve` process hosts many sessions in many directories. The executor starts one per workspace and CLI, keeps it for the life of the workspace, and carries each phone message to it (HTTP) and each event back (SSE). This note is what the adapter codes against. It replaced the one-process-per-turn runner (`opencode run`), which is gone.

Sources, tagged on every fact below:

- **[bin]** the installed `opencode 1.19.5` (`/root/.local/bin/opencode`), exercised live: `opencode serve` on a loopback port with throwaway `HOME` and `XDG_*` dirs under `/dev/shm`, driven with curl. No provider credential, no model turn. The one thing that runs a command without a model is `POST /session/{id}/shell`, used to see a real tool part.
- **[spec]** the OpenAPI 3.1 document the same server serves at `GET /doc` (480 KB, 472 schemas, saved only under `/dev/shm`, not committed; refetch it with `curl $URL/doc`). Shapes marked [spec] were read there but not exercised.
- **[t3]** `references/t3code/packages/provider-opencode/src/server/{OpenCodeRuntime,OpenCodeServerOwner,adapter}.ts` (the 1.x adapter, minimum version 1.14.19, drives the routes in this note through `@opencode-ai/sdk/v2`) and `.../v2/{OpenCode2Client,OpenCode2Server}.ts` (the 2.x adapter, drives `/api/*`).
- **[rec]** the recorded wire transcripts `references/t3code/apps/server/src/orchestration-v2/testkit/fixtures/*/opencode_transcript.ndjson`: `simple`, `plan_questions`, `turn_interrupt` recorded against opencode 1.14.39 (real turns, SDK request and SSE boundary); `opencode2_*` recorded against 2.0.18. Some frames in `opencode_child_approval` carry hand-written ids (`evt_childperm0001`): treat them as shape examples only.

## How the executor speaks it

The service's driver (`OpencodeSessions`, `backend/remote-chat-agent/cli-driver`) and the executor's bridge (`clis/rca-exec/src/adapters/opencode_bridge.rs`) split the work so that the credential never leaves the machine the CLI runs on:

- `session_open` with program `opencode` starts `opencode serve --hostname 127.0.0.1 --port 0` in the workspace, with a password the executor drew from `/dev/urandom` (`OPENCODE_SERVER_USERNAME=rca`, `OPENCODE_SERVER_PASSWORD`) and a parent-death signal. Nothing the server prints goes to the service; the executor reads the `listening on` line, opens `GET /global/event` itself and says `{"ready":true}` once the stream answers.
- `session_send` takes one line, `{"id","method","path","body"}`, for a route of `exec_proc::SESSION_HTTP_ROUTES` (`/session`, `/permission`, `/question`, `/global/health`, `/config` read-only, `/agent`), makes the request with the credential and answers on the session's output with `{"id","status","body"}`. Every frame of the event stream goes out as `{"event": <frame>}` (the heartbeat dropped), the end of the stream as `{"closed": why}`. Every text that leaves is scrubbed of the password.
- The driver keeps one server per workspace and one session per thread (`POST /session` with the permission rules of the thread's runtime mode, or `GET /session/{id}` for the id the thread stored; the server's store keeps sessions across a restart, so a resume is a lookup). A turn is `prompt_async` with `agent` (`plan` for the plan mode, else `build`) and the model when it names a provider (`provider/model`); it ends at the session's idle after the prompt's own user message was seen. The directory is named only when the thread's directory is absolute: the server runs in its workspace's directory, which is every other request's default.
- A steer is another `prompt_async` on the session; the turn stays open until an idle that comes after the server stored every prompt sent. An interrupt is `POST /session/{id}/abort`; `MessageAbortedError` is read as no error, and a server that does not end the turn within 15 seconds is taken down. A permission request is an approval event answered with `POST /permission/{id}/reply` (`once`, `always`, `reject`); a question is an input event answered with `POST /question/{id}/reply` or `/reject`; a request that a subagent's session asks is routed to the thread of its root through `parentID`.
- A server whose event stream ends is taken down (the stream is volatile on v1); its turns end with an error and the threads are resumed by their session ids on a new server. A server idle for `RCA_OPENCODE_IDLE_SECS` (600 by default, 0 for ever) is closed.
- Not done: the service's tool server (`POST /mcp` is per directory, not per thread), the model gateway (`docs/gaps/opencode-session-not-through-gateway.md`), usage events (`ExecEvent` has none for any CLI), aborting a subagent's session with its parent.

## Two APIs in one binary

1.19.5 serves two route families on one port, both backed by the same session store (a session made through `/api/session` shows in `GET /session`) **[bin]**:

| family | routes | state in 1.19.5 |
| --- | --- | --- |
| legacy (v1) | `/session…`, `/event`, `/global/event`, `/permission…`, `/question…`, `/mcp`, `/config` | what T3's 1.x adapter drives and what the 1.14.39 recordings show. Use this. |
| `/api/*` (v2) | `/api/session…`, `/api/event`, `/api/permission/request` … | present, early: its event names (`session.next.*`, `permission.v2.asked`) differ from the 2.0.18 recordings (`session.inbox.*`, `session.execution.*`, `form.created`), and `GET /experimental/capabilities` answers `{"backgroundSubagents":false}`. T3 uses it only for opencode 2.x. |

**[spec][rec]** The v2 family is where opencode models a prompt `delivery` of `"steer"` or `"queue"` and a durable per-session event log (`GET /api/session/{id}/event?after=`). It is worth a second adapter once opencode 2.x is the installed version; for 1.19.x every section below is the v1 family unless a heading says v2.

## Start, port, auth

```
opencode serve --hostname 127.0.0.1 --port 0
→ stdout: opencode server listening on http://127.0.0.1:4096
```

- **[bin]** `serve` flags: `--port` (default `0`), `--hostname` (default `127.0.0.1`), `--mdns`, `--mdns-domain`, `--cors <origin>…`, `--log-level DEBUG|INFO|WARN|ERROR`, `--print-logs` (stderr), `--pure` (no external plugins). Every other knob is an env var.
- **[bin]** Readiness is the single stdout line `opencode server listening on http://HOST:PORT`. **[t3]** T3 parses it with `/server listening on\s+(https?:\/\/[^\s]+)/i`, picks a free port itself rather than trusting `--port 0`, and waits up to 30 s. Here `--port 0` printed `4096`, so read the port from the line, never assume it. Without a password the process prints `Warning: OPENCODE_SERVER_PASSWORD is not set; server is unsecured.` first.
- **[bin]** Auth is HTTP Basic. `OPENCODE_SERVER_PASSWORD` turns it on, `OPENCODE_SERVER_USERNAME` sets the user (default `opencode`). Verified: no header gives `401` with `www-authenticate: Basic realm="Secure Area"` and an empty body; a wrong password gives `401`; with `OPENCODE_SERVER_USERNAME=rca` the user `opencode` gives `401` and `rca` gives `200`.
- **[bin]** Browser-style clients can pass the credential as a query parameter: `?auth_token=<base64 of user:password>` returned `200` on `/global/health`. The executor does not need it; use the header.
- **[t3]** Encode the credential as UTF-8 before base64 (`Buffer.from("opencode:"+pw,"utf8")`); Latin-1 helpers mis-encode non-ASCII passwords. T3 also clears an inherited `OPENCODE_SERVER_PASSWORD` for its 2.x server and sets `OPENCODE_PASSWORD`; the 1.19.5 binary reads `OPENCODE_SERVER_PASSWORD` **[bin]**.
- **[t3]** Spawn detached in its own process group, kill the group (`SIGTERM`, then `SIGKILL` after 1 s) on shutdown, keep the last 64 KiB of stdout and stderr for crash messages, treat child exit as the only crash signal. T3 also keeps a pid ledger so an orphan server from a crashed parent is reaped on restart.
- **[bin]** Env that matters: `OPENCODE_CONFIG_CONTENT` (inline JSON config, see Config), `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, `OPENCODE_PERMISSION`, `OPENCODE_DISABLE_AUTOUPDATE`, `OPENCODE_DISABLE_MODELS_FETCH`, `OPENCODE_DISABLE_PROJECT_CONFIG`, `OPENCODE_DB`, and `HOME` plus `XDG_{CONFIG,DATA,CACHE,STATE}_HOME` for where state lives (sessions are in a SQLite database under the data dir). Names found with `strings` on the binary; only `OPENCODE_SERVER_*` and `OPENCODE_CONFIG_CONTENT` were exercised.

### Health and version

```json
GET /global/health   → {"healthy":true,"version":"1.19.5"}
GET /api/health      → {"healthy":true}
```

**[t3]** T3 refuses a server older than 1.14.19 using `global/health`. Use the same call as the "is it up" probe.

## Directory handling: one server, many working directories

- **[bin]** Every route takes `?directory=<abs path>`; the SDK sends the same value as the `x-opencode-directory` header **[t3]**. Both were verified on `POST /session`: the new session's `directory` is the given one. With neither, the server uses the directory it was started in (its cwd).
- **[bin]** A session is created in, and belongs to, one directory (`session.directory`). Later calls on a session id work whatever `directory` they carry (`GET /session/{id}` for a session of another directory succeeded), but state that is per-directory instance does follow `directory`: `/mcp`, `/config`, `GET /session`, `GET /permission`, `GET /session/status`, and the `/event` stream. Always pass the session's own directory.
- **[bin]** `GET /session` without `directory` returned the sessions of every directory of the same project; with `?directory=X` it returned only X's. Projects are keyed by the git root commit, and a repo with no commit is project `global`, so two scratch repos shared a project. Filter on the `directory` field client-side as well.
- **[t3]** T3 creates one SDK client per thread bound to its `cwd` and adds `directory` to every call; the server is shared. The T3 adapter notes the directory-bound client "is not safe for cross-thread pooling": one client per directory, not one per process.

## Sessions

All **[bin]** unless tagged.

| purpose | request | response |
| --- | --- | --- |
| create | `POST /session?directory=` body `{title?, parentID?, agent?, model?:{id,providerID,variant?}, permission?:Rule[], metadata?}` | `200` Session |
| get (resume by id) | `GET /session/{id}` | `200` Session; unknown id `404 {"name":"NotFoundError","data":{"message":"Session not found: ses_x"}}` |
| list | `GET /session?directory=&roots=&search=&limit=&start=` ([spec]; `scope=project` is the only `scope`) | `200` Session[] |
| list (paged) | `GET /experimental/session?cursor=&limit=&archived=` | `200` Session[] each with `project:{id,worktree}` |
| update | `PATCH /session/{id}` body `{title?, permission?, …}` | `200` Session (`title` change verified; T3 sends `{permission}`) |
| delete | `DELETE /session/{id}` | `200 true` |
| messages | `GET /session/{id}/message?limit=&before=` | `200` `[{info:Message, parts:Part[]}]`, newest first |
| one message | `GET /session/{id}/message/{messageID}` ([spec]) | |
| children | `GET /session/{id}/children` | `200` Session[] (subagent sessions; `[]` here) |
| status of all | `GET /session/status?directory=` | `200` `{ "<sessionID>": {"type":"busy"} }`, `{}` when all are idle |
| todo | `GET /session/{id}/todo` | `200` Todo[] |
| fork | `POST /session/{id}/fork` body `{messageID?}` | `200` Session with `parentID` unset and title `"<title> (fork #1)"` |
| revert | `POST /session/{id}/revert` body `{messageID, partID?}` | `200` Session with `revert:{messageID,snapshot,diff}`; on a busy session `409 SessionBusyError` |
| unrevert | `POST /session/{id}/unrevert` | `200` Session, `revert` absent |
| summarize (compact) | `POST /session/{id}/summarize` ([spec]; T3 calls it) | |
| shell | `POST /session/{id}/shell` body `{agent, command, model?}` | `200` `{info, parts}` after the command ends |

```json
→ POST /session?directory=/work/repo   {"title":"t1"}
← {"id":"ses_dfeb8abbeffeJPKDP5inHSUikS","slug":"happy-eagle","projectID":"global","directory":"/work/repo","path":"","cost":0,"tokens":{"input":0,"output":0,"reasoning":0,"cache":{"read":0,"write":0}},"title":"t1","version":"1.19.5","time":{"created":1795317781569,"updated":1795317781569}}
```

- **[bin]** Session ids are `ses_` + opaque; message ids `msg_`, part ids `prt_`, event ids `evt_`, permission ids `per_`, question ids `que_`. `session.agent` and `session.model` are filled by the first prompt (`agent:"build"`, `model:{id,providerID}`), or by create.
- **[bin]** `GET …/message` pages with an `X-Next-Cursor` header and a `Link: <…&before=CURSOR>; rel="next"` header (exposed through `access-control-expose-headers`). The cursor is base64 JSON `{id,time}`. Pass it back as `before`. Absent header = last page.
- **[bin]** **Resume needs no call.** The sessions persist in the SQLite store: after killing and restarting the server, `GET /session/{id}` and `GET /session/{id}/message` returned the same session and its 5 messages, and a new prompt on that id works with no "load" step. The executor's "resume by id" is `GET /session/{id}` (to check it exists and read `directory`) and, for history, `GET …/message`. **[t3]** T3's resume is exactly `session.get` plus `session.messages`.
- **[bin]** Compare with Codex: there is no per-process thread subscription to join; any client with the credential can drive any session, so the executor must serialise its own turns per session (see Prompts).
- **[bin]** Sessions made without a model are legal; they get `agent`/`model` from the first prompt.

### The permission ruleset on a session

`permission` is a `PermissionRuleset = [{permission:string, pattern:string, action:"allow"|"deny"|"ask"}]`, last match wins **[spec][t3]**. It was accepted and echoed by `POST /session` **[bin]**. **[t3]** T3 builds it from its approval mode:

- full access: `[{permission:"*",pattern:"*",action:"allow"}]`.
- otherwise: deny everything, then `ask` (approval-required) or leave denied (non-interactive) for `bash, edit, webfetch, websearch, codesearch, external_directory, doom_loop`, then allow `question, read, glob, grep, lsp, todowrite, task, skill`; `read` of `*.env` and `*.env.*` is `ask` (or `deny`), `*.env.example` allowed; `edit` allowed for auto-accept-edits.
- **[t3]** A child (subagent) session copies its parent's rules only at creation, so T3 `PATCH`es the children's `permission` too when the policy changes.

## Prompts

### prompt_async

```json
→ POST /session/{id}/prompt_async?directory=/work/repo
  {"messageID":"msg_edc93addd001jzzrE6lotn7BJj","model":{"providerID":"openai","modelID":"gpt-5.4-mini"},"agent":"build","variant":"high","system":"…","parts":[{"type":"text","text":"fix the bug"}]}
← 204 No Content (empty body)
```

- **[bin][spec]** Body: `parts` (required) of `{type:"text",text}` | `{type:"file",mime,url,filename?}` | `{type:"agent",name}` | `{type:"subtask",…}`; optional `messageID` (`^msg`), `model:{providerID,modelID}` (both required together), `agent`, `variant`, `system`, `tools:{name:boolean}`, `format` (structured output), `noReply`. The response is `204` as soon as the message is accepted, not when the turn ends; events carry the rest. Unknown session `404` **[spec]**; a bad body is `400 {"name":"BadRequest","data":{"message":"…","kind":"Payload"}}` **[bin]** (seen on a permission reply with a wrong enum value; same envelope).
- **[bin]** `noReply:true` stores the user message and runs **no** model: the server emitted `session.updated`, `message.updated` (user), `message.part.updated` (text), `session.updated`, `session.status {idle}`, `session.idle` and `GET …/message` listed the message. This is how to test the plumbing without a model.
- **[bin]** `POST /session/{id}/message` is the same body but answers `200 {info:AssistantMessage, parts:Part[]}` only when generation ends. **[t3]** T3 uses `prompt_async` for turns and `POST …/command` (`{command, arguments, messageID, model?, agent?, variant?, parts}`) for `/name` slash commands, which also blocks until generation ends, so a command call is cancelled by the caller, not awaited.
- **[t3]** T3 mints the user `messageID` itself (`msg_` + 12 hex of `epochMs*0x1000+counter` + 28 base62 random chars) so it can recognise its own user message in the stream. The ids sort by time; opencode compares them to decide whether a user message is newer than the last assistant step, so a supplied id must be larger than every earlier id in the session. Whether 1.19.5 honours a supplied id was not checked (T3's comment: "Versions may ignore supplied message IDs"); correlate by `message.updated` role `user` and `parentID` of the assistant message when in doubt.
- **[t3]** T3 repeats `agent`, `variant` and `system` on every prompt, steers included, because "OpenCode runs each step with the agent and system prompt of the latest user message".
- **[bin]** There is no turn object. A turn is the span from the user message to `session.status {idle}` / `session.idle` for that session **[t3]** ("OpenCode has no turn object; correlate by initiating user message and settle on session idle").

### A prompt while a turn is running

- **[bin]** `POST …/shell` and `POST …/revert` on a busy session answer `409 {"_tag":"SessionBusyError","sessionID":"ses_…","message":"Session is busy: ses_…"}`. A `noReply` prompt on a busy session answered `204` and the message was stored behind the running shell.
- **[t3]** For a real model turn T3 does not queue behind opencode and does not rely on `409`: it sends a **second `prompt_async` with a fresh `messageID`** to the same session while the turn is active, calls that a steer (`supportsActiveSteering: true`), and keeps the turn open until the next idle. Admission is tracked by three signals: the `204`, the user `message.updated`, and a `session.status busy`; an `idle` seen before both counts as "reconcile by reading `GET /session/status`". T3 keeps its own queue for turns that are not steers.
- **[rec]** The v2 family models this explicitly: `POST /api/session/{id}/prompt {prompt:{text}, delivery:"steer"|"queue", resume?}` answers `{data:{id,sessionID,prompt,delivery,…}}`; a steer sent during a running shell call was delivered at the next step boundary (`session.inbox.enqueued` then `session.inbox.delivered` when the step ended) in 2.0.18.
- **Not observed live:** how 1.19.5 orders a second user message that arrives mid-step with a real model (see Not verified). Design for it as "the next step of the same turn sees the new message", with the Codex `turn/steer` mapping as an `agent.queue` steer, and with the executor, not opencode, owning the queue for non-steer sends.

### Abort

```json
→ POST /session/{id}/abort        ← 200 true
```

- **[bin]** On an idle session it returns `true` and emits `session.status {idle}` then `session.idle`. On a running `bash` tool (shell) it returned in 19 ms, the tool part finished `completed` with output `"start\n\n\n<metadata>\nUser aborted the command\n</metadata>"`, and the session went `idle` (no `session.error` was emitted in that shell case).
- **[rec]** `turn_interrupt`: the `session.status idle` event arrived **before** the abort HTTP response; do not wait for the response to settle the turn.
- **[t3]** Abort only stops the one session: a subagent child keeps running, so T3 walks `GET /session/{id}/children` recursively and aborts each (concurrency 8, tolerant of `404`). A model turn that is aborted ends with `session.error` carrying `MessageAbortedError` (`{name:"MessageAbortedError",data:{message}}`) **[spec]**; T3 treats that as `interrupted`, not `failed`.
- **[rec]** v2: `POST /api/session/{id}/interrupt` answers `{"interrupted":true}` in 2.0.18, then `session.tool.failed {error:{type:"aborted"}}`, `session.step.failed`, `session.execution.interrupted {reason:"user"}`.

## Event stream

Two SSE endpoints, both `text/event-stream`, one JSON object per `data:` line, no `event:` field **[bin]**:

| endpoint | scope | frame |
| --- | --- | --- |
| `GET /event?directory=` | the directory instance of the subscription (the server start dir if none) | `{"id":"evt_…","type":"…","properties":{…}}` |
| `GET /global/event` | every directory | `{"directory":"/work/repo","project":"global","payload":{"id":"evt_…","type":"…","properties":{…}}}` |

- **[bin]** Verified isolation: with a `/event` subscription on the start dir, a session created in another directory produced no event on it, while `/global/event` carried both, tagged with `directory`. **One `/global/event` subscription per server is the way to serve many directories**; route by `payload.properties.sessionID`. T3 subscribes `/event` once per directory-bound client instead.
- **[bin]** First frame is `server.connected` (`properties:{}`). A `server.heartbeat` follows every ~10 s on `/event` and `/global/event` (`properties:{}`); `/api/event` sends a `: heartbeat` SSE comment instead. **[t3]** T3 treats 45 s of silence as a dead stream (2.x) and, on a reconnect (`server.connected` seen twice), marks token usage incomplete because events in the gap are lost: **the stream is volatile, there is no replay on v1**. Subscribe before sending a prompt, and after a reconnect re-read `GET /session/status` and `GET …/message` to reconcile.
- **[bin]** `/global/event` also emits `{"directory":"global","project":"global","payload":{"type":"project.updated",…}}` and `{"payload":{"type":"sync","syncEvent":{"id","type":"session.created.1","seq":0,"aggregateID":"ses_…","data":{…}}}}` (a durable-log mirror of each change). Ignore both.
- **[t3]** Undecodable or unknown event types are skipped, not fatal; the 1.19.5 `Event` union in [spec] has 88 members (ide, pty, tui, lsp, workspace, vcs noise included). Ignore unknown types.

### Event table (legacy family)

`properties` fields. **[bin]** = seen live on 1.19.5; **[rec]** = in a 1.14.39 recording; **[spec]** = schema only.

| `type` | properties | meaning | seen |
| --- | --- | --- | --- |
| `server.connected` | `{}` | stream open | bin |
| `server.heartbeat` | `{}` | keepalive | bin |
| `session.created`, `session.updated`, `session.deleted` | `{sessionID, info:Session}` | session row changed (title, agent, model, tokens, cost, `revert`) | bin |
| `session.status` | `{sessionID, status}`, status `{"type":"busy"}` \| `{"type":"idle"}` \| `{"type":"retry","attempt","message","next","action?"}` | turn running or finished; `retry` is a provider retry | bin (busy, idle), spec (retry) |
| `session.idle` | `{sessionID}` | turn finished (fires right after `status idle`) | bin |
| `session.error` | `{sessionID?, error?}`, error one of `ProviderAuthError`, `UnknownError`, `MessageOutputLengthError`, `MessageAbortedError`, `StructuredOutputError`, `ContextOverflowError`, `ContentFilterError`, `APIError{message,statusCode?,isRetryable,responseBody?}` | turn failed or was aborted; `sessionID` absent = process-wide | spec, t3 |
| `message.updated` | `{sessionID, info:Message}` | a message row changed. User: `{id,role:"user",time:{created},agent,model:{providerID,modelID,variant?},system?,tools?}`. Assistant: `{id,role:"assistant",parentID,modelID,providerID,mode,agent,path:{cwd,root},cost,tokens:{total?,input,output,reasoning,cache:{read,write}},time:{created,completed?},finish?,error?}`; `time.completed` set = this assistant message is done | bin (user, assistant), rec |
| `message.removed` | `{sessionID, messageID}` | after revert cleanup | spec |
| `message.part.updated` | `{sessionID, part:Part, time}` | a part was created or changed; carries the **full** part each time | bin, rec |
| `message.part.delta` | `{sessionID, messageID, partID, field, delta}` | text delta: append `delta` to `part[field]` (`field:"text"`) | spec, t3 |
| `message.part.removed` | `{sessionID, messageID, partID}` | | spec |
| `permission.asked` | `{id, sessionID, permission, patterns:string[], metadata, always:string[], tool?:{messageID,callID}}` | a tool needs approval; turn blocks | spec, t3 |
| `permission.replied` | `{sessionID, requestID, reply:"once"\|"always"\|"reject"}` | request answered (by anyone) | spec, t3 |
| `question.asked` | `{id, sessionID, questions:[{question,header,options:[{label,description}],multiple?,custom?}], tool?:{messageID,callID}}` | the `question` tool is waiting for the user | rec |
| `question.replied` | `{sessionID, requestID, answers:string[][]}` | | rec |
| `question.rejected` | `{sessionID, requestID}` | | spec, t3 |
| `todo.updated` | `{sessionID, todos:[…]}` | plan/todo list | spec, t3 |
| `session.diff` | `{sessionID, diff:[SnapshotFileDiff]}` | files changed so far | spec |
| `session.compacted` | `{sessionID}` | compaction finished | spec, t3 |
| `command.executed` | `{name, sessionID, arguments, messageID}` | slash command ran | spec |
| `file.edited` | `{file}` | | spec |
| `mcp.tools.changed` | `{server}` | an MCP server's tool list changed | spec |

### Parts (`part.type`) **[spec]** with live shapes

All have `{id, sessionID, messageID}`.

- `text {text, synthetic?, ignored?, time?:{start,end?}, metadata?}`: the assistant text, or user text. The shell run showed a synthetic user text `"The following tool was executed by the user"` **[bin]**.
- `reasoning {text, time:{start,end?}, metadata?}`.
- `tool {callID, tool, state, metadata?}` with `state`:
  - `{status:"pending", input, raw}` (arguments still streaming)
  - `{status:"running", input, title?, metadata?, time:{start}}`
  - `{status:"completed", input, output, title, metadata, time:{start,end}, attachments?}`
  - `{status:"error", input, error, metadata?, time:{start,end}}`
- `step-start {snapshot?}`, `step-finish {reason, cost, tokens:{total?,input,output,reasoning,cache:{read,write}}, snapshot?}`: one per model step; **[t3]** token usage is the sum of `step-finish` tokens (and the assistant `message.updated.tokens`).
- also `file`, `subtask`, `agent`, `snapshot`, `patch`, `retry`, `compaction`.

### Text, tool and turn lifecycle

Order of a plain text turn **[rec]** (`simple`, 1.14.39, real): user `message.updated` → `message.part.updated` (user text) → `session.status busy` → `message.updated` (assistant, no `completed`) → `step-start` part → `reasoning` part (`time.start`) → `reasoning` part again (`time.end`) → `text` part with `text:""` → `text` part with the full text and `time.end` → `step-finish` part → `message.updated` (assistant with `finish:"stop"`) → `message.updated` (assistant with `time.completed`) → `session.status busy` → `session.status idle`. In that real recording the text arrived as one final `message.part.updated` and no `message.part.delta`; with a streaming model, deltas are emitted **[spec][t3]** and T3 appends `delta` to the stored part text and re-emits on `part.updated`. So: key text by `part.id`, treat `part.updated` as authoritative (full text), apply `part.delta` between updates, and finalise on the assistant message's `time.completed`.

Real tool transitions **[bin]** (a `bash` part from `POST …/shell`, ids shortened):

```json
{"type":"message.part.updated","properties":{"sessionID":"ses_…","part":{"type":"tool","id":"prt_…","messageID":"msg_…","tool":"bash","callID":"01M80M…","state":{"status":"running","time":{"start":1795317909969},"input":{"command":"echo hi-from-shell; sleep 1; echo done"}}},"time":1795317909969}}
{"type":"message.part.updated","properties":{…"state":{"status":"running","time":{"start":1795317909969},"input":{…},"metadata":{"output":"hi-from-shell\n"}}…}}
{"type":"message.part.updated","properties":{…"state":{"status":"completed","time":{"start":1795317909969,"end":1795317910989},"input":{…},"title":"","metadata":{"output":"hi-from-shell\ndone\n"},"output":"hi-from-shell\ndone\n"}…}}
```

Streaming command output arrives as `running` updates whose `state.metadata.output` grows; the final `completed` carries `output`. The `question` tool in a real model turn goes `pending` → `running` → `completed` **[rec]**. The shell path skipped `pending`.

A model turn that calls a tool spans several steps: `plan_questions` shows a second assistant `message.updated` after the question tool completed, and the last assistant message has `finish:"stop"`. The `finish` value of a tool-calling step was not captured; the turn ends only at `session.status idle` **[rec]**.

### Token usage and cost

- **[rec][spec]** Per step on the `step-finish` part and cumulatively per assistant message: `tokens:{total,input,output,reasoning,cache:{read,write}}` and `cost` (USD number). The session row aggregates `tokens` and `cost` (`session.updated`). Example recorded: `{"total":10837,"input":571,"output":9,"reasoning":17,"cache":{"write":0,"read":10240}}`.
- **[t3]** T3 sums `step-finish` per user-prompt message; an event gap (stream reconnect) makes it flag usage incomplete, and it re-reads `GET …/message` to fill it.

## Permission requests

1. **[spec][t3]** The turn blocks and the stream carries

```json
{"type":"permission.asked","properties":{"id":"per_…","sessionID":"ses_…","permission":"bash","patterns":["git status"],"metadata":{},"always":["bash"],"tool":{"messageID":"msg_…","callID":"call_…"}}}
```

`permission` is the rule key (`bash`, `edit`, `webfetch`, `external_directory`, `doom_loop`, `read` of an env file, …); `patterns` are what is being asked about (the command, a path glob); `always` is what `always` would save. Correlate to the tool part by `tool.callID`.

2. Reply **[bin]** (request shape and error replies exercised; the full ask-reply-resume loop needs a model turn):

```json
→ POST /permission/{requestID}/reply?directory=/work/repo   {"reply":"once","message":"optional text for reject"}
← 200 true
```

- `reply` is `"once"` | `"always"` | `"reject"`; any other value `400 {"name":"BadRequest","data":{"message":"Expected \"once\" | \"always\" | \"reject\", got \"yes\"\n  at [\"reply\"]","kind":"Payload"}}`. **[t3]** T3 maps accept to `once`, accept-for-session and accept-always to `always`, decline and cancel to `reject`.
- Unknown or already answered id: `404 {"_tag":"PermissionNotFoundError","requestID":"per_nope","message":"Permission request not found: per_nope"}` (a second reply to the same id gives this).
- Deprecated alias **[bin]**: `POST /session/{sessionID}/permissions/{permissionID}` body `{"response":"once"|"always"|"reject"}`, same `200 true` / `404`.
- `GET /permission?directory=` lists pending requests, `[]` when none **[bin]**; use it after an SSE reconnect to recover requests whose `permission.asked` was missed.
- After the reply the stream emits `permission.replied {sessionID, requestID, reply}`; `always` also stores the rule for the rest of the server's life **[spec]**. **[t3]** T3 ignores an `asked` for an id it already saw settled.
- What `reject` does to the turn (the tool fails and the model continues, or the turn stops) was not observed; see Not verified.

**v2 family [bin]**: `POST /api/session/{id}/permission {action, resources[], save?[], metadata?, source?}` creates a request and **blocks until it is answered**, returning `{"data":{"id":"per_…","effect":"ask"}}` (immediately with `effect:"allow"` when a rule allows it). The stream shows `permission.v2.asked {id, sessionID, action, resources, save, metadata, source?}` on `/api/event` only (not `permission.asked` on `/event`); `GET /api/session/{id}/permission` lists them. Reply `POST /api/session/{id}/permission/{requestID}/reply {"reply":"once"|"always"|"reject","message"?}` → `204`, then `permission.v2.replied {sessionID, requestID, reply}`. In 2.0.18 the same frames are named `permission.asked`/`replied` with `data:{id,sessionID,action:"shell",resources,save,source:{type:"tool",messageID,id}}` and the reply input is `decision` **[rec]**: do not assume the v2 names across versions.

## Questions (the `question` tool)

- **[rec]** `question.asked` fires when the model calls the `question` tool (the tool part is `pending` then `running`); the turn blocks until answered.

```json
{"type":"question.asked","properties":{"id":"que_edc9624d9001e2GFKX724Z8b5Z","sessionID":"ses_…","questions":[{"question":"Should this fixture prefer strict schemas or UI flexibility?","header":"Schema vs Flexibility","options":[{"label":"Strict schemas","description":"Prioritize validation and exact structure over visual freedom."},{"label":"UI flexibility","description":"…"}],"multiple":false}],"tool":{"messageID":"msg_…","callID":"call_…"}}}
→ POST /question/que_edc9624d9001e2GFKX724Z8b5Z/reply   {"answers":[["Strict schemas"]]}
← 200 true
{"type":"question.replied","properties":{"sessionID":"ses_…","requestID":"que_…","answers":[["Strict schemas"]]}}
```

- `answers` is an array in question order, each an array of the chosen option labels (or free text when `custom` is not `false`); an unanswered question is `[]` **[rec][t3]**. The tool then completes with `output:"User answered: Strict schemas"` and `metadata.answers`.
- `POST /question/{requestID}/reject` (no body) cancels; stream `question.rejected`. Unknown id on either: `404 {"_tag":"QuestionNotFoundError","requestID":"que_nope","message":"Question request not found: que_nope"}` **[bin]**. `GET /question` lists pending **[spec]**.

## Many sessions in one server process

- **[bin]** Sessions are rows, not processes; sessions of two directories coexisted in one server and `GET /session/status` keys by session id (`{"ses_…":{"type":"busy"}}`). Every event carries `sessionID`; route on it. The `409 SessionBusyError` is raised per session id (only the busy session's calls were refused); two sessions busy at the same time was not run.
- **[t3]** T3 runs many threads on one server per instance and one `/event` subscription per directory-bound client; it flags that one directory-bound client must not be shared by threads of other directories.
- **[t3]** Subagents (the `task` tool) are child sessions with `parentID`; their events arrive on the same stream with the child's `sessionID`, and their permission requests are asked on the **child** session id. Resolve the child to its root via `GET /session/{id}` `parentID` before surfacing to the phone.
- **[bin]** A crash or restart of the server loses the SSE connection and any in-flight turn; the session and its stored messages survive. **[t3]** T3 fails active turns as `transport_error` on exit and marks the thread reusable.

## Config injection: MCP, model, provider

- **[bin]** `OPENCODE_CONFIG_CONTENT='{"permission":{"bash":"ask"},"mcp":{"probe":{"type":"local","command":["/x"],"enabled":true}}}'` at spawn was merged into `GET /config` and `GET /mcp` for every directory (`{"probe":{"status":"failed","error":"ENOENT: …posix_spawn '/nonexistent/mcp'"}}`). **[t3]** T3 always sets it, to `{}` by default, because with `extendEnv:false` an unset value would not be inherited. This is the process-wide route and fits server-level rca MCP wiring.
- **[bin]** Runtime MCP: `POST /mcp?directory=D` body `{"name":"rca","config":{"type":"remote","url":"http://127.0.0.1:9/mcp","headers":{"Authorization":"Bearer x"},"oauth":false}}` or `{"type":"local","command":["…"],"environment":{"K":"v"},"cwd"?, "enabled"?, "timeout"?}` → `200` map of every MCP server of that directory to its status (`connected` | `disabled` | `failed {error}` | `needs_auth` | `needs_client_registration`). **Scoped to `directory`**: a server added with `?directory=work2` did not appear for the start directory and vice versa; it is lost on server restart (`GET /mcp` after restart was `{}`). Add per directory, re-add after a restart, and check the returned status for the name. `POST /mcp/{name}/connect`, `/disconnect`, `DELETE`, and the `/auth` routes also exist **[spec]**. **[t3]** T3 adds a remote server named `t3-code` with a bearer header per provider session, and its tools appear to the model prefixed by the server name.
- **[bin]** Model, per request, no config: `model:{providerID,modelID}` and `variant` on the prompt, `agent` for build/plan (`GET /agent` lists `build`, `plan` (primary), `general`, `explore` (subagent), and internal `compaction`, `summary`, `title`), and `system` for extra instructions. `model:{id,providerID,variant?}` and `agent` also on `POST /session`. Choosing a model needs it to exist in `GET /config/providers` (`{providers:[{id,name,source,env,options,models:{<id>:{…}}}], default}`) **[bin]**; credentials come from env vars named in each provider's `env` list, `PUT /auth/{providerID}`, or the config `provider` block. `POST /session` and a `noReply` prompt accepted a model id that is not in the catalog (`anthropic/claude-x` was echoed back), so validate the id against `GET /config/providers` in the adapter; what a real turn does with an unknown model was not run.
- **[bin]** Do not use `PATCH /config` for per-thread settings: it applied to the one directory and **wrote `config.json` into that working directory** (a stray file in the user's repo), and the change was not visible to `GET /config` until the instance reloaded. Use prompt-level `model`/`agent`/`system`, session-level `permission`, `OPENCODE_CONFIG_CONTENT` at spawn, and `POST /mcp`.
- **[spec]** `PATCH /global/config`, `POST /global/dispose`, `POST /instance/dispose` exist; do not call them from the adapter.

## Process and failure behaviour

- **[t3]** Child exit is the crash signal; an SSE stream that ends while the child is alive is a transport failure (fail active turns, mark the session errored, do not resume that stream: start a new one and reconcile).
- **[bin]** The server is quiet on stdout after the listening line; logs go to stderr only with `--print-logs`.
- **[bin]** Startup took about 6 s to the listening line in this sandbox, and the first request for a new directory boots that directory's services (file watcher, project copy refresh), logged as `booting location services`. Expect extra latency on the first call per directory.
- **[t3]** Heartbeat gap is the liveness check (45 s on 2.x; the v1 heartbeat is 10 s).

## Minimal turn, end to end

```
spawn: opencode serve --hostname 127.0.0.1 --port <free>   env: OPENCODE_SERVER_PASSWORD, OPENCODE_CONFIG_CONTENT, HOME/XDG
wait for stdout "opencode server listening on <url>"        GET /global/health → {healthy, version}
GET /global/event                                           once per server; route by payload.properties.sessionID
POST /mcp?directory=D {name, config}                        once per directory (and again after a server restart)
POST /session?directory=D {title, permission[]}   ← Session  once per conversation
  or GET /session/{id}?directory=D                           resume, then GET …/message for history
POST /session/{id}/prompt_async?directory=D {messageID, model, agent, variant, system, parts}  ← 204
← message.updated(user) · session.status busy · message.part.updated … · message.part.delta …
← permission.asked → POST /permission/{id}/reply {reply}     ← 200 true   · permission.replied
← question.asked   → POST /question/{id}/reply {answers}     ← 200 true   · question.replied
POST /session/{id}/prompt_async … (second message)           steer: next step of the same turn
POST /session/{id}/abort ← 200 true  (+ abort children)      then session.status idle
← message.updated(assistant, time.completed, tokens, cost) · session.status idle · session.idle   ⇒ turn done
```

Mapping to agent.threads: `message.part.updated` of `text` → assistant text (delta by `part.id`); `reasoning` → thinking; `tool` parts → tool items by `callID` (status `pending|running` → in progress, `completed`, `error`); `permission.asked` → an agent.requests approval (reply `once`/`always`/`reject`); `question.asked` → an ask_user request; `step-finish` and assistant `message.updated` → usage; `session.status idle` → turn done (`interrupted` when the executor sent the abort, `failed` when a `session.error` with a non-abort error preceded it).

## Not verified

- **No model turn was run** (task rule). Not exercised live on 1.19.5: any real assistant text stream (`message.part.delta` frames), the `pending` tool state, `permission.asked` and `question.asked` as emitted by a model tool call, `session.error` payloads, `session.status {retry}`, token and cost values, step boundaries, subagent (`task`) child sessions. Their shapes are from the 1.19.5 OpenAPI [spec], T3's handlers [t3] and the 1.14.39 recordings [rec]; the recordings predate 1.19.5 by five minor versions. A local scripted fake provider (a loopback OpenAI-compatible HTTP stub named in the `provider` config block) would close most of this without a credential; not done because the task said no model turn.
- How a second `prompt_async` behaves while a real model step is streaming on 1.19.5: queued behind, merged at the next step, or run as a second concurrent loop. Only T3's reliance on "steer works" and the 2.0.18 v2 recording back it. Also not checked: whether the server honours a client-supplied `messageID` and whether a lower-sorting id is rejected.
- What `reject` does to the turn (message-less reject versus `message`), and whether `always` persists across a server restart (T3 and the schema only name the values).
- The ask/reply/resume loop end to end: only the request and error shapes of the reply endpoints were exercised, plus the v2 `/api/session/{id}/permission` create, ask, reply, `permission.v2.replied` loop with a synthetic request.
- Whether a permission request made on a child session is also listed by `GET /permission` of the root's directory, and the child-to-root resolution cost (T3 does `session.get` per request).
- `POST /session/{id}/command`, `/summarize`, `/init`, `/share` (shapes from [spec] and T3 only); `session.diff` and revert of real file edits (revert on a session with no file changes returned `snapshot` `4b825dc6…`, the empty tree, `diff:""`).
- Event loss and ordering across an SSE reconnect on v1 (there is no cursor; T3 reconciles by reading messages); `GET /session/{id}/event?after=` replay exists only in the v2 family and was not exercised.
- Concurrency limits with many busy sessions, memory per session, and idle server shutdown behaviour; `--mdns`, `--cors` were not run.
- The v2 `/api/*` family beyond session create, permission create/reply and the heartbeat: `POST /api/session/{id}/prompt` (including `resume:false`), `/wait`, `/interrupt`, `/question`, durable per-session events. 1.19.5's v2 event names differ from the 2.0.18 recordings; re-derive from the installed binary's `/doc` before writing a v2 adapter.
- `OPENCODE_SERVER_USERNAME`, `OPENCODE_PERMISSION`, `OPENCODE_DISABLE_*` were found with `strings`; only `OPENCODE_SERVER_USERNAME`, `OPENCODE_SERVER_PASSWORD`, `OPENCODE_CONFIG_CONTENT` were run.
- Windows, `opencode acp` (a separate ACP server) and `opencode attach` were not examined.
