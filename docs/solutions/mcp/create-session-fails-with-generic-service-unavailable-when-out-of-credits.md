---
title: "create_session fails repeatedly with a generic \"service is temporarily unavailable\" error when the account is out of usage credits"
date: 2026-08-22
category: mcp
tags: [claude-code-remote, mcp, remote-control, create_session, usage-overage, credits, policy-limits]
problem_type: knowledge
module: claude-code-remote
last_updated: 2026-08-22
---

## Context

A session tried to spawn a fresh Claude Code Remote session (`create_session`,
targeting a specific environment) to hand off the second half of a two-step
plan. The call failed identically 9 times across several minutes,
including two explicit backoff waits (8s, 16s), with no variation:

```
failed to create session: the service is temporarily unavailable — try again
```

This is a **different error signature** from the two prior documented
Claude Code Remote spawning issues in this repo (both are `-32003` MCP
approval errors, or a spawn that succeeds but then deadlocks) — see
"Related" below. This one is a plain create-call failure with a generic
message, no MCP error code, no approval dialog.

## Guidance

**Rule out a real transient outage first, but don't stop there.** `list_sessions`
(and other read-only Claude Code Remote calls) succeeded cleanly between
every failed `create_session` attempt, and `list_sessions` for the target
environment showed no stale or conflicting session — exactly one active
session (the caller itself), the rest archived. That rules out both "the
whole MCP connection is down" and "a concurrency/quota conflict on this
environment" as explanations. It does **not** by itself explain why
`create_session` specifically keeps failing while everything else works.

**Check `~/.claude.json`'s `cachedExtraUsageDisabledReason` next.** In this
case it read:

```json
"cachedExtraUsageDisabledReason": "out_of_credits"
```

This is a **more specific value** than the `"org_level_disabled"` seen in
`docs/archive/README.md`
— that doc's finding was an org policy toggle (`allow_remote_control.allowed:
false`, later shown to be an unreliable predictor on its own); `out_of_credits`
is a literal billing/budget exhaustion signal. It directly explains the
observed asymmetry: `create_session` provisions a new billable container
(real compute spend), while `list_sessions`/`get_session`/other MCP tool
calls in the *current* session are metered against the running session,
not a new allocation. A budget gate would plausibly reject the
former and never touch the latter — matching the exact behavior observed.

`~/.claude/policy-limits.json`'s `allow_remote_control.allowed: false` was
also present, as in the org-policy doc, but per that doc's update this
value alone is not a reliable predictor (a prior session succeeded despite
it). Treat it as corroborating context, not the primary signal —
`cachedExtraUsageDisabledReason` is the more specific and actionable one.

**What NOT to waste time on:** retrying the identical call more than a
handful of times with backoff (a real outage would likely clear within that
window; a credits gate will not, however long you wait); trying a
distinctly-named/titled session (title has no bearing on a billing gate);
assuming a stale/conflicting session in the target environment without
checking `list_sessions` first (cheap, and rules the theory out
or in immediately).

## Why this matters

A generic "temporarily unavailable" message gives no hint that the real
cause is a billing/credits state rather than infrastructure flakiness. An
agent that only retries will burn turns and, per this repo's verification
discipline, risks reporting a vague "external outage" status to the
user when the actual, actionable fact (out of credits) was one file read
away. Diagnosing the *specific* reason turns "wait and see" into "here is the
literal actionable fact, and here is what unblocks it" — the difference
between a deferral and a demonstrated, evidenced blocker.

## When to apply

- `create_session` (or any Claude Code Remote call that provisions a new
  session/container) fails with a plain, non-MCP-coded message like
  `the service is temporarily unavailable — try again`, repeatedly, with no
  variation across retries and backoff.
- Read-only Claude Code Remote calls (`list_sessions`, `get_session`) succeed
  cleanly in the same window — ruling out a blanket connection/outage.
- `list_sessions` scoped to the target environment shows no concurrency
  conflict (no other active/RUNNING session competing for a slot).
- You want to distinguish "infrastructure is down, retry later" from "the
  account is out of budget, retrying will never help" before reporting status.

## Examples

**The failure, verbatim, repeated identically:**

```
create_session(...) → failed to create session: the service is temporarily unavailable — try again
```
(9 consecutive attempts, including 8s and 16s backoff waits, zero variation.)

**The diagnostic that ruled out concurrency:**

```
list_sessions(mine: true) → exactly one RUNNING session in the target
environment (the caller itself); every other session in that environment is
ARCHIVED.
```

**The actual signal, found in `~/.claude.json`:**

```json
"cachedExtraUsageDisabledReason": "out_of_credits"
```

**Practical resolution path:**

1. Confirm the diagnosis by reading `~/.claude.json`'s
   `cachedExtraUsageDisabledReason` (and, for corroboration only,
   `~/.claude/policy-limits.json`'s `allow_remote_control.allowed`) rather
   than continuing to retry the identical call.
2. If it reads `out_of_credits` (or similar), this is a billing/account-admin
   escalation — top up usage/credits or wait for the account's usage window
   to reset — not fixable inside the session or by editing repo
   files.
3. For the immediate task, report the specific blocker (not a vague "service
   is down") and hand the exact spawn parameters/prompt back to the user so
   they can either wait for credits to reset or spawn the session themselves
   through the normal UI, billed the same way but the human's own
   call to make.

**Related:**
- `docs/archive/README.md`
  — a different, `-32003`-coded failure mode with an org-policy (not
  billing) root cause; its "Update" section is the source of the "policy-
  limits.json alone is not a reliable predictor" caveat reused here.
- `docs/archive/README.md`
  — a third, unrelated CCR limitation: `create_session` *succeeds* there but
  the spawned child later deadlocks. Together, the three docs cover "spawn
  blocked by policy," "spawn blocked by credits," and "spawn succeeds but
  can't be steered" — three distinct failure shapes on the same tool
  surface.
