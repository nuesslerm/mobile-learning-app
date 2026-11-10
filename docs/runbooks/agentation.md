---
title: Agentation annotation tool manual test
verifies: The Agentation tool captures the correct element under a tap on both web and Expo Go
tier: agent-driven
maxAgeDays: 60
---

# Agentation — Manual Test Runbook

Verify that the Agentation annotation tool captures the correct element under the tap on BOTH web (Playwright MCP) and Expo Go (`wf android`). Complete all steps on one platform before switching to the other.

---

## Web (Playwright MCP at 127.0.0.1:8800/mla-tamagui)

**Setup:**

```bash
wf test prepare --web   # the web export of this tree; serve frontend/mla-tamagui/dist under /mla-tamagui on 127.0.0.1:8800 (the Setup of docs/runbooks/device-lab-codemirror-typing.md)
```

Open the Playwright MCP browser in device-frame mode and navigate to the storybook route.

---

### Step 1 — Navigate to Storybook and locate the Forms text input

Action:

- `browser_navigate({ url: 'http://127.0.0.1:8800/mla-tamagui/design-system/storybook' })`
- Scroll the storybook scroll view until the "Forms" section is visible (the first email input with testID `Bluesky-Forms-Default`).

Verify:

```js
const input = document.querySelector('[data-testid="Bluesky-Forms-Default"]');
!!input && input.getBoundingClientRect().height > 0;
// → true
```

Result: ✅

---

### Step 2 — Open the Agentation overlay

Action:

- Tap the Debug FAB to open the Debug Sheet.
- Switch to the "Agentation" tab.
- Tap the "Start Annotating" button.

Verify:

```js
window.__agentation;
// → object with toggle, annotateAt, getAnnotations, etc.
```

Result: ✅

---

### Step 3 — Capture the text input via the dev hook

Action:

```js
const input = document.querySelector('[data-testid="Bluesky-Forms-Default"]');
const rect = input.getBoundingClientRect();
const x = rect.left + rect.width / 2;
const y = rect.top + rect.height / 2;
window.__agentation.annotateAt(x, y);
```

Expected: The inline annotation editor opens with the captured component.

Verify:

```js
const anns = window.__agentation.getAnnotations();
anns.length === 1;
// → true
```

Result: ✅

---

### Step 4 — Confirm the captured component is the input, not a layout wrapper

Action: Read the component name in the inline editor.

Expected:

- Component name shows `Input` (or the design-system input component name), **NOT** `FrameSizeListenerWeb`.
- Source file shows `design-system/storybook.tsx` (or similar), **NOT** `_layout.tsx`.

Verify:

```js
// The editor renders the component name here:
const nameEl = document.querySelector('[data-testid="AgentationEditAnnotation-ComponentName"]');
nameEl.textContent;
// → should contain "Input"
```

Result: ✅ (component name = `Input`, source = `design-system/storybook.tsx`)

---

---

### Step 5 — Deselect and inspect the marker

Action:

```js
window.__agentation.deselect();
```

Expected: The inline editor closes. The Debug Sheet remains open. The annotation marker (red outline) is drawn around the captured input; it may be partially behind the Debug Sheet.

To see the marker fully, close the Debug Sheet by tapping its Close button, then:

```js
const marker = document.querySelector('[data-testid="AgentationDot-1"]');
!!marker;
// → true
```

Result: ✅ (red dashed outline around the first email input, label "Input"; visible after closing the Debug Sheet)

---

### Step 6 — Capture a second element

Action:

```js
const btn = document.querySelector('[data-testid="Bluesky-Button-destructive"]');
const rect = btn.getBoundingClientRect();
window.__agentation.annotateAt(rect.left + rect.width / 2, rect.top + rect.height / 2);
```

Expected: A second annotation is created.

Verify:

```js
window.__agentation.getAnnotations().length === 2;
// → true
```

Result: ✅

**Setup:**

```bash
wf android open-link exp://u.expo.dev/<projectId>/group/<group>
# once the app runs, the route
wf android open-link exp://u.expo.dev/<projectId>/group/<group>/--/design-system/storybook
```

`<projectId>` is the `projectId` in `frontend/mla-tamagui/app.config.js`; `<group>` is in the `### EAS Update` summary of the CI run for the pushed commit (a branch: `wf ship eas --initial-route /design-system/storybook`, which also starts cold on that route).

A cold start opens the bare update link: its manifest answers 404 under a `/--/<route>` path while Expo Go is not running yet (`e2e/warm-app.lua`, CI run 39593152492), so a route link is for an app that already runs.

> Note: Mobile-mcp interaction tools are required to execute the steps below. If they are unavailable in the current environment, perform the equivalent actions manually on a connected device/simulator and record the result.

### Step 1 — Launch storybook on the simulator

Action: Cold-start Expo Go with `wf android open-link exp://u.expo.dev/<projectId>/group/<group>`, then open the storybook route with `wf android open-link exp://u.expo.dev/<projectId>/group/<group>/--/design-system/storybook`.

Expected: Storybook renders, and the Debug FAB is visible.

Result:

---

### Step 2 — Open Agentation

Action:

- Tap the Debug FAB.
- Tap the "Agentation" tab.
- Tap "Start Annotating".

Expected: The overlay arms (capture surface active).

Result:

---

### Step 3 — Long-press the text input

Action:

- Scroll the storybook to the Forms section.
- Long-press the first email input.

Expected: The inline annotation editor opens.

Result:

---

### Step 4 — Confirm the captured component

Expected: The editor title/component name is the input, not the root layout.

Result:

---

### Step 5 — Inspect the marker after closing the sheet

Action:

- Close the Debug Sheet.
- Observe the annotation marker around the input.

Expected: A red outline is drawn around the input bounds.

Result:

---

## Pass Criteria

All web steps pass and the captured component is the expected element (not a layout wrapper) before automated tests are written. Expo Go steps should pass when `wf android` interaction tooling is available; if not, record the manual verification result.
