---
module: remote-chat-agent
date: 2026-08-13
problem_type: integration_issue
component: service_object
severity: critical
symptoms:
  - "Every chat turn returns a well-formed SSE stream with a session frame, a done frame, and zero chunk frames"
  - "The agent looks healthy: /health is 200, a session is created, a model resolves"
  - "opencode's own log shows prompt_async failed … NotFound: FileSystem.realPath (<dir>) (ENOENT)"
  - "No session.error event is ever emitted, so the stream ends in a normal terminal idle"
root_cause: config_error
resolution_type: config_change
related_components:
  - opencode-telegram-bot
  - docker
tags:
  - opencode
  - container-boundary
  - silent-failure
  - sse
  - docker-compose
  - working-directory
---

# A sibling container cannot resolve the working directory you send it

## Problem

`remote-chat-agent` sends `directory: <cwd>` with every `session.create` and
`session.promptAsync` call. `opencode serve` runs in a *different container* and
resolves that path against **its own** filesystem. Sharing a network namespace
(`network_mode: service:tailscale`) shares no files, so a `cwd` perfectly real
in the agent container does not exist in the opencode container, and every turn
produced an empty answer.

## Symptoms

The stream is structurally valid, which is what makes this expensive to diagnose:

```
data: {"type":"session","sessionId":"ses_006f97f1bffezO8ogRtlS0hvo6"}
data: {"type":"model_used","id":"gpt-5.6-luna","providerID":"opencode-go"}
data: {"type":"done"}
```

A session is created. A model resolves. `done` arrives. There is no error
anywhere in the response, in the agent's logs, or in the container's stderr.

The cause is only visible inside opencode's own log file:

```
prompt_async failed sessionID=ses_… cause="Cause([Die(PlatformError: NotFound:
  FileSystem.realPath (/tmp/oc-cwd-bs6yAH)
  (cause: Error: ENOENT: no such file or directory, lstat '/tmp/oc-cwd-bs6yAH'))])"
```

opencode logs that to `~/.local/share/opencode/log/opencode.log` and **never
publishes it as a `session.error` event**, so the SSE consumer sees a healthy
idle. `session.get` afterwards confirms `tokens.input = 0`, `tokens.output = 0`.

## What Didn't Work

- **Blaming the credential.** The first conclusion was that opencode needed an
  interactive `opencode auth login` and that a provider API key was insufficient.
  That was wrong, and a preflight message asserting it got committed before being
  disproved. `opencode auth list` reports the env key as
  `Environment / OpenCode Go [OPENCODE_API_KEY]`, and with the key present the
  full model catalogue resolves. No OAuth is involved.
- **Reading the agent's logs.** They showed `stream.idle.terminal ms:18397
  eventCount:13` — thirteen events and no content. Accurate, and useless: the
  agent had faithfully reported that opencode said nothing.
- **Persisting an auth volume.** A volume was mounted at `/root/.opencode`, a
  path opencode never writes to (the real path is `~/.local/share/opencode`).
  Harmless, and unrelated to the failure.

What actually found it was dumping every raw event from `opencode serve` for one
turn, seeing an assistant message created with zero parts, and then reading
opencode's own log file inside the container.

## Solution

Three changes — one makes it work, two make a recurrence loud.

**1. Give both containers the same path.** `docker-compose.prod.yml`:

```yaml
  opencode:
    volumes:
      # THE WORKING DIRECTORY MUST BE THE SAME PATH IN BOTH CONTAINERS.
      - worktrees:/app/.worktrees
  agent:
    environment:
      WORKTREE_BASE_DIR: /app/.worktrees
      # Default cwd for a turn that names no worktree. `process.cwd()` is /app,
      # which exists only in the AGENT container.
      OPENCODE_CWD: /app/.worktrees
    volumes:
      - worktrees:/app/.worktrees
```

**2. Probe the directory before spending a turn on it.** A `probe`, called at
the top of `run_capturing` in `backend/remote-chat-agent/service-core/src/application/one_shot.rs`
(the one-shot turn the voice planner, the Tamil check, image descriptions and runs
are built on), which returns `Failure::DirectoryNotVisible` naming the path and
the reason.

The endpoint choice matters. `GET /find/file` is used because it fails through
the *same* `FileSystem.realPath` code path a prompt does — an unresolvable
directory answers 500 with the identical ENOENT. The alternatives all look
better and are worse:

| Endpoint | Behaviour on a directory no turn can use |
|---|---|
| `/path` | 200, echoes the unknown directory back |
| `/file/status` | 200, empty array |
| `session.create` | succeeds, returns a session |
| `/api/fs/list` | 500 — but *also* 500s for directories that work fine, because it is scoped to the server's boot location (`Path escapes the location`) |
| `/find/file` | 500 only when `realPath` fails — matches prompt behaviour |

**3. Refuse to call a contentless turn a success.** A turn that reaches its
terminal `session.idle` having emitted no text, no reasoning, no tool activity
and no question now fails with `Failure::Empty` instead of passing as an answer.

## Why This Works

The failure had two independent halves, and fixing either alone leaves a trap:

- The *cause* was a path that existed on one side of a container boundary. The
  shared volume removes it.
- The *invisibility* was opencode declining to publish an internal prompt
  failure as a stream event. Nothing in the wire protocol distinguished "the
  model had nothing to say" from "the prompt was never processed". The probe
  converts the common case into a named error before any work is done, and the
  contentless-turn check catches every other cause of the same silence.

The probe is deliberately not a general health check. It asks exactly the
question that matters — *can you resolve this path?* — through the code path
that will otherwise fail.

## Prevention

**Treat "the other process's filesystem" as a distinct namespace.** Sharing a
network namespace reads like sharing an environment; it shares no files. Any
parameter naming a path that crosses a process boundary needs the same path on
both sides, asserted somewhere.

**When a protocol can express "nothing happened", never emit it silently.** The
concrete rule that came out of this: a terminal state reached with zero output is
an error until proven otherwise.

```rust
if accumulator.emitted_chars() == 0 && !other_content {
    diag("stream.empty.response", &[("sessionId", json!(session_id)), ("cwd", json!(cwd)), ("eventCount", json!(events))]);
    return Err(Failure::Empty { session_id, cwd: cwd.clone(), events });
}
```

**Pick a preflight that shares the failure path.** A probe that passes where the
real operation fails is worse than no probe — it launders the bug. Prefer the
endpoint whose stack trace matches the one you are trying to prevent, and record
why the obvious alternatives were rejected (see the table above) so nobody
"simplifies" it back.

**Read the third-party's own log before theorising.** opencode writes to
`~/.local/share/opencode/log/opencode.log`; the answer was sitting there from the
first failed turn. A dependency that swallows an error from its API usually still
writes it somewhere.

**Regression coverage** — `backend/remote-chat-agent/rs/tests/flow/voice.rs`
pins both behaviours through the voice planner: a `/find/file` probe answering
500 fails the turn naming the directory before anything is prompted, and an
assistant message followed by idle with no parts fails with "opencode finished
the turn without emitting any content" (`Failure::Empty`).
