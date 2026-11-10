---
title: rn-agentation bottom sheet, wireframe, and MCP
verifies: The three-stage rn-agentation work renders and behaves correctly on the web surface
tier: agent-driven
maxAgeDays: 60
---

# Runbook: rn-agentation bottom sheet + wireframe + MCP

## Scope

Verifies the three-stage `rn-agentation` work on the **web** surface (Expo web
UI) because the bottom-sheet and wireframe UI is the same cross-platform code
that runs inside Expo Go on native.

## Preconditions

```bash
# Build the web export of this tree; serve frontend/mla-tamagui/dist under /mla-tamagui on 127.0.0.1:8800
# (the Setup of docs/runbooks/device-lab-codemirror-typing.md).
wf test prepare --web
```

Open `http://127.0.0.1:8800/mla-tamagui/design-system/storybook` with the Playwright MCP browser tools. Do **not**
run `expo start` or `pnpm --filter mobile-learning-app run web` — both are blocked by the agent-approve guard /
navigation hook.

For automated clipboard assertions in Playwright, stub `writeText` first:

```js
window.__agentationClipboard = null;
const orig = navigator.clipboard.writeText.bind(navigator.clipboard);
navigator.clipboard.writeText = async (text) => {
  window.__agentationClipboard = text;
  return orig(text);
};
```

## Test 1 — Annotate an element

1. Navigate to the slot's storybook URL.
2. Confirm a single **Annotate** FAB is visible (bottom-right, pencil icon).
3. Click the FAB; confirm it turns into a red **Stop annotating** button and a
   bottom-sheet peek appears.
4. Confirm the sheet shows the mode row: **Annotate**, **Placement**, **Rearrange**.
5. With **Annotate** active, long-press (≈500 ms) on the translucent full-screen
   catcher in the main canvas (e.g. at `clientX: 500, clientY: 140`).
6. Confirm a small colored dot marker appears at the press point and the sheet
   status shows `1 annotation` with the captured component name (e.g.
   `StorybookApp`).
7. Select the annotation in the sheet (tap its row if listed, or the dot marker)
   and type a note into the **Note** input. In Playwright this may not propagate
   into the RN TextInput state; verify the note on a real device/simulator tap.
   If using Playwright, skip the note assertion.
8. Tap **Copy**; confirm the clipboard contains an upstream-style markdown
   report with the component name, source, and bounds (and the note, when entered
   on device).
9. Tap **Clear** and confirm the marker and count reset to zero.

Expected clipboard excerpt:

```markdown
## 1. AchievementBadge

- **Source file:** `design-system/storybook.tsx`
- **Component chain:** AchievementBadge › StorybookApp › StorybookRoute › ...
- **Platform:** web
- **Bounds:** x=362 y=124 w=660 h=19
```

On device, the note also appears:

```markdown
- **Note:** Achievement badge needs larger icon
```

## Test 2 — Place and manipulate wireframes

1. With the sheet open, tap **Placement**.
2. Confirm a horizontal component palette appears with **Button**, **Card**,
   **Text**.
3. Tap **Card** in the palette (use a real pointer click; simple DOM clicks on
   the text node may not register inside the horizontal scroll view).
4. Tap (short press) the catcher at an empty canvas coordinate above the sheet
   peek. Placement / rearrange use a short tap; annotate uses a long-press.
5. Confirm a wireframe box labeled `Card · 1` appears and the sheet status shows
   `1 annotation · next: Card`.
6. Tap **Button** in the palette, then tap the catcher at a different
   coordinate.
7. Confirm a second wireframe box labeled `Button · 2` appears and the count
   updates to `2 annotations · next: Button`.
8. Tap the first wireframe box (`Card · 1`) to select it; confirm the border
   turns solid and a resize handle appears at the bottom-right corner.
9. Drag the wireframe body to a new position; confirm the box moves and the
   sheet still shows the selected component.
10. Drag the resize handle down and to the right; confirm the **Card** wireframe
    grows.
11. Tap **Copy** and confirm the clipboard contains the wireframe markdown
    report with component types and relative positions.

**Note:** the palette currently surfaces only standalone DS primitives that can
render without children (`Button`, `Card`, `Text`). Compound components
(`Tabs`, `TextField`, `Sheet`) are wired through the same registry but need
child-content wrappers before they can appear here.

## Test 3 — Rearrange mode

1. Tap **Clear** to remove existing annotations.
2. Tap **Rearrange**.
3. Tap **Text** in the palette and tap the catcher on the canvas.
4. Confirm a wireframe box labeled `Text · 1` appears and the sheet status shows
   `1 annotation · next: Text`.
5. Drag the wireframe to a new position and resize it with the corner handle.
6. Tap **Copy** and confirm the clipboard contains a wireframe markdown report
   whose header reads `> 1 component placed`.

## Stage 4 — MCP integration

1. Verify `mcps.json` contains an `agentation` server entry.
2. In the host agent config, confirm `.mcp.json` / `opencode.json` expose the
   `agentation` MCP server.
3. Start the server with the registered command (`npx agentation-mcp@1.2.0 server`).
4. Pass the server's HTTP endpoint via env:
   `EXPO_PUBLIC_AGENTATION_ENDPOINT=http://localhost:4747 wf test prepare --web --force` (the export bakes it in).
5. Capture an annotation; confirm the host agent can call
   `agentation_get_session` / `agentation_get_pending` and see the annotation.

## Known limitations

- Web tap-to-capture is handled by a full-screen catcher overlay. Direct
  Playwright clicks on underlying storybook elements will hit the catcher, not
  the element, which is the intended behavior while Agentation is active.
- Component type chips inside the horizontal `BottomSheetScrollView` may not
  respond to simple `HTMLElement.click()` in Playwright; use proper pointer
  events or manual taps on device.
- **Copy** currently produces either an annotation markdown report (when no
  wireframes exist) or a wireframe markdown report (when any wireframe exists).
  A single report that mixes both annotation notes and wireframe placements is
  not yet implemented.
- The **Note** `TextInput` may not receive Playwright-driven `input` events in
  the web build; notes work on native device/simulator taps.
- The app-level `typecheck` currently hits a pre-existing TS stack-overflow
  unrelated to these changes; the `rn-agentation` package typecheck passes.
- Drag / resize gestures rely on `react-native-gesture-handler` and are best
  verified on a device or simulator; on web they require pointer events to be
  synthesized by RNGH web.
