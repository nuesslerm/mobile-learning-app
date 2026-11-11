---
title: "Claude Code Remote headless session spawning is unreliable for automated diagnostics — framing matters more than expected"
date: 2026-08-08
category: workflow
tags: [claude-code-remote, ccr, create_session, subagent, headless, prompt-injection, orchestration]
problem_type: knowledge
module: workflow
---

## Context

Dispatching a Claude Code Remote (CCR) session programmatically (`create_session`, no human typing into it) behaves differently from a normal interactive session in ways that aren't obvious until they cost a session:

- `create_session`'s `permission_mode` cannot exceed the calling session's own mode — a caller running in a stricter mode can't spawn a more permissive child.
- A headless, API-spawned session is treated with genuine suspicion by its own safety layer: imperative "don't ask clarifying questions, just execute this" framing gets flagged as a prompt-injection attempt; ambiguous or terse prompts get met with "let me know what you'd actually like help with" instead of action; sessions can independently detect and report "automated event detected; awaiting user confirmation" purely from being API-spawned, with no injected instruction driving that response.
- Only a short `post_turn_summary` string is retrievable from a completed CCR session via `get_session` — there is no full-transcript access. Anything not captured in that summary is lost once the session ends.

## Guidance

- **Use natural, conversational, first-person framing when dispatching a CCR session** ("Hey, could you check X for me?") rather than imperative "don't ask, just execute" framing. The latter reads as an attempt to suppress the agent's normal judgment and is more likely to be flagged or met with a clarifying-question deflection than acted on.
- **Write a complete, unambiguous prompt.** A terse or underspecified prompt to a headless session has no human available to clarify, so it either stalls waiting for confirmation or produces a generic "what would you like help with" response instead of doing the work.
- **Don't rely on `get_session` for anything beyond the final summary.** If the dispatching session needs intermediate detail, have the child session write it to a file the parent can read, or have the child's `post_turn_summary` itself carry the load-bearing details — there is no way to pull a full transcript back out afterward.
- **Check `permission_mode` compatibility before dispatching.** A child session inherits a ceiling from the caller's own mode; don't assume `bypassPermissions` or similar is available to a child spawned from a more restricted parent.

## Why this matters

These aren't bugs to route around once — they're standing properties of how CCR treats API-spawned sessions, and they burn time identically every time they're rediscovered from scratch: a session dispatched with brusque, imperative framing gets refused or deflected, looks like a broken integration, and gets re-dispatched with the same framing before anyone thinks to change the prompt's tone. Knowing this in advance turns a multi-attempt debugging loop into "phrase it like you're asking a colleague" on the first try.

## When to Apply

- Dispatching any CCR session programmatically for a diagnostic, verification, or fan-out task where no human will interact with it directly.
- Debugging why a headlessly-spawned session refused to act, asked a clarifying question instead of executing, or reported detecting a prompt-injection attempt.
- Designing a multi-agent fan-out flow (parent dispatches several CCR children, polls for results) — plan around the `post_turn_summary`-only retrieval limit from the start rather than discovering it after a child session has already ended.

## Related

- `mcp__Claude_Code_Remote__create_session`, `mcp__Claude_Code_Remote__get_session`, `mcp__Claude_Code_Remote__send_later` / `create_trigger` — the tool surface this applies to.
- `docs/archive/README.md` — sharpens the `get_session`-only limitation noted above: beyond having no full-transcript access, there is no tool at all to answer a pending `AskUserQuestion` or otherwise redirect an already-running child session. If a dispatched child deadlocks on stale state, the documented recovery is archive the child and redispatch a fresh one, not try to message it.