---
title: Testing a change to a deployed service: both layers, in this order
date: 2026-09-02
category: testing
tags: [testing, deployed-services, verification]
problem_type: knowledge
module: docs
---

Moved verbatim out of the root `AGENTS.md` on 2026-09-02 (context diet, known-gaps #51). The root keeps a distilled version and points here.

## Testing a change to a deployed service: BOTH layers, in this order

Applies to every deployable service, not one of them. A change is not verified
until it has been exercised **twice**, and the order matters: the first localises
a failure, the second is the only one that proves the product.

1. **Directly against the service's own interface** — call the endpoint, CLI, or
   function with the exact payload its real caller sends, bypassing everything
   upstream. A failure here is unambiguous because nothing else is in the path.
   Where a fix changes a value, test BOTH arms: send the old value and confirm it
   still fails, send the new one and confirm it passes. A green result alone does
   not show the fix was the cause.

2. **Through the real entry point, against the deployed service** — whatever a
   user actually touches: the chat command, the HTTP route behind the gateway,
   the CLI a human types. Assert on what the user would see. Only this catches
   the wiring between components that step 1 skips by design.

**Never stop after step 1.** It is faster and sharper, which is exactly what
makes it tempting to treat as sufficient. It is not: it omits every seam between
components, and those seams are where this repo's real defects have lived.

Worked example, both halves from one session: calling the agent's worktree API
directly returned `200` with the new `owner/repo` value and `400` with the old
one — a clean two-armed proof. The same operation through the bot still failed,
because the bot ran its setup step against a path that exists only inside the
agent's container. Step 1 could not see that, and step 2 was the whole product.

**Clean up whatever the run created.** Test artifacts on a host with a hard
storage cap are a leak, not a leftover — each worktree is a full checkout. Every
create path needs a delete path, and the cleanup belongs in the same command
that ran the test rather than in someone's memory.
