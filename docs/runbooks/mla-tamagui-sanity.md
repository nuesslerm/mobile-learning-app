---
title: mla-tamagui sanity runbook
verifies: That app #2 (the Tamagui side of the side-by-side) works end to end -- problem list, editor with a live CodeMirror WebView, keyboard toolbar rails, overlays and the debug log sheet -- in both the browser and Expo Go.
tier: manual
maxAgeDays: 90
date: 2026-08-28
module: frontend/mla-tamagui
---

# mla-tamagui sanity runbook

App #2 — the Tamagui side of the side-by-side comparison. Acceptance for this
app is exactly two things: **it works in the browser**, and **it works in Expo
Go**. This runbook covers both, and is explicit about which half a machine can
assert and which half needs a human holding a phone.

Screenshots of every step below, on both an iPhone and a Pixel, are published
as a report — regenerate it with `pnpm --filter mla-tamagui run report`. That
report is browser evidence, not device evidence; see §2.

## 1. Browser — automated, and the regression net

```bash
pnpm --filter mla-tamagui run test:e2e
```

It runs by hand, never in CI, against a local export of the app: it exports the web bundle once (an existing
`dist/` is reused), starts the leetcode backend on :8789, serves the export on loopback and runs each
check of `frontend/mla-tamagui/e2e/run-all.mjs` through the `@lib/browser-client` SDK (`libraries/browser-client/`), which drives the
`wf browser` daemon (`clis/harness-browser/src/browser_daemon.rs`) in a real device frame with the mock keyboard. No Metro is
involved. CI's browser evidence is the `e2e/*.lua` tests on `wf test prepare --web`.

The suite and each `verify:<name>` script serve whatever `dist/` is on disk, so after a source change run
`wf test prepare --web` first (it rebuilds only when the tree's inputs changed). The checks bind fixed ports: the
coding-agent ones need :8787 free.

Not green today, none of it from the Metro retirement: `verify-regression-spine` fails while the tamil route files
differ from origin/main, `verify-sheet` fails "tapping the backdrop dismisses the sheet",
`verify-result-and-toast` finds the Run Local button covered by `EditorSyntax-Dedent`, and
`verify-color-scheme-picker` fails 2 of its 4 sub-checks (seen 22 Oct on a fresh `wf test prepare --web`). Expected output once they are fixed:

```
› design system renders themed
ok: "Design system" — button rgb(0, 145, 255), badge rgb(0, 145, 255) r9999px, group rgb(242, 242, 242) r9px
› debug FAB opens the live log sheet
ok: FAB moved the sheet from top 1059 to 145 (viewport 852) — 3 rows ("3 entries") …
› leetcode: list, editor, typing, description
ok: list "Blind 75" — 72 rows in 16 sections … CodeMirror held 87 chars … accepted "verify_marker" …
› keyboard overlays on iOS
ok: iphone-15-pro → overlay; viewport 852px, reported inset 270px, rail 480–582 …
› keyboard resizes on Android
ok: pixel-8 → resize; viewport 645px, reported inset 0px, rail 543–645 …
ok: all checks passed
```

The service-report dashboard has no check here: it needs a live health-report endpoint, which a local export
does not have (its flow tests cover it).

Each check asserts on live DOM geometry and computed styles, not on presence:
presence passes for unstyled elements, for a sheet Tamagui keeps mounted
off-screen, and for a WebView that loaded nothing. Failures print the page's own
console errors and a screenshot path.

`test:runbook` is the same command — the runbook's automated half *is* the e2e
suite, and pointing them at two different things would let one rot.

## 2. Expo Go — manual, and the only thing that proves the native leg

CI cannot assert this: there is no phone in CI. **The record is this section,
filled in by a human with screenshots.**

The way in is an EAS Update: no tailnet needed, one immutable update per change
(no Fast Refresh). For a pushed commit it is the `### EAS Update` link of the CI
run summary; for the branch under test, publish one by hand:

```bash
wf ship eas --initial-route /
```

It prints one JSON line whose `link` is the EAS dashboard URL; a hand publish bakes
only the `EXPO_PUBLIC_*` variables of your environment, so the backends are not wired
in (Run Local and the coding agent need them; the steps below are UI only). Open the link on the phone and use **Open
with Expo Go** (the dashboard page carries the QR and the deep link: a bare
`exp://` URL pasted into a chat is generally not tappable, so the dashboard link
is the one to send).

Then walk the same path the browser suite walks, and screenshot each step:

| # | Step | What must be true |
|---|---|---|
| 1 | Cold start | The Blind 75 hero renders; the list is populated (72 problems, 11 sections) |
| 2 | Scroll the list | The hero scrolls away and the compact bar fades in; rows stay tappable the whole way |
| 3 | Open the drawer (☰) | Two destinations; picking "Design system" navigates and the drawer closes |
| 4 | Tap a problem row | The editor opens with the problem's title in the header |
| 5 | Tap into the editor | The **real iOS keyboard** comes up and both toolbar rails sit directly above it, not underneath |
| 6 | Type | Characters land in CodeMirror with syntax highlighting |
| 7 | Tap a toolbar key (Indent, Parentheses) | It inserts at the caret and the keyboard stays up |
| 8 | Tap ⓘ in the header | The description sheet slides up over a dimmed backdrop; it scrolls; pan-down dismisses it |
| 9 | Tap the debug FAB | The log sheet opens with live entries, newest first |

Step 5 is the one worth the trip. The browser suite emulates iOS's overlay
strategy (`docs/archive/solutions/testing/device-frame-keyboard-must-match-the-platform.md`),
but emulation of a safe-area behaviour is exactly the kind of thing that is
right in the harness and wrong on glass.

### Record

| Date | Branch | Update group | Device / OS | Result | Screenshots |
|---|---|---|---|---|---|
| _(not yet run)_ | `claude/ds-10-acceptance` | EAS `e209a7ed…` | — | — | — |

The row above is deliberately empty rather than assumed. No Expo Go pass has
been performed for this app: the update is published and loadable, and that is
a different claim from "it was opened on a phone and looked right".

**SDK 57 (issue #290) makes this row the blocking gate, not a nicety.** The
apps moved to SDK 57 precisely because the installed Expo Go (client 57.0.9,
supported SDK 57.0.0) refuses an SDK 54 bundle. Both apps now typecheck and
export for web on 57, and the browser half below passes — but whether Expo Go
57 actually loads and renders this bundle on glass is unverified, and only a
person holding the phone can fill it in.

### Regression spine

`frontend/mla-tamagui/e2e/verify-regression-spine.mjs` runs first in the suite
and fails when `frontend/libraries/tamagui-design-system` or this app's routes
(`frontend/mla-tamagui/app`) changed since the merge base with `origin/main`:
app #2 only means something as a side-by-side, so a moved reference voids the
comparison. App #1 (mobile-learning-app) and its design system have since been
deleted, so the old both-apps-identical rule no longer applies; a change made on
purpose is recorded with its reason in
`frontend/mla-tamagui/e2e/regression-spine-waivers.json` rather than silently
ignored, and any other change under those paths has no waiver and fails the
suite.

## 3. What is deliberately not here

- **No testID enforcement.** No lint rule asks for testIDs (the rules were dropped with the move to oxlint), and
  there is no `testids:extract|check`. Elements the checks actually
  drive carry hand-written semantic testIDs (`LeetcodeListSignIn`), not the
  generated positional kind a rewrite invalidates.
- **No unit suite.** Per this repo's test-priority order, runbook and e2e come
  first and unit last; app #2 has no logic of its own worth a unit test that the
  five browser checks do not already exercise end to end.
