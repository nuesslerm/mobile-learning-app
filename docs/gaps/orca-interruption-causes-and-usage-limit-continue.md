---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-agent/src/**"
opened: 2026-11-20
last_verified: 2026-11-20
links:
  - "docs/plans/rca-parity/plan.md"
---
# Orca `agentSession.continueInterrupted` and `agentSession.cancelUsageLimitContinue` stay not offered

Issue #83, O3 (sessions cluster). Checked 20 Nov against the recorded host (`orca-v1.4.259-recorded`, `-crawl`) and the pinned source in `references/orca`.

## What the recordings hold

- `cancelUsageLimitContinue` with valid params: only the refusal `structured_agent_session_unsupported` (`clientCapabilityMissing`). With bad params: validation errors.
- `continueInterrupted` with valid-shaped params: only `invalid_argument` ("Invalid agent session id"). No recording has a session the host cut short.

No recording shows a success shape, so any success reply would be written from the source alone.

## Why building it would still be a guess

- The host continues only a turn it wrote an attributed stop note for (update, quit, crash) and rechecks those notes before it continues (`structured-agent-session-interrupted-continuation.ts`). rca's journal is folded from rca's own events; a cancel is a user stop with no cause, and `turn.retry` retries only a failed newest turn.
- The usage-limit continue is a timer the host keeps for a turn that hit a provider limit, with `resetsAt` from the CLI's own message. The CLIs' limit text is not pinned or recorded here, and the real-CLI jobs hold no limited account to record one.

## What would make it offered

A recording of a host turn stopped by an update and by a usage limit (needs a limited Claude or Codex login, owner-held), then in rca: a `cause` on `turn.finished`, a stored resume-at per thread, and a clock that sends the continue turn. Not built: nothing in the recordings fixes the shapes.
