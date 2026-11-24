---
status: open
area: leetcode-editor
paths:
  - "e2e/editor-language.lua"
  - "e2e/editor-completion.lua"
  - "e2e/editor-typing.lua"
opened: 2026-11-21
last_verified: 2026-11-21
links: []
---
# The language switch and the suggestions have no Android e2e unit

## What is broken

`e2e/editor-language.lua` (switch to JavaScript, type, the Comment key writes `//`) and `e2e/editor-completion.lua` (the list, a tap, the Indent key) run in the browser only. Android runs `editor-typing` with Python and suggestions off. The emulator runs only in CI, its typing unit fails now and then on text that did not read back (it failed on a head with none of this work in it), and a unit that cannot be tried by hand is hard to land without making that worse.

## Why blocked

Not blocked: it needs the emulator's typing unit to be steady first, or a point tap computed from the browser run for the picker sheet and the list rows (the accessibility tree misses some WebView content).

## What unblocks it

Add `problem=two-sum lang=javascript` to the `units:` header of `editor-typing.lua` and an `editor-completion` Android path, one unit at a time, reading the hosted Android run after each.
