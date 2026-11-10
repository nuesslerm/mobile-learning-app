---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/rs/src/api/routes/edit_stats.rs"
  - "backend/remote-chat-agent/service-core/src/application/edit_stats.rs"
opened: 2026-09-15
last_verified: 2026-10-20
links:
  - archive/solutions/integration-issues/a-success-signal-that-cannot-fail-hides-a-broken-deploy.md
---
# `POST /api/edit-stats` answers `stored: true` for a row the table then rejects

## What is broken

The route validates, hands the row to a fire-and-forget writer and answers `202 {"ok":true,
"stored":true}`; `stored` means "a writer is configured". When `SOURCE` was `VARCHAR2(8)` and
`CHUNK_INDEX` did not exist, every insert failed at ORDS and nothing in the response, client or the
since-removed wf command `edit-stats` said so (fixed for that case by the since-removed wf command `infra oracle db add-plan-columns`).

## Why blocked

Not blocked; the reporting gap remains: a write failure is visible only in the agent's own logs.

## What unblocks it

Add a write-error counter or `GET /api/edit-stats/health` reporting the last insert error, so a gate can
tell "no rows because nothing ran" from "every insert is failing".

## Re-checked 2026-10-20

Re-checked against dev at 95e494827. POST /api/edit-stats still answers 202 with stored set to enabled() (whether a table is configured) before any insert is attempted (routes/edit_stats.rs:30, application/edit_stats.rs:217). Write failures are logged at most once a minute as edit_stats.flush_failed (application/edit_stats.rs:297-300). No counter or health route exposes them, so a gate still cannot tell 'nothing ran' from 'every insert fails'. The Rust flow test voice.rs:74-78 asserts the misleading stored: true.
