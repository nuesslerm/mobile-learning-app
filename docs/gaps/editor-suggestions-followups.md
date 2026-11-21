---
status: open
area: leetcode-editor
paths:
  - "frontend/mla-tamagui/features/leetcode/data/editor/cm6/completionSources.ts"
  - "frontend/mla-tamagui/features/leetcode/data/editor/cm6/completionExtension.ts"
  - "frontend/mla-tamagui/features/leetcode/state/autocompleteSettingStore.ts"
opened: 2026-11-21
last_verified: 2026-11-21
links: []
---
# The editor's suggestions are off by default and offer names only

## What is broken

Suggestions (issue 86, section D) offer the problem's own names, the names in scope (Python), the language's keywords and the Python builtins, and they are off until the person turns them on in the list's Suggestions row. What is left out on purpose or not yet built:

- Member names after a dot are the ones already in the buffer; the common ones of a type (`append`, `pop`, `get`, `items`, `add`, `join`, `split`, `popleft`) are not offered.
- A builtin function does not insert its parentheses.
- JavaScript and C++ get keywords and the problem's names, no builtins or standard library names.
- Snippets are not offered: they need a Tab that moves between fields, which a phone has no key for.
- The list is a popup beside the caret line; a strip on the toolbar rail that shows the top suggestions was not built.
- The default is off. It flips to on once a person has used suggestions on a phone and Android has run editor-typing with it on.

## Why blocked

Not blocked: each is its own slice.

## What unblocks it

A person using suggestions on a phone (Return and the Indent key above all), then one-line flips and slices in the order above. The list's placement (above the caret when the box has room, below when the caret is near the top of it) is covered by the browser run `e2e/editor-completion.lua`; Android has no unit for it yet (see editor-android-language-and-suggestions-e2e).
