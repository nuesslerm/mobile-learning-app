---
title: Debug overlay tab manual test
verifies: The Debug Overlay debug-tab survives the cold-start deep-link + lazy-mount fix without a navigation-context crash, on web and Expo Go
tier: agent-driven
maxAgeDays: 60
---

# Debug Overlay Tab — Manual Test Runbook

Verify the **Debug Overlay** debug-tab in the in-app debug sheet (FAB → sheet → "Overlay" tab) on BOTH web (Playwright MCP at `127.0.0.1:8800/mla-tamagui`) and Expo Go (`wf android`). This is regression coverage for the cold-start deep-link + lazy-mount fix that prevents the navigation-context crash.

**Bug context:** Previously, `DebugSheet.tsx` used a `prevRef` seeded to `true` to detect the `false→true` edge for lazily mounting `DebugSheetContent`. When a `?debugTab=overlay` deep-link set `open=true` on the very first render, the edge never fired and the sheet body was blank (or threw the navigation-context error). Fix: replace `prevRef` with a `mounted` state, keyed on `open && !mounted`. The deep-link path must now reliably open the sheet on the requested tab on cold-start.

The Overlay tab itself holds four global toggles (outlines / random background / labels / include debug surface) and a per-component denylist filter, all backed by a persisted zustand store (`debug-overlay.v1` in `localStorage` / AsyncStorage).

**Feature flag:** `EXPO_PUBLIC_DEBUG_OVERLAY` must NOT be `'false'` and `isFeatureEnabled('debug_overlay')` must return `true`. The dev/staging builds enable it by default.

---

## Web (Playwright MCP at 127.0.0.1:8800/mla-tamagui)

**Setup:**

```bash
wf test prepare --web   # the web export of this tree; serve frontend/mla-tamagui/dist under /mla-tamagui on 127.0.0.1:8800 (the Setup of docs/runbooks/device-lab-codemirror-typing.md)
```

**Open via deep-link (covers the cold-start fix):**

1. **Cold-start deep-link opens sheet on Overlay tab** — `browser_navigate({ url: 'http://127.0.0.1:8800/mla-tamagui/?debugTab=overlay' })`
   - Expected: page loads, debug sheet visible, **Overlay** tab is active (not Logs).
   - Verify: `browser_evaluate(() => document.querySelector('[data-testid="DebugSheetContainer"]') !== null)` → `true`
   - Verify: `browser_evaluate(() => document.querySelector('[data-testid="OverlayTab-View1"]') !== null)` → `true`
   - Verify: `browser_evaluate(() => document.querySelector('[data-testid="OverlayTab-Outlines-Switch"]') !== null)` → `true`
   - Screenshot: `browser_take_screenshot({ filename: 'debug-overlay-cold-start.png' })` — should show Overlay tab contents, NOT a blank sheet body.

**Open via FAB (covers the normal in-app path):** 2. **FAB opens the sheet on the default Logs tab** — `browser_navigate({ url: 'http://127.0.0.1:8800/mla-tamagui/' })` (no deep-link param), then `browser_click({ selector: '[data-testid="DebugFab"]' })`.

- Expected: sheet appears on the **Logs** tab (default).
- Verify: `browser_evaluate(() => document.querySelector('[data-testid="DebugSheetContainer"]') !== null)` → `true`

3. **Tap Overlay tab inside the sheet** — `browser_click({ selector: '[data-testid="DebugTab-Overlay"]' })`.
   - Expected: tab content swaps to the Overlay tab controls.
   - Verify: `browser_evaluate(() => document.querySelector('[data-testid="OverlayTab-View1"]') !== null)` → `true`
   - Verify: presence of all four master switches: `OverlayTab-Outlines-Switch`, `OverlayTab-RandomBg-Switch`, `OverlayTab-Labels-Switch`, `OverlayTab-IncludeDebugSurface-Switch`.

**Functional behavior of the Overlay tab toggles (persisted):** 4. **Clear persisted state** — before functional steps, `browser_evaluate(() => localStorage.removeItem('debug-overlay.v1'))` and reload with `browser_navigate({ url: 'http://127.0.0.1:8800/mla-tamagui/?debugTab=overlay' })`.

- Expected: all four master switches start `false` (default).
- Verify: `browser_evaluate(() => document.querySelector('[data-testid="OverlayTab-Outlines-Switch"]')?.getAttribute('aria-checked') ?? document.querySelector('[data-testid="OverlayTab-Outlines-Switch"]')?.getAttribute('data-state'))` → `'false'` or similar off-state.

5. **Toggle Outlines on** — `browser_click({ selector: '[data-testid="OverlayTab-Outlines"]' })` (tap the row).
   - Expected: switch flips to on.
   - Verify: `browser_evaluate(() => document.querySelector('[data-testid="OverlayTab-Outlines-Switch"]').getAttribute('aria-checked'))` → `'true'`.
   - Verify: `browser_evaluate(() => JSON.parse(localStorage.getItem('debug-overlay.v1') || '{}').state?.outlines)` → `true`.

6. **Toggle Random Background on** — `browser_click({ selector: '[data-testid="OverlayTab-RandomBg"]' })`.
   - Expected: `data-state="checked"` (or `aria-checked="true"`) on `OverlayTab-RandomBg-Switch`; `randomBg: true` in localStorage.

7. **Toggle Labels on** — `browser_click({ selector: '[data-testid="OverlayTab-Labels"]' })`.
   - Expected: switch on; `labels: true` in localStorage.

8. **Toggle Include debug surface** — `browser_click({ selector: '[data-testid="OverlayTab-IncludeDebugSurface"]' })`.
   - Expected: switch on; `includeDebugSurface: true` in localStorage.

**Per-component filter section:** 9. **Components heading is visible** — `browser_evaluate(() => document.querySelector('[data-testid="OverlayTab-Components-Heading"]')?.textContent)` contains `"Components"`.

10. **Component rows render (or empty state shows)** — depending on whether any DS primitives are mounted on the current route, either:
    - `[data-testid="OverlayTab-Components-Empty"]` is visible with text "No instrumented components on screen yet.", OR
    - At least one `[data-testid^="OverlayTab-Component-"][data-testid$="-Switch"]` is present.

11. **"All component overlays" master toggles all groups** — `browser_click({ selector: '[data-testid="OverlayTab-Components-All"]' })`.
    - Expected: every individual `[data-testid="OverlayTab-Component-<name>-Switch"]` flips to off; `disabledComponents` in localStorage contains entries for every known name.
    - Tap again: every group re-enables; `disabledComponents` is `{}`.

**Persistence across reload:** 12. **Reload preserves toggles** — after steps 5–8 (all four masters ON), `browser_navigate({ url: 'http://127.0.0.1:8800/mla-tamagui/?debugTab=overlay' })`.

- Expected: all four switches still on (state hydrated from localStorage).
- Verify: each `-Switch` element still shows `aria-checked="true"` after reload.

**Other tabs still reachable (regression: deep-link doesn't break the tab bar):** 13. **Switch to Logs tab** — `browser_click({ selector: '[data-testid="DebugTab-Logs"]' })`.

- Expected: Overlay tab content unmounts, Logs tab content mounts. `[data-testid="OverlayTab-View1"]` is no longer in the DOM; the Logs tab UI is visible.
- Verify: `browser_evaluate(() => document.querySelector('[data-testid="OverlayTab-View1"]'))` → `null`.

14. **Switch back to Overlay tab** — `browser_click({ selector: '[data-testid="DebugTab-Overlay"]' })`.

- Expected: Overlay tab content returns with the same toggle state.

15. **Close the sheet** — `browser_click({ selector: '[data-testid="DebugSheetClose"]' })`.

- Expected: sheet collapses; `DebugSheetContainer` may stay in DOM but its inner sheet content is hidden.
- Verify: `browser_evaluate(() => document.querySelector('[data-testid="OverlayTab-View1"]'))` → `null` (content unmounted).

---

## Expo Go (`wf android`)

**Setup:**

```bash
wf android open-link exp://u.expo.dev/<projectId>/group/<group>
```

`<projectId>` is the `projectId` in `frontend/mla-tamagui/app.config.js`; `<group>` is in the `### EAS Update` summary of the CI run for the pushed commit (a branch: `wf ship eas --initial-route <route>`, which also starts cold on that route).

A cold start opens the bare update link: its manifest answers 404 under a `/--/<route>` path while Expo Go is not running yet (`e2e/warm-app.lua`, CI run 39593152492), so a `/--/<route>?query` link is for an app that already runs.

The debug overlay is gated on `EXPO_PUBLIC_DEBUG_OVERLAY !== 'false'` and `isFeatureEnabled('debug_overlay')`. Dev/staging builds enable it by default.

1. **Deep-link opens sheet on Overlay tab** — with the app running (after the Setup's bare link), `wf android open-link 'exp://u.expo.dev/<projectId>/group/<group>/--/?debugTab=overlay'`; a cold start cannot take the link (the Setup says why), so the tab is reached by the link after it.
   - Expected: app opens, debug sheet is visible, **Overlay** tab is active.
   - Verify: `wf android take-screenshot <output.png>` → sheet body shows the four Overlay toggles + Components heading; no blank/white sheet body; no crash dialog.

2. **FAB opens the sheet** — `wf android take-screenshot <output.png>` to find the FAB (look for a small bug icon at bottom-right), read its coords off the screenshot. `wf android tap-on --point <x>,<y>` on the FAB.
   - Expected: sheet appears on the default **Logs** tab.

3. **Tap Overlay tab** — `wf android take-screenshot <output.png>` → find the "Overlay" tab button in the tab bar, read its coords → `wf android tap-on --point <x>,<y>`.
   - Expected: tab content swaps to Overlay controls. Screenshot confirms four switches and Components section.

4. **Toggle each master** — tap each toggle row (`OverlayTab-Outlines`, `-RandomBg`, `-Labels`, `-IncludeDebugSurface`).
   - Expected: switches flip state. `wf android take-screenshot <output.png>` after each tap to record the visual flip.

5. **Components section** — verify heading `OverlayTab-Components-Heading` reads "Components". If the Home screen has DS primitives mounted, per-component rows appear. Tap "All component overlays" → all rows off → tap again → all rows on.

6. **Persistence across cold restart** — `wf android run-shell 'am force-stop host.exp.exponent'`, then `wf android open-link exp://u.expo.dev/<projectId>/group/<group>` (bare, as in the Setup; once the app runs, `exp://u.expo.dev/<projectId>/group/<group>/--/?debugTab=overlay` lands on the Overlay tab).
   - Expected: toggles preserved from the previous session (AsyncStorage).
   - Verify: re-open the Overlay tab and confirm switches still show their previous state.

7. **Other tabs reachable** — tap Logs / Info / State / Theme / Stories / Agent tabs in sequence, then return to Overlay.
   - Expected: every tab renders without crashing. No navigation-context error in logs.

8. **Close sheet** — tap `DebugSheetClose` (or drag the sheet handle to dismiss).
   - Expected: sheet animates away; FAB remains.

---

## Pass Criteria

All web steps (1–15) AND all Expo Go steps (1–8) must pass before automated tests are written. If any step fails:

1. Stop Phase 3.
2. Document the failure: step number, platform, expected vs actual.
3. Fix the bug in the app code (most likely `DebugSheet.tsx`, `DebugSheetContent.tsx`, or `useDebugTabDeepLink.ts`).
4. Restart Phase 3 from step 1 on both platforms.
5. Never proceed to Phase 4 (automation) with a known failure.

**Most critical regression target:** Step 1 (cold-start deep-link). If that fails, the original bug has regressed.

---

## Notes for the implementer

- **localStorage key for cleanup:** `debug-overlay.v1` (zustand `persist` middleware; the actual state lives under `state` inside the persisted blob — see `debugOverlayStore.ts:91`).
- **Deep-link param:** `?debugTab=<tab>` (read via `useGlobalSearchParams` in `useDebugTabDeepLink.ts`).
- **Tab trigger testIDs:** `DebugTab-Logs`, `DebugTab-Info`, `DebugTab-State`, `DebugTab-Theme`, `DebugTab-Stories`, `DebugTab-Agent`, `DebugTab-Overlay` (see `DebugSheetContent.tsx:136`).
- **Overlay tab testIDs:** `OverlayTab-Outlines`, `OverlayTab-RandomBg`, `OverlayTab-Labels`, `OverlayTab-IncludeDebugSurface`, `OverlayTab-Outlines-Switch`, etc. (see `OverlayTab.tsx:36-67`).
- **Switch state attribute:** React Native Web renders `<Switch>` as a `<button role="switch">` with `aria-checked="true"|"false"` AND a `data-state="checked"|"unchecked"` attribute. Assert on whichever the actual DOM has (prefer `aria-checked`).
- **Persistence:** the dev overlay writes through to AsyncStorage (`keyValueStorage` shim, which uses localStorage on web). Reload to verify.
