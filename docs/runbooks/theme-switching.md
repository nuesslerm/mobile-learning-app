---
title: Theme switching manual test
verifies: The preset theme system applies correctly on both web and Expo Go
tier: agent-driven
packages: features/theme-customizer, features/theme-customizer-impl, features/theme-customizer-mocks
maxAgeDays: 60
---

# Theme Switching — Manual Test Runbook

Verify the preset theme system on BOTH web (Playwright MCP) and Expo Go (`wf android`).
Complete all steps on one platform before switching to the other.

> **Critical note (web):** NativeWind's `vars()` injects CSS custom properties on the
> `ThemeVarsProvider` View element, NOT on `document.documentElement`. All CSS var
> assertions must query `[data-testid="ThemeVarsProvider-View1"]`.

---

## Web (Playwright MCP at 127.0.0.1:8800/mla-tamagui)

**Setup:**

```bash
wf test prepare --web   # the web export of this tree; serve frontend/mla-tamagui/dist under /mla-tamagui on 127.0.0.1:8800 (the Setup of docs/runbooks/device-lab-codemirror-typing.md)
```

Helper snippet — read vars from the correct element:

```js
const el = document.querySelector('[data-testid="ThemeVarsProvider-View1"]');
getComputedStyle(el).getPropertyValue('--primary').trim();
```

---

### Step 1 — Settings screen renders

Action: `browser_navigate({ url: 'http://127.0.0.1:8800/mla-tamagui/settings' })`

Expected:

- "Settings" h2 heading visible
- "APPEARANCE" section with Auto/Light/Dark toggle (Auto selected by default)
- "Theme" section heading + hint text
- "DEFAULT" category header + Classic Duolingo card
- "LIGHT PALETTES" category with Ocean Breeze, Forest Fresh, Desert Dawn, Pastel Pop
- "DARK & DRAMATIC" and "WARM" categories below

Verify:

```js
// Auto toggle active
!!document.querySelector('[data-testid="ThemeModeToggle-system"]');
// Classic Duolingo card present
!!document.querySelector('[data-testid="PresetCard-classic-duolingo"]');
// Ocean Breeze card present
!!document.querySelector('[data-testid="PresetCard-ocean-breeze"]');
```

Result: ✅

---

### Step 2 — Default CSS vars (Classic Duolingo light)

Action: Run evaluate immediately after Step 1 (no interaction yet).

Verify:

```js
const el = document.querySelector('[data-testid="ThemeVarsProvider-View1"]');
getComputedStyle(el).getPropertyValue('--primary').trim();
// → "88 204 2"
```

Result: ✅

---

### Step 3 — Select a preset (no immediate apply)

Action: `browser_click({ target: '[data-testid="PresetCard-ocean-breeze"]' })`

Expected:

- Ocean Breeze card gains a border ring
- "Selected" text badge appears top-right of the card
- Apply CTA button appears at bottom of the list
- CSS vars are NOT changed yet (still `88 204 2`)

Verify:

```js
const el = document.querySelector('[data-testid="ThemeVarsProvider-View1"]');
({
  primaryUnchanged:
    getComputedStyle(el).getPropertyValue('--primary').trim() === '88 204 2',
  selectedLabel: document.querySelector(
    '[data-testid="PresetCard-PendingLabel-ocean-breeze"]'
  )?.textContent,
  applyBtnVisible: !!document.querySelector(
    '[data-testid="PresetThemesGrid-ApplyBtn"]'
  ),
});
// → { primaryUnchanged: true, selectedLabel: 'Selected', applyBtnVisible: true }
```

Result: ✅

---

### Step 4 — Apply the preset

Action: `browser_click({ target: '[data-testid="PresetThemesGrid-ApplyBtn"]' })`

Expected:

- "Selected" badge disappears
- Apply button disappears
- Ocean Breeze card shows "Active" text + checkmark icon
- CSS vars update to Ocean Breeze light palette

Verify:

```js
const el = document.querySelector('[data-testid="ThemeVarsProvider-View1"]');
({
  primary: getComputedStyle(el).getPropertyValue('--primary').trim(),
  // → "0 119 182"
  activeLabel: document.querySelector(
    '[data-testid="PresetCard-ActiveLabel-ocean-breeze"]'
  )?.textContent,
  // → "Active"
  applyGone: !document.querySelector(
    '[data-testid="PresetThemesGrid-ApplyBtn"]'
  ),
  // → true
});
```

Result: ✅

---

### Step 5 — Toggle Dark mode

Action: `browser_click({ target: '[data-testid="ThemeModeToggle-dark"]' })`

Expected:

- CSS vars switch to Ocean Breeze dark palette
- Background becomes dark

Verify:

```js
const el = document.querySelector('[data-testid="ThemeVarsProvider-View1"]');
({
  primary: getComputedStyle(el).getPropertyValue('--primary').trim(),
  // → "0 149 227"  (brighter blue for dark mode)
  background: getComputedStyle(el).getPropertyValue('--background').trim(),
  // → "12 14 18"
});
```

Result: ✅

---

### Step 6 — Persistence across reload

Action: `browser_navigate({ url: 'http://127.0.0.1:8800/mla-tamagui/settings' })` (full reload)

Expected: Ocean Breeze Dark persists — same CSS vars, same "Active" indicator.

Verify:

```js
const el = document.querySelector('[data-testid="ThemeVarsProvider-View1"]');
({
  primary: getComputedStyle(el).getPropertyValue('--primary').trim(),
  // → "0 149 227"
  background: getComputedStyle(el).getPropertyValue('--background').trim(),
  // → "12 14 18"
  activeLabel: document.querySelector(
    '[data-testid="PresetCard-ActiveLabel-ocean-breeze"]'
  )?.textContent,
  // → "Active"
});
```

Result: ✅

---

### Step 7 — Restore defaults (cleanup)

Action: Select and apply Classic Duolingo, switch mode back to Auto.

```js
// Verify restored
const el = document.querySelector('[data-testid="ThemeVarsProvider-View1"]');
getComputedStyle(el).getPropertyValue('--primary').trim();
// → "88 204 2"
```

---

## Expo Go (`wf android`)

**Setup:**

```bash
wf android open-link exp://u.expo.dev/<projectId>/group/<group>
# once the app runs, the route
wf android open-link exp://u.expo.dev/<projectId>/group/<group>/--/settings
```

`<projectId>` is the `projectId` in `frontend/mla-tamagui/app.config.js`; `<group>` is in the `### EAS Update` summary of the CI run for the pushed commit (a branch: `wf ship eas --initial-route <route>`, which also starts cold on that route).

A cold start opens the bare update link: its manifest answers 404 under a `/--/<route>` path while Expo Go is not running yet (`e2e/warm-app.lua`, CI run 39593152492), so a route link is for an app that already runs.

---

### Step 1 — Settings screen renders

Action: `wf android take-screenshot <output.png>` after launch.

Expected:

- "Settings" heading at top
- APPEARANCE section with Auto/Light/Dark toggle pills
- "Theme" section heading + hint text
- DEFAULT category with Classic Duolingo card (green/gray/blue swatches)
- LIGHT PALETTES category visible below

Verify: Screenshot shows all expected sections (read the sections off the screenshot — there is no list-elements equivalent).

---

### Step 2 — Select a preset

Action: Find the Ocean Breeze card's coordinates by reading the screenshot, then `wf android tap-on --point <x>,<y>`.

Expected:

- Ocean Breeze card gains border ring
- "Selected" badge appears on the card
- Scroll down to confirm Apply button is visible at bottom

Verify: `wf android take-screenshot <output.png>` — card should show green ring + "Selected" text.

---

### Step 3 — Apply the preset

Action: `wf android tap-on --point <x>,<y>` on the Apply button.

Expected:

- Apply button disappears
- Ocean Breeze shows "Active" + checkmark
- Tab bar tint color changes from green to blue (Ocean Breeze primary)

Verify: `wf android take-screenshot <output.png>` — confirm "Active" label visible on Ocean Breeze card.

---

### Step 4 — Dark mode toggle

Action: `wf android tap-on --point <x>,<y>` on the "🌙 Dark" pill.

Expected:

- Screen background changes to dark (#0C0E12)
- Text becomes light-colored
- Tab bar darkens

Verify: `wf android take-screenshot <output.png>` — dark surfaces visible.

---

### Step 5 — Persistence across app restart

Action:

```bash
# Terminate and cold start (bare link), then the route
wf android run-shell 'am force-stop host.exp.exponent'
wf android open-link exp://u.expo.dev/<projectId>/group/<group>
wf android open-link exp://u.expo.dev/<projectId>/group/<group>/--/settings
```

Expected: Ocean Breeze Dark still active after restart.

Verify: `wf android take-screenshot <output.png>` — "Active" label on Ocean Breeze, dark background.

---

## Pass Criteria

All steps ✅ on BOTH platforms before writing automated tests.

| Step                                  | Web | Expo Go |
| ------------------------------------- | --- | ------- |
| 1. Screen renders with categories     | ✅  | ✅      |
| 2. Default CSS vars correct           | ✅  | ✅      |
| 3. Tap-to-select (no immediate apply) | ✅  | ✅      |
| 4. Apply updates vars + Active badge  | ✅  | ✅      |
| 5. Dark mode toggle                   | ✅  | ✅      |
| 6. Persistence across reload/restart  | ✅  | ✅      |

**Phase 3 PASSED on both platforms — automated tests may now be written.**
