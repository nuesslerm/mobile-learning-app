---
title: Visually verify anything that can be visually verified, and show the evidence
date: 2026-09-02
category: workflow
tags: [verification, screenshots, evidence]
problem_type: knowledge
module: docs
---

Moved verbatim out of the root `AGENTS.md` on 2026-09-02 (context diet, known-gaps #51). The root keeps a distilled version and points here.

## Visually verify anything that can be visually verified, and show the evidence

If a change has a visual, on-screen effect — a UI change, a kiosk/browser
flag, a rendered page, a simulator/device screen, a desktop or window state
— checking logs, process argv, or exit codes is not verification of it. A
process launching with the right flags is not the same as a human seeing the
right thing on screen, and this repo has already shipped a "verified" fix
(#198's kiosk mode) whose live screenshot showed the original bug completely
unfixed — full browser chrome, the desktop panel, and a crash-restore prompt —
despite the flags checking out perfectly. Argv/log checks are still worth
doing (they're cheap and narrow down failures fast), but they complement a
screenshot, never substitute for one.

**Take the screenshot at conditions that match the real one being verified**
(same viewport/device dimensions, same invocation path, same state) — a
screenshot of the wrong thing rendering correctly is exactly as misleading
as no screenshot at all. See
`docs/archive/runbooks/vnc-kiosk-mobile-viewport-check.md` for a worked example:
checking chromium's argv passed while the real bug (visible only at a
phone's aspect ratio) was still live.

**Always show the screenshot/recording to the user as the evidence**, not
just a claim that verification happened — `SendUserFile` (or inlining it)
so they see what you saw, at the end of the turn that claims the fix works.
