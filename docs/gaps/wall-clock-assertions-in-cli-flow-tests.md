---
status: open
area: ci
paths:
  - "clis/cdp/tests/flow.rs"
  - "clis/blob-cache/tests/nx_server.rs"
opened: 2026-11-21
last_verified: 2026-11-21
links: []
---
# A few CLI flow tests still assert against the wall clock

## What is broken

The owner rule is no flaky tests. None of these failed in the last 40 hosted runs, but they read a real clock and a delayed box can miss them: `clis/cdp/tests/flow.rs` lines 297 and 339 (`returned < 50 ms` for `log_input`, and `unwritten.is_empty()` racing a 300 ms fake capture), line 342 (`(at..at+50).contains(&t)` is an upper bound on a real clock) and line 256 (< 4 s); `clis/blob-cache/tests/nx_server.rs:204` (answered < 1 s with 32 idle sockets). Looser bounds of the same kind sit in `clis/wf-bucket/tests/flow/guard.rs` (388, 507), `metrics.rs:130`, `clis/deploy/tests/flow/ship.rs` (925, 1270), `clis/pipeline/tests/flow/ci_comment.rs:1251`, `clis/wf-login/tests/flow/auth.rs:428`, `clis/wf-harness/tests/flow/android.rs:1191` and `harness.rs` (313, 324), and a group under `clis/rca-exec/tests/flow/`.

## Why blocked

Not blocked. The clis/cdp ones need a gate in the fake browser's capture (hold it on a channel until the test releases it) instead of milliseconds; the loose ones are bounds that only fail on a seconds-long stall.

## What unblocks it

Rewrite the riskiest (`clis/cdp/tests/flow.rs` 297, 339, 342) to wait on a released gate, as `clis/rca-exec/tests/flow.rs` `run_command_streams_stdout_and_stderr` now does, then sweep the looser bounds.
