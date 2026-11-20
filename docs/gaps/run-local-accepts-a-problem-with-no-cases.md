---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/domain/verdict.ts"
  - "frontend/mla-tamagui/features/leetcode/domain/runParams.ts"
opened: 2026-11-20
last_verified: 2026-11-20
links: []
---
# Run Local shows a green "Accepted" for the eight problems that have no bundled test cases

## What is broken

Eight catalog problems have no test cases (clone-graph, linked-list-cycle, reorder-list, encode-and-decode-strings, serialize-and-deserialize-binary-tree, implement-trie-prefix-tree, design-add-and-search-words-data-structure, find-median-from-data-stream). A run sends no cases, the harness answers ok with no rows, and `runResponseOf` reports Accepted 0 of 0, with the "All tests passed" toast. That is what the backend's own runner answers too: the unit test `given no cases at all ... stays Accepted 0 of 0` pins it so that a local verdict matches a remote one.

## Why blocked

Not blocked. Changing the verdict for zero cases is a decision about parity with the backend, and issue #86 item E4 adds the missing cases, which removes the situation.

## What unblocks it

E4 (every Blind 75 problem has cases). Until then, either keep the parity or show "No test cases" and no success toast for a run with zero cases, in `runResponseOf` and the toast branch of `ProblemEditorScreen`.
