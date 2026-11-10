---
title: "Seven green browser suites shipped five broken overlays, four of them the same shape: a component that never sets its own colour looks fine on a light page and is invisible on a dark theme"
date: 2026-08-29
category: ui-bugs
module: frontend/libraries/tamagui-design-system
problem_type: ui_bug
component: frontend
severity: high
symptoms:
  - "A bottom sheet reads as UI 'hanging in the middle of the screen' — only the drag handle and one hairline divider are visible, over what looks like empty background"
  - "A wheel picker's rows are present in the DOM, hit-testable, and readable by a test — and invisible to a human"
  - "A dialog appears as bare text floating over the screen behind it, with nothing dimmed"
  - "A header overflow menu renders off the top of the screen, only its last row showing under the status bar"
  - "Every browser check passes throughout"
root_cause: "Components that omit a colour inherit a default chosen for a light surface (React Native's black text, or the page's own `background` token), and collision middleware measures against a native container that is not the screen."
resolution_type: code_fix
---

## What happened

App #2 had seven passing browser suites and a design system that looked correct
in every screenshot the harness produced. On a real device, five overlays were
broken — four of them the same bug in different clothes.

## Symptoms

Each one reads as a *layout* failure but is actually a *colour* failure, which
is why none were diagnosed from the description:

| Reported as | Actually |
|---|---|
| "the bottom sheet is hanging in the middle of the screen" | the sheet had no visible surface, so only its handle and divider read as a sheet |
| "the language selector is not even showing up" | rows rendering in black on a black sheet |
| "the dialog you can't even tell apart from the background" | card background and backdrop dim both discarded |
| "the popper is out of bounds" | `flip()` measuring against the wrong container |

## What didn't work

**Raising the sheet's z-index above the keyboard toolbar.** Reasonable, and
provably useless: the rail is a body-level portal at `position: fixed` and the
sheet deliberately does not portal, so nothing set from inside a route can
outrank it. See the companion note on portals and stacking context.

**Trusting a browser check to catch any of it.** These are not flaky-in-the-browser
bugs; they are *absent* in the browser. Three of the four colour failures depend
on the dark theme being active and on React Native's platform defaults, and the
`flip()` failure depends on a native-stack header view with no web analogue —
on web the collision boundary is the window, so the menu never flips.

## Solution

**Sheet — paint an actual surface.** `backgroundStyle` used `theme.background`,
which on the dark theme is the same black as the page:

```tsx
// color2, NOT `background`: a sheet painted in the page's own background is
// invisible on the dark theme.
backgroundStyle={
  { backgroundColor: theme.color2?.val ?? '#fff' }
}
```

**Wheel — supply the colours the library does not.**
`@quidone/react-native-wheel-picker` ships no colours, so its rows fall back to
React Native's default black text. Themed the item text and gave the selection a
visible band.

**Dialog — stop discarding the caller's style.** The headless-factory slot
component destructured `style` out to keep gluestack's internal layout props off
the DOM, then never passed it on — so `styles.card` (the surface) and
`styles.backdrop` (the dim) were both thrown away:

```tsx
// before: <View {...rest} style={baseStyle}>          — caller's style dropped
// after:  <View {...rest} style={[baseStyle, style]}> — merged
```

The card's `backgroundColor: 'white'` also went, in favour of the theme.

**Menu — stop flipping.** floating-ui's collision boundary on native is the
reference's offsetParent. The "…" trigger lives inside the native-stack header's
own short view, so "room below" measures as roughly 40pt, `flip()` fires, and the
card renders *upward* off the top of the screen:

```tsx
// No `allowFlip`. A header overflow button is always at the top with the whole
// screen beneath it, so there is nothing for flip to decide.
<Popover open={open} onOpenChange={setOpen} placement="bottom-end">
```

## Why this works

The unifying rule: **a component that does not set a colour is not neutral — it
has picked one, and it picked it for a light surface.** React Native's default
`Text` colour is black. A design token named `background` is the page's
background, not an elevated one. A third-party control that "inherits" on web
inherits nothing under React Native. Every one of these renders correctly on a
white page and disappears on a dark one.

The menu is a different mechanism with the same lesson about platform defaults:
middleware that measures "available space" measures it against whatever the host
calls the container, and on native that is not the screen.

## Prevention

- **Never let a third-party control inherit its text colour.** Pass an explicit
  themed colour. If a fallback is needed, make it the *dark-theme* value — a
  `?? '#ffffff'` fallback on a light sheet reintroduces exactly this bug the day
  the theme fails to resolve.
- **Never paint an elevated surface with the page's own background token.** Use
  the elevated step (`color2` here). If a component is only distinguishable by
  its border, it has no surface.
- **A slot/wrapper component must merge the caller's `style`, never replace it.**
  Dropping props to keep them off the DOM is fine; dropping `style` silently
  removes the caller's entire visual contract.
- **Disable collision middleware whose boundary you cannot verify.** When the
  geometry is known statically — a header button always has the screen below it —
  the middleware can only be wrong.
- **Treat "it looks like a layout bug" as a colour hypothesis too.** Every one of
  these was reported as position or absence. Reading the computed colour against
  the nearest opaque ancestor settles it in one query, and a browser check can
  make that assertion even when it cannot see the theme.

## Related

- `docs/solutions/ui-bugs/a-z-index-cannot-beat-a-body-level-portal.md` — the
  fifth defect from the same session, a genuinely different mechanism.
- `.agents/skills/browser-exploration/SKILL.md` — the method that surfaced six
  further defects of this class after these five were fixed, including a wheel
  row at ~1.5:1 contrast and a disabled button with no disabled styling.
