---
status: open
area: rca-parity
paths:
  - "libraries/agent-api/exec-testkit/cassettes/exec-recordings/**"
opened: 2026-11-04
last_verified: 2026-11-04
links:
  - "docs/plans/rca-parity/plan.md"
---
# The scripted CLI replays cassettes that were written, not recorded

## What is broken

The recordings are the interactions of the mimic cassette set `libraries/agent-api/exec-testkit/cassettes/exec-recordings`: the stream-json of
Claude Code, Codex and opencode as their documentation describes it and as `rca-cli-driver`'s `translate()` reads it. None is the output of a
run of the real CLI at a pinned version, so a change in what a CLI prints that the documentation does not show would pass every test that
replays them. The set's header says so in `provenance.kind` (`imported`: converted by `wf mimic import` from the former `recordings.json`
and `recordings/*.jsonl`, each legacy file's hash in `provenance.imported_from`); a set from a real run says `recorded`.

## Why blocked

Recording a turn means running the real CLI, and a run from a development session would use the host's own ambient sign-in to the provider.
The plan for M3 asks for real recordings at pinned versions; that run belongs where the CLI has a credential provisioned for it, not where it
would borrow a person's.

## What unblocks it

A recording run on the VM against each provider's own credential directory (`/data/providers/<provider>`, plan section 2, "Per-provider
credential isolation"), owner-gated (docs/plans/mimic/plan.md, D5): `wf mimic record` writes the set with `provenance.kind: recorded`, the CLI
version stamped and pinned in the CLI pin file (D5). The `rca-cli-driver` test that translates every recording and the exec-testkit flow
tests that read the set then check it with no change.
