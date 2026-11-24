---
status: open
area: browser-e2e
paths:
  - "e2e/run-error-line.lua"
  - "frontend/mla-tamagui/features/leetcode/ui/editor/EditorInput.tsx"
opened: 2026-11-21
last_verified: 2026-11-21
links: []
---
# A browser script cannot read a whole long buffer back from the web editor

## What is broken

On the web the editor's DOM holds only the lines near the viewport (about 30 of 49 lines at the start of a long problem), so `device:get_element({id='CodeEditorInput'}).text` is a window onto the buffer, not the buffer. A script that types a long solution and reads it back to prove it landed sees a prefix, and a script written against a short problem passes while the same check on a long one cannot.

## Why blocked

Not blocked. It is how CodeMirror virtualizes its lines; `e2e/run-error-line.lua` works around it by proving the marked line and not the whole text.

## What unblocks it

Read the buffer through the typed editor API (`read` over the loopback the page exposes as `window.__editorEngine__`) in a browser script, or give the harness a `get-editor-text` read that asks the engine instead of the DOM.
