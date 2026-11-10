---
title: Sharing a docker volume across containers needs one uid, not a shared group
date: 2026-08-20
category: infra
tags: [docker, volumes, uid, permissions, agent-shell, remote-chat-agent, setgid, umask]
problem_type: design-decision
module: infra/oracle-cloud/agent-shell
---

## Context

`agent-shell` (SSH surface) and `remote-chat-agent` (Telegram surface) are
meant to share one workspace volume, so a worktree, a credential, or a context
file created through either surface is visible to the other. They run as
different identities today: `remote-chat-agent` as **root** (no `USER`
directive; `IS_SANDBOX=1` exists precisely because it runs as root),
`agent-shell`'s `agent` as **1001** (`node:22-bookworm-slim` already occupies
1000 with `node`), and `vnc-auth-capture`'s desktop user `abc` as **1000**.

The question was which of two fixes to use. It was settled empirically against
a real docker daemon rather than by reasoning, because reasoning gets the
answer half right.

## What was measured

Three identities against one named volume, `busybox:1.36`, default settings.

**Naive sharing does not merely degrade — it fails at boot.** Docker creates a
fresh volume's root as `drwxr-xr-x root:root`. So a non-root container cannot
create anything at the top level at all:

```
mkdir: can't create directory '/w/from-agent': Permission denied
```

`agent-shell` would fail making its own `/workspace/home` on a fresh volume.
Everything root then writes with the default umask 0022 is `rw-r--r-- root:root`,
so `agent` gets a **read-only** view: read OK, append denied, create-sibling
denied, and — the one that matters for a worktree — `rm -rf node_modules`
denied, which breaks any reinstall.

### Fix A — shared gid + setgid dir + `umask 002`: works, but fails silently

With the directory `chmod 2775` and group `2000`, and every writer using
`umask 002`, both 1001 and 1000 read and write fine. But the umask is the whole
load-bearing part, and nothing enforces it. Root writing in that same setgid
directory *without* `umask 002` produces:

```
-rw-r--r--  0 2000  g.txt          -> agent: WRITE_DENIED
drwxr-sr-x  0 2000  nm2/           -> agent: RM_OK
```

Note the split result: the `rm` succeeds (removal depends on the *parent*
directory's write bit, which is group-writable) while the write fails. So one
forgotten umask yields a partially-broken tree rather than a clean error —
the worst possible failure shape for something you only notice mid-task.
Enforcing it means `process.umask(0o002)` in the Node service, inherited by the
Agent SDK's subprocess, plus pnpm, plus every shell — enforceable, yet it fails
open and silently every time someone misses one.

### Fix B — one root init chowns the volume root once, everything runs as 1001

```
chown 1001:1001 /w && chmod 775 /w
```

Then, from separate containers both running `--user 1001:1001`: create a
worktree and `node_modules`, modify from the other container, `rm -rf` the
`node_modules`, write `~/.claude/.credentials.json` — all `ALL_OK`. Crucially
the **default umask 0022 becomes harmless**, because the same uid owns
everything; there is no discipline to forget.

`abc` (1000) still cannot write (`WRITE_DENIED`), and writes fine once it
is also 1001.

## Decision

**Fix B.** Three concrete steps:

1. `agent-shell`'s `entrypoint.sh` already runs as root before dropping to
   sshd — it chowns the volume root once at boot. Idempotent and cheap.
2. `remote-chat-agent`'s runtime image runs as 1001 instead of root. This also
   makes `IS_SANDBOX=1` unnecessary: that flag exists only because the Agent
   SDK refuses `permissionMode: 'bypassPermissions'` as root. Running non-root
   removes the reason for the escape hatch rather than adding risk.
3. `vnc-auth-capture` either takes uid 1001 too, or keeps using vm-ops-api's
   `POST /agent-shell/credentials` relay — which already writes with an
   explicit uid/gid and exists for exactly this mismatch.

Fix A is recorded as rejected, not unconsidered: it is the more obvious answer,
and it is the one that breaks quietly six weeks later.

## Reproducing

`docker run --user <uid>:<gid> -v <vol>:/w busybox:1.36 sh -c '...'` against a
freshly created volume. The two probe scripts are in this session's scratchpad;
the whole thing takes under a minute and is worth re-running before changing
any uid here.
