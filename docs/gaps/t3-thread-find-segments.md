---
status: open
area: rca-parity
paths:
  - "clis/t3-serve/core/src/usecases/desk_threads.rs"
  - "clis/t3-serve/tests/flow/desktop_threads.rs"
opened: 2026-11-20
last_verified: 2026-11-20
links:
  - "docs/plans/rca-offer-more/plan.md"
  - "docs/plans/rca-parity/plan.md"
---
# The in-thread find counts raw text, and only its empty answer was recorded

## What is broken

`orchestration.searchThread` and `orchestration.searchThreadStream` answer from t3's own cache of the thread's log. The only recorded success of the pinned server
(`t3-v0.1.9-scenarios`, `t3-scn-rpc-orchestration.searchThread*`) is the thread with no message: `{activeIndex: 0, match: null, navigation: [], snapshotSequence, totalMatches: 0}`.
A result with matches is authored from the contract and `ThreadFind.ts` (entry id, the occurrence walk by `index`, `start` and `offset`, a navigation of at most 17 entries), not recorded.

The pinned server counts occurrences in the rendered text segments of a message (markdown rendered, inline skills and context records left out); this server counts them in the raw
`text` of user and assistant messages and the `markdown` of proposed plans, with the same case fold. A word that sits only inside markup (a link target, a code fence language) is
counted here and not there.

## Why blocked

No recorded scenario holds a thread with messages, and the segment rules live in the pinned client packages (`threadFindText.ts`), not in the executor.

## What unblocks it

Record one scenario of the pinned server with a thread that holds a message and a search for a word in it (a fixture the scenarios cassette generates), replay it, and port
`searchableMessageSegments` if the counts differ.
