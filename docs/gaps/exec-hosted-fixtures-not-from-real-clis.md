---
status: open
area: rca-parity
paths:
  - "libraries/agent-api/exec-hosted/tests/cassettes/exec-hosted/**"
  - "clis/rca-exec/src/hosted/**"
opened: 2026-11-04
last_verified: 2026-11-04
links:
  - "docs/plans/rca-parity/plan.md"
---
# The hosted fixtures and the GitLab argv are written from documentation, not recorded from gh and glab

## What is broken

`libraries/agent-api/exec-hosted/tests/cassettes/exec-hosted` (a mimic cassette set, `provenance.kind: imported` from the former `hosted_*.json` files) holds what `gh` and `glab` print for each argv `rca-exec` starts, written from the CLIs'
documented JSON and text output, and the `rca-exec` flow tests replay them through the `fake-hosted` double of exec-testkit. No interaction is the
output of a run of the real CLI at a pinned version. Three things in particular are unverified against a real account: the `glab` flags the
executor passes (`mr update --reviewer=+a,-b`, `mr create --yes`, `repo create <group>/<name>`, `auth login --web`, `api --input=-`), the text
`gh auth status` and `glab auth status` print (read by `parse_auth_status`), and the web sign-in prompt (`parse_login_prompt`; GitLab's
sign-in shows an address and no one-time code).

## Why blocked

Recording needs a real GitHub and GitLab account and a test repository, and a development session would borrow a person's ambient sign-in. The
deployed check (`backend/remote-chat-agent/rs/tests/deployed_verification.rs`, run by `test:verify:deployed` with `WF_DEPLOYED=1` and `WF_HOSTED_TEST_REPO`) is read-only and runs only where
a test repository and a signed-in CLI exist.

## What unblocks it

An owner-gated run on the VM (docs/plans/mimic/plan.md, D5), against one test repository on each provider and the CLIs' own sign-in, that
records each action's output into the set with `provenance.kind: recorded` and the CLI versions stamped (the CLI pin file (D5)). Every row
of the coverage tables whose chain test depends on these cassettes keeps a reverify note until then.
