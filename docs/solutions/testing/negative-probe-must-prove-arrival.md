---
title: A negative probe must prove the request arrived
date: 2026-09-02
category: testing
tags: [testing, security, probes]
problem_type: knowledge
module: docs
---

Moved verbatim out of the root `AGENTS.md` on 2026-09-02 (context diet, known-gaps #51). The root keeps a distilled version and points here.

## A negative probe must prove the request arrived

When testing that something is *rejected*, first prove the request reached the
code doing the rejecting. A refusal and a request that never arrived look
identical from the client, and the comfortable reading is the wrong one.

Worked example, which hid a real hole. Probing a path-traversal guard with
`DELETE /api/worktree/../../etc` returned `404 Cannot DELETE /etc` — read as
"the guard holds". It was not: curl normalized the path before sending, so the
handler never ran, and the 404 would have looked the same with no guard at all.
Re-probed URL-encoded (`%2e%2e%2f%2e%2e%2fetc`), the segment arrived intact and
the guard answered `400` with its own refusal message — a response only the
guard produces.

That distinction then exposed the bug: `..` alone passed the guard, because the
slug function permits dots so `slugify('..') === '..'`. It reached
`rm -rf <base>/..` — the parent directory — and was stopped only by GNU `rm`'s
own refusal to remove `..`.

The rule: pick a probe whose success and failure produce *different* responses,
and confirm the negative case returns something only your code can emit — a
specific error string, a distinctive status. A generic 404 from a framework's
router is not evidence about your logic.
