---
title: "The old wf browser tap --ref verb failed on frame-nested pages: a ref-shape regex didn't recognize frame-prefixed refs"
date: 2026-08-29
category: developer-experience
module: "multi-browser (since removed; now the Rust wf browser in clis/cdp + clis/wf-harness)"
problem_type: developer_experience
component: tooling
root_cause: logic_error
resolution_type: code_fix
severity: high
applies_when:
  - "Driving the old tap --ref verb with a snapshot ref against a page in the default device-frame render mode (an Expo route or a third-party page reached via --url); the TypeScript multi-browser CLI's snapshot() returned frame-prefixed refs like f1e3 there, while the Rust wf browser's clis/cdp/src/snapshot.js returns bare eN kept in window.__wfRefs of the frame the snapshot ran in"
  - "tap --ref <ref> fails with \"no element resolved for ref\" even immediately after the snapshot() call that produced that ref"
tags:
  - "wf-browser"
  - "multi-browser"
  - "playwright"
  - "tap"
  - "snapshot-ref"
  - "browser-automation"
  - "aria-ref"
---

## Problem

Since the harness API (docs/plans/harness-api/spec.md, 2026-10-20) the `wf browser` CLI takes no refs: elements are found by
selector (`wf browser tap-on --id <testID>`), and `snapshot` became `wf browser hierarchy`. The daemon's `tap`, `type` and
`snapshot` RPC methods still take and make refs for @lib/browser-client, so what follows still holds for them.

`tap --ref <ref>` — passing a ref freshly returned by `snapshot` moments earlier — reliably failed with `no element resolved for ref "<ref>"` whenever the ref was frame-prefixed (`f1e3`, `f2e10`, ...), which was the normal shape the TypeScript CLI's `snapshotWithRefs()` produced in the default `device-frame` render mode (both for this repo's own Expo routes, once the app is embedded in the harness's `device-frame-app` iframe, and for any third-party page reached via `--url`). `type --ref` against the identical ref appeared to work every time, making it look like `tap` was simply broken and `type` was the reliable alternative.

## Symptoms

These are the symptoms under the TypeScript `multi-browser` CLI:

- `tap --ref f1e3` (or any `f<N>e<M>`-shaped ref) failed with `no element resolved for ref "f1e3"`, even when that exact ref came from a `snapshot()` call moments before, with nothing else executed in between.
- `tap --ref e3` (a *bare* `eN` ref, with no frame prefix) resolved fine — this only reproduced on frame-nested content.
- `type --ref f1e3` against the same page and a genuinely different (non-first) contenteditable element correctly landed text in that specific element — not a coincidental fallback (verified: typing into a second contenteditable via its ref, distinct from the first, produced the right text in the right element).

The Rust `wf browser` never produces an `f<N>e<M>` ref and treats one as a CSS selector: `tap --ref f1e3` and `type --ref f1e3` both run `document.querySelector('f1e3')` in `clis/cdp/src/attached.rs` and fail with `tap: no element resolved for ref "f1e3"` and `type: no element resolved for ref "f1e3"`.

## What Didn't Work

The first fix attempt was routing `tap()`'s resolution through the top-level `page` instead of the nested `target` (`Frame`) — matching what `type()` already appeared to do. Rebuilt and tested live: still failed with the identical "no element resolved" error. This ruled out "Frame vs Page scoping" as the *sole* cause, and the investigation was shelved as only partially characterized (see GitHub issue [#232](https://github.com/nuesslerm/expo-hello-world-app-2/issues/232)).

## Solution

The real defect was a classification regex, not a scoping problem — though scoping is also part of the fix. The TypeScript `multi-browser` CLI's `resolveRef()` (its `ref-resolver.ts`) decided whether a `--ref` string is an `aria-ref=` locator or a raw CSS/text selector using:

```ts
const ARIA_REF_SHAPE = /^e\d+$/;
```

This only matches a *bare* `eN`. Playwright's `ariaSnapshot({mode:'ai'})` prefixes ref ids with a frame index (`f1e3`, `f2e10`, ...) whenever the snapshot is taken from anything other than the main frame — exactly what `snapshotWithRefs(target)` does in device-frame mode, since `target` is the nested `device-frame-app` frame (or deeper). So `resolveRef(target, "f1e3")` tested `ARIA_REF_SHAPE.test("f1e3")`, got `false` (the string starts with `f`, not `e`), and took the *raw selector* branch: `target.locator("f1e3")` — a CSS lookup for a literal `<f1e3>`-named element, which obviously matches nothing. `type()` never had this classification step; its code always built `page.locator('aria-ref=' + ref)` unconditionally, regardless of shape, which is why it always treated the ref correctly and `tap()` didn't.

Fixing only the regex (`/^(?:f\d+)*e\d+$/`) was not sufficient on its own — resolving the now-correctly-classified `aria-ref=f1e3` via `target.locator(...)` (the Frame) still failed. The second half was required too: resolve every aria-ref-shaped ref through the top-level `page`, never through a `Frame`, confirmed empirically — `frame.locator('aria-ref=f1e3')` fails to resolve a ref that `frame.locator('body').ariaSnapshot(...)` itself just produced, while `page.locator('aria-ref=f1e3')` resolves the identical ref correctly, even across a cross-origin nested iframe. `resolveRef()` now takes both objects explicitly and dispatches on ref shape:

```ts
export function resolveRef(scopedTarget: BrowsingTarget, hostPage: Page, ref: string): Locator {
  return ARIA_REF_SHAPE.test(ref)
    ? hostPage.locator(`aria-ref=${ref}`)
    : scopedTarget.locator(ref);
}
```

`type()` was also switched to call this same function (it previously duplicated the `aria-ref=` construction inline, with no shape check at all — a CSS/text-selector ref passed to `type --ref` was silently mishandled the same way `tap` mishandled aria-refs, just masked by `type`'s own contenteditable fallback instead of a hard error).

Verified with a deterministic repro (a `data:`-style test page with two distinct buttons and two distinct contenteditable elements, loaded via `--url` in default device-frame mode, reproducing the same frame-nesting `snapshotWithRefs` sees on any real page): `tap` on two *different* frame-prefixed refs now clicks the two distinct, correct buttons (confirmed via a JS state flag each button sets); `type` on two different frame-prefixed refs lands text in the two distinct, correct contenteditable elements (not the fallback's always-first-element behavior). Also re-verified on this repo's own real leetcode CM6 editor route, and the full `multi-browser` vitest suite (46 tests, including e2e tests that drive real headless Chromium) still passed.

That TypeScript CLI was later replaced by the Rust `wf browser`, which drives Chromium over CDP rather than through Playwright, so `resolveRef()` and frame-prefixed refs are gone. `clis/cdp/src/snapshot.js` numbers refs as bare `eN` and keeps them in `window.__wfRefs` of the frame it ran in (in device-frame mode, the `device-frame-app` frame or an editable frame inside it, chosen by `use_app_target()` in `clis/harness-browser/src/browser_launch.rs`; `snapshot` always takes the editable frame when there is one, since `clis/harness-browser/src/browser_daemon.rs` rejects `snapshot --outer`). `clis/cdp/src/attached.rs` looks a ref up in the frame the command targets, which the daemon picks again on each command through `use_app_target()`: `bounding_box()` (behind `tap`, `drag`, `stroke`, `trace` and `swipe`) and `type_text()` resolve a `/^e\d+$/` ref from that frame's `window.__wfRefs` and anything else with `document.querySelector`. That frame is the snapshot's frame only when the command does not pass `--outer` and the editable frame has not appeared or changed since the snapshot. `tap --outer` and `type --outer` target `device-frame-app`, so on a page with an editable child frame (the leetcode CM6 editor route), `snapshot` followed by `tap --outer --ref e3` looks `e3` up in `device-frame-app`'s `window.__wfRefs` and fails with `tap: no element resolved for ref "e3"`, the same frame-mismatch class of failure this doc is about.

## Why This Works

`ARIA_REF_SHAPE` is used purely to decide which of two Playwright locator strategies to use — there was never a need for it to reject frame-prefixed refs; that was simply an oversight in a regex written before frame-nested refs were ever exercised (this repo's earliest `wf browser` testing likely used `plain` render mode or top-level-page refs, where bare `eN` is all that's ever produced). Broadening the regex without also fixing the resolution target would have "correctly" built `frame.locator('aria-ref=f1e3')` and still failed, because Playwright's aria-ref selector engine answers cross-frame queries only when asked through the `Page` object — resolving via the `Frame` a ref happened to be scoped to at snapshot time does not work, even for a ref that frame itself just produced.

## Prevention

1. **A shape-detection regex should be widened the moment a new, valid shape is discovered producing false negatives** — the fix here is a two-character change to the regex, but it went unfixed for a full investigation (including a GitHub issue documenting it as "partially characterized" and a live-tested-but-reverted candidate fix) because the first fix attempt addressed a plausible but incomplete theory (Frame vs Page) without also checking the shape-classification step that fed into it.
2. **A regression guard was added**: the TypeScript CLI's `ref-resolver.test.ts` unit-tested `resolveRef()` directly — asserting a frame-prefixed ref (`f1e3`, and a multiply-nested `f2f7e19`) is classified as an aria-ref and resolved via the host page, a bare `eN` is too, and a CSS/text selector still resolves via the scoped target. Verified live to catch this exact regression: re-running against the pre-fix single-argument `resolveRef(page, ref)` implementation fails all four assertions. That test was deleted with the TypeScript CLI; in the Rust port, `an_attached_session_taps_types_drags_strokes_and_swipes_like_a_user` in `clis/wf-harness/tests/flow/browser.rs` taps an element by its snapshot ref, by a CSS selector, and by a selector that matches nothing. That test runs on an attached session, which has no device frame (`clis/harness-browser/src/browser_daemon.rs` says so outright: "attached sessions have no frame"), and the phone-framed tests (`a_phone_framed_session_*`) tap only CSS selectors, so tapping a snapshot ref inside the `device-frame-app` frame, the case this doc is about, has no flow test in the Rust port.
3. When a "the natural fix didn't work" investigation gets shelved as partially characterized, the next person picking it up should re-derive the failure from the actual code (not just retry the same hypothesis) — reading `ref-resolver.ts`'s regex directly, rather than only `session.ts`'s Frame/Page handling, is what surfaced the real defect here.

## Related

- **GitHub issue [#232](https://github.com/nuesslerm/expo-hello-world-app-2/issues/232)** — the original investigation (root cause not yet found), now updated with this fix.
- `.agents/skills/browser-cli/SKILL.md` — the `wf browser` reference skill; its "Pitfalls" section previously recommended a CSS/text locator workaround for `tap --ref`, now updated to reflect that a snapshot ref works correctly again.
- GitHub issues #229, #230, #231 — other bugs in the TypeScript `multi-browser` CLI behind `wf browser` (since replaced by the Rust port) found in the same broader investigation (daemon self-registration, registry write no-op, WebSocket-over-SOCKS5 transport failure). Distinct root causes and files from this finding; not overlapping.
