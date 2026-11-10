---
title: A setter that silently no-ops on a missing container hides the real failure
date: 2026-08-28
category: developer-experience
tags: [wf, multi-browser, registry-service, slots, daemon, diagnostics, silent-failure]
problem_type: bug
module: frontend/libraries/registry-service
---

## Context

`wf browser start` and the session verbs (then open and navigate, now `wf browser start-device` and `wf browser open-link`) failed with `Timed out waiting for browser daemon` on every
fresh container. CLAUDE.md documents that exact message as the wrong-worktree case, so it
actively pointed investigation away from the cause. The daemon started and stayed alive;
Chromium was present; the bundle and `dist/` were built; loopback was in `NO_PROXY`.
Everything checked out, and it still timed out.

## The bug

`frontend/libraries/registry-service/src/use-cases/slots.ts` — `setBrowserMcpService` began:

```ts
const slotData = data.slots[slot];
if (!slotData) return;          // silent no-op
slotData.browserMcp = record;
```

The daemon *did* call `setDaemon`. The write was discarded, `withLockMutate` still rewrote the
registry file byte-identical, and the client polled a permanently-`null` URL until its 15s
deadline.

The slot was missing because **only `wf ship mla-tamagui`, `wf android` and `wf android` ever call
`slotCreate`.** `multi-browser` never did. So `wf browser` carried an undocumented prerequisite —
some other lifecycle must have run on this branch first. On a long-lived dev box that is always
true, which is why it never showed up there.

## The fix

Auto-vivify the slot, mirroring `setService` in the same file, which had always created its
container. The asymmetry between the two setters *was* the defect.

`tests/unit/slots.test.ts` contained `it('given the slot does not exist, set/clear are no-ops
(do not throw)')` — **a test asserting the broken behavior**, which is why the bug survived. It
now asserts creation for `set`, keeps the no-op assertion for `clear` (a no-op clear on a missing
slot is genuinely correct), and adds a case proving an invalid slot id still throws rather than
creating a malformed slot.

## The generalisable lesson

Two things made a small bug expensive, and both are worth checking for elsewhere:

1. **A setter that silently discards its write when its container is absent.** Either create the
   container or throw — returning quietly means the caller believes it succeeded. Note the sibling
   `setService` in the same file already did the right thing, so the codebase contained both the
   bug and its own counterexample.
2. **A detached child spawned with its stdio discarded.** `ensureDaemon` used `stdio: 'ignore'`,
   so nothing the daemon did — including failing to register — reached anyone. The daemon now
   writes to a log file, and the timeout names the resolved slot, whether that slot exists, and
   the registry path. That one line would have made this a 30-second diagnosis instead of an
   afternoon.

A generic timeout is not a diagnosis. When a wait loop gives up, it should say what it was waiting
for and what it observed.
