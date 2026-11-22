---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/cli-driver/src/domain/driver.rs"
  - "backend/remote-chat-agent/cli-driver/src/domain/translate.rs"
opened: 2026-11-22
last_verified: 2026-11-22
links: []
---
# Codex's thread id event and `exec resume` argument order are built from the CLI's documented shape, not run against it

## What is broken

`session_of` reads Codex's id from the `thread.started` line (`thread_id`), and `command_for` builds `codex exec --json --skip-git-repo-check --sandbox <mode> [-m <model>] resume <id> <prompt>`. No recorded Codex output and no installed `codex` were available when this was written, so the event shape and the flag order (options ahead of `resume`) are unverified. The group runner still passes a resume id only to Claude Code, and `command_for_remote` still refuses Codex, so no live turn exercises this yet.

## Why blocked

Needs a `codex` binary and a login (`wf secret login codex`, a person signs in).

## What unblocks it

Run `codex exec --json "hi"` once, record its first line into a fixture, and run `codex exec resume --help` to confirm that `--sandbox` and `-m` stay valid ahead of `resume`. Then let `Runner::prepare` pass the stored session to Codex too, only when the stored session's `cli` is Codex.
