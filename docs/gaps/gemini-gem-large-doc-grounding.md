---
status: blocked
area: gemini
paths:
  - "clis/knowledge/src/knowledge_gemini.rs"
opened: 2026-10-04
last_verified: 2026-10-26
links: []
---
# Gemini Gem does not ground on the full content of a large attached Doc

## What is broken

Asking the Gem about content late in a packed context doc (e.g. the Gemini doc-regeneration source
at ~387K chars of `clis-1`'s 663,084) answers "not findable in the attached docs".

## Why blocked

The Doc is not truncated: Docs API `documents.get` returns all 663,084 chars and both markers, and
the sync's single `insertText` (now `sync_doc` in `knowledge_gemini.rs`) lands at the right `endIndex`.
Gems ground large Docs through per-query chunk retrieval, not the whole document; the Gem itself
reported 0 chars accessible for a verbatim-quote query. This is a platform limit, not a defect here.

## What unblocks it

Untested mitigation: lower `--target-chars` so each doc is small enough to be read whole, re-test
against the Gem, and weigh it against the 10-doc cap. Otherwise live with it; do not re-diagnose.

Mitigation under test: the context is now one file whose first section is `ARCHITECTURE.md` (layering rules, package map, key flows), small and dense so a retrieval of any chunk can find the global rules (`clis/knowledge/src/knowledge_gemini.rs`). Whether the Gem grounds on late files of that one bundle is not yet measured.
