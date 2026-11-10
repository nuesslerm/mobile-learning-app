---
title: Keyboard and keyboard-toolbar responsiveness
verifies: The device-frame on-screen keyboard and the app's keyboard toolbar respond correctly via wf browser
tier: agent-driven
maxAgeDays: 60
---

# Runbook: keyboard + keyboard-toolbar responsiveness (wf browser)

Agentic procedure to verify the device-frame on-screen keyboard and the app's
`KeyboardToolbar` respond correctly when driving the app through the
`wf browser` CLI. No physical device, no real soft keyboard.

## What this exercises

- **Focus → keyboard**: focusing any editable element (a `TextInput` **or** the
  sandboxed CodeMirror editor iframe) raises the on-screen keyboard.
- **Push-up**: the app iframe shrinks so content reflows above the keyboard.
- **KeyboardToolbar**: the app's helper rails mount and dock above the
  keyboard — driven by the daemon injecting `window.__MCP_KEYBOARD_VISIBLE__`
  into the app frame (the app can't see focus inside the sandboxed editor).
- **Typing**: on-screen keys (incl. shift / numbers / symbols) reach the
  focused element.
- **Blur → hide**: keyboard + toolbar retract.

## Preconditions

```bash
wf test prepare --web    # the web export of this tree; serve frontend/mla-tamagui/dist under /mla-tamagui on 127.0.0.1:8800
```

The serve step is the Setup of `docs/runbooks/device-lab-codemirror-typing.md`. After switching branches,
`wf test prepare --web` again: it rebuilds when the tree's inputs changed.

## Steps

The keyboard moves only by user action: tap the editor to raise it, tap Done to lower it.

```bash
mkdir -p .agents/tmp/kb
wf browser start-device --device kb
wf browser open-link http://127.0.0.1:8800/mla-tamagui/leetcode/two-sum
wf browser assert-visible --id CodeEditorInput --timeout 30000
wf browser take-screenshot .agents/tmp/kb/1-idle.png                  # (1) idle — no keyboard

# tap the editor -> keyboard + toolbar should appear, content pushed up
wf browser tap-on --id CodeEditorInput
wf browser extended-wait-until --condition "device:get_device_state('keyboard').visible" --timeout 2000
wf browser take-screenshot .agents/tmp/kb/2-keyboard.png              # (2) keyboard + toolbar rails

# type through the preset's keyboard, key by key at the caret
wf browser input-text s
wf browser get-element --id CodeEditorInput                           # its text (the buffer) holds the typed char
wf browser take-screenshot .agents/tmp/kb/3-typed.png                 # (3) typed

# tap Done -> keyboard + toolbar retract
wf browser scroll-until-visible --element-id EditorAction-Done --from-id EditorActionToolbar --direction RIGHT
wf browser tap-on --id EditorAction-Done
wf browser extended-wait-until --condition "not device:get_device_state('keyboard').visible" --timeout 2000
wf browser take-screenshot .agents/tmp/kb/4-idle.png                  # (4) idle again
```

## Expected results

| Step      | Expected                                                                                        |
| --------- | ----------------------------------------------------------------------------------------------- |
| (1) idle  | Editor visible, no keyboard, no toolbar rails                                                   |
| (2) tap   | Mock keyboard up; `KeyboardToolbar` helper rails docked above it; editor + action row pushed up |
| (3) type  | Typed characters appear in the editor's buffer                                                  |
| (4) Done  | `get-device-state keyboard` is `{visible: false, bounds: null}`; keyboard + toolbar gone; layout restored |

## Notes / gotchas

- **Screenshots are of the screen as the device shows it**, the device frame's status bar, keyboard and home
  indicator included (`wf browser take-screenshot <path>`).
- **The keyboard is auto-driven by focus** in the default `mock` mode; there is
  no verb to raise or lower it. A tap outside an input keeps it up, as on Android; dismiss it with the toolbar's Done.
- **Keys that are not letters** (Enter, Backspace, Tab, the arrows by Maestro's names `"Remote Dpad Left"` and
  so on, Ctrl+End) go through `wf browser press-key`.
- **A stale export after a big dependency/store change**: `wf test prepare --web --force` builds it again.
