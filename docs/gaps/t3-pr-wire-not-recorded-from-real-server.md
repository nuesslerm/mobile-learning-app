---
status: open
area: rca-parity
paths:
  - "clis/t3-serve/core/src/usecases/pr.rs"
  - "clis/t3-serve/core/src/usecases/sourcecontrol.rs"
opened: 2026-11-04
last_verified: 2026-11-04
links:
  - "docs/plans/rca-parity/plan.md"
  - "docs/gaps/exec-hosted-fixtures-not-from-real-clis.md"
---
# The pull request, source control and clone answers are built from T3's schemas, not replayed from a recorded T3 server

## What is broken

Every answer of the `pullRequests.*`, `sourceControl.*`, `projectClone.*` and `subscribeProjectClones` rows is built from the Effect schemas of the pinned
T3 contracts (`packages/contracts/src/pullRequest.ts`, `sourceControl.ts`, `projectClone.ts`), and the chain tests check them against the executor's
neutral answer (and so against fixtures written from the CLIs' documentation, see `exec-hosted-fixtures-not-from-real-clis`). No session of a real T3
server with a GitHub account was recorded for these rows, so three things are unverified against the phone: how `Schema.Option` fields
(`SourceControlProviderAuth.account`, `.host`, `.detail`, `SourceControlDiscoveryItem.version`) are encoded (the mapper sends `{"_tag":"Some","value":x}` and
`{"_tag":"None"}`), which optional fields of `PullRequestDetail` and `PullRequestListEntry` the phone needs beyond the schema's required ones, and the exact
shape of the `subscribeProjectClones` stream (the mapper sends the whole list of snapshots each time one changes).

## Why blocked

Recording a T3 server's answers needs a GitHub account and a repository with pull requests, and a pinned T3 server run against them.

## What unblocks it

A run of the pinned T3 server (`wf mimic record t3-v0.1.6 --live`, the T3 driver of `tooling/mimic-drivers`) against one test repository, recorded into
`clis/t3-serve/cassettes` and replayed by `clis/t3-serve/tests/flow/mimic_t3.rs` with an allowlist for what differs by design. Until then the rows are served by the chain (the executor ran the action and the answer is
the mapping of what it said), and the phone's reading of the answers is untested.

## Not built, and why

- `pullRequests.updateComment`, `diffFileContents`, `reviewGuide`, `generateReviewGuide` and `git.preparePullRequestThread` have no executor action behind them
  (edit a comment, fetch the pull request head, a model turn over the diff); they stay pending with a reverify note in `t3-decisions.json`.
- `pullRequests.invalidate`, `reportState`, `stack` and `subscribeRefreshes` answer (success, success, null, a counter that does not move) so the phone works,
  but t3 serve keeps no cache and has no refresh source, so they stay pending with a reverify note.
- `submitReview` refuses inline comments, `setReaction` refuses taking a reaction back and `runAction` refuses close, reopen, revert, delete-branch,
  disable-auto-merge and approve-workflows: the executor's closed set has no such verb.
- A `sourceControl.signInWait` holds the phone's request socket for up to 60 s and then answers `expired`; the phone asks again (the flow stays open on the executor).
