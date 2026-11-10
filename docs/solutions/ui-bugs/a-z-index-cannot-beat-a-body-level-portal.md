---
title: "A z-index set inside a route cannot outrank a body-level portal, so the keyboard toolbar painted over an open sheet and swallowed its taps"
date: 2026-08-29
category: ui-bugs
module: frontend/libraries/tamagui-design-system
problem_type: ui_bug
component: frontend
severity: medium
symptoms:
  - "A tap aimed at a control inside an open bottom sheet lands on a keyboard-toolbar key instead"
  - "A browser check fails with `something else is on top at the element centre: DIV EditorSyntax-Paren`"
  - "Raising the sheet's `containerStyle` z-index above the toolbar's own band changes nothing"
  - "On a device, the soft keyboard itself is drawn on top of the sheet, covering the control the sheet exists to offer"
root_cause: "The toolbar rail is a body-level portal at `position: fixed`; the sheet deliberately does not portal, so it is confined to its route's stacking context and no z-index can lift it out."
resolution_type: code_fix
---

## Problem

The editor's keyboard toolbar rail painted over an open bottom sheet and ate the
taps meant for the sheet's controls.

## Symptoms

```
FAIL: tapInApp([data-testid="ProblemEditorLanguagePicker-JavaScript"]):
      something else is on top at the element centre: DIV EditorSyntax-Paren
```

On a device the same geometry was worse: the real soft keyboard drew on top of
the sheet, covering the language dial entirely.

## What didn't work

**Raising the sheet's z-index.** The first fix set `containerStyle` to the
toolbar's band plus one:

```tsx
containerStyle={
  { zIndex: KEYBOARD_TOOLBAR_Z + 1 }   // did nothing
}
```

The check failed again, identically. The reason is structural, not a tuning
problem:

> **The rail is a portal rendered at the document body. The sheet is rendered
> inside its route, deliberately — a portalled sheet under a
> `presentation: 'transparentModal'` route renders correctly and then swallows
> every touch, which is the bug this design already exists to avoid.**

A z-index only orders siblings within a stacking context. A portal that escaped
the route's context is not a sibling, so no value assigned from inside the route
can outrank it. This attempt was removed rather than left in place — a fix that
provably does nothing is worse than none, because the next reader assumes it
works.

**Hiding the rail alone.** That removed the *accessory bar* but not the keyboard:
on the device the keyboard still drew over the sheet.

## Solution

Two mechanisms, because there are two things on top.

**A shared open-sheet count the rail reads.** `frontend/libraries/tamagui-design-system/lib/openSheetStore.ts`:

```ts
export const useOpenSheetStore = create<OpenSheetState>()((set) => ({
  count: 0,
  open: () => set((s) => ({ count: s.count + 1 })),
  close: () => set((s) => ({ count: Math.max(0, s.count - 1) })),
}));
```

The sheet registers while open; the rail gates on it:

```tsx
const sheetOpen = useOpenSheetStore((state) => state.count > 0);
...
{keyboard.visible && !sheetOpen ? <KeyboardToolbarPortal>…</KeyboardToolbarPortal> : null}
```

**Dismissing the keyboard itself**, which takes two calls:

```tsx
useEffect(() => {
  if (!open) return;
  Keyboard.dismiss();
  if (Platform.OS !== 'web' || typeof document === 'undefined') return;
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}, [open]);
```

`Keyboard.dismiss()` resigns the first responder — what closes a real soft
keyboard, including one raised by the editor's WebView. On web it only blurs
React Native's own tracked `TextInput`, so a focused CodeMirror iframe keeps
focus and the keyboard state stays "up"; hence the explicit blur, which also
makes the behaviour assertable in a browser.

## Why this works

Ordering is settled where both participants live. Since the rail escapes to the
body and the sheet must not, the only shared ground is application state: the
rail asks "is a sheet up?" and removes itself. That is also correct product
behaviour — a modal sheet owns the screen, and its keyboard belongs to whatever
is behind it.

## Prevention

- **Before reaching for z-index, establish that both elements are in the same
  stacking context.** If one is portalled to the body and the other is not, the
  question is not "what number" but "which mechanism".
- **A component that escapes to a portal needs a way to be told to stand down.**
  A small shared store beats a growing z-index table, which encodes an ordering
  that only holds if nothing else portals.
- **When a fix does not work, delete it.** Leaving the z-index in alongside the
  real fix would have left the next reader believing stacking order was the
  mechanism.
- **Assert the specific condition, not a proxy a second mechanism also
  satisfies.** The browser check for this asserts `editorFocused === false`, not
  just that the rail is gone — because `openSheetStore` already removes the rail,
  so a rail-only assertion would pass whether or not the keyboard was dismissed.
  Proven by reverting the fix:

  ```
  ok   the keyboard is up before the sheet opens
  FAIL opening a sheet puts the keyboard away — editorFocused=true rail=false
  ```

  `rail=false` in a *failing* run is the whole argument for that discipline.

## Related

- `docs/solutions/ui-bugs/overlays-that-are-green-in-a-browser-and-invisible-on-a-dark-device.md`
  — four other overlay defects from the same session, all colour rather than
  stacking.
