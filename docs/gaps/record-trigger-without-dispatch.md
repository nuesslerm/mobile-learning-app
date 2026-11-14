---
status: open
area: ci
paths:
  - ".github/workflows/mimic-record.yml"
  - "clis/mimic/src/marker.rs"
  - "clis/mimic/src/job_verb.rs"
opened: 2026-11-07
last_verified: 2026-11-07
links: []
---
# The record-references trigger is a commit marker, not a dispatch, and has not run live

## Why not `workflow_dispatch`

Agent sessions in this repo reach GitHub through an egress proxy that answers 403 to `workflow_dispatch` and to the Actions API (listing runs, downloading an artifact), so a dispatched
capture could be started and read only by a human. The same constraint shaped the Gem sync: an EMPTY commit to `dev` whose message carries a marker is a plain `git push`, which the proxy
allows, and the CI pipeline ignores it (`ci-hosted.yml` starts an empty commit only with a `[services:...]` or `[cache:...]` tag). `mimic-record.yml` follows that pattern (`[record <set>...]`, `[replay <set>...]`, parsed by `wf mimic trigger`).
For the same reason the cassette set also travels through the verification-artifacts bucket (`wf mimic publish` / `fetch`, plain HTTPS), with the Actions artifact as the human-facing copy.

## What has not been seen

- A live run: no marker commit has been pushed. Unseen: the gate letting a `[record ...]` commit through and the CI pipeline starting nothing, the matrix running legs one at a time, the
  checkpoint artifact round trip (`mimic-checkpoint-<set>`, found with the artifacts API and restored with `actions/download-artifact` across runs), `nice -n 19` through `wf`, and a real
  provision of the Orca source and the pinned T3 release on the hosted runner (cold install and build time, disk under the work root `~/.cache/mimic-work`).
- `wf mimic record --resume --max-minutes <n>` exists (it stops at a batch boundary after n minutes, a decimal is fine, with its state under `<set>/raw/checkpoint`, and prints `complete:false`; one flow test through the `wf-mimic` binary against the toy reference covers the stop, the resume to the uninterrupted set and the refusal of a changed corpus or rules), but no real T3 or Orca record has run it: the batch size of a real reference against `--max-minutes` is unmeasured.
- The checkpoint artifact holds the unmasked raw capture for 3 days (a private Actions artifact, readable by anyone with read access to the repository). It is uploaded only after a clean out-of-budget exit and only when a scan for Bearer-token and JWT shapes finds nothing, which is a heuristic: other secrets in raw captures are not detected. Moving it to a private bucket path needs a private write path in `wf-bucket`.
- Toolchain steps other than node (`actions/setup-python`, `actions/setup-go`, `rustup`) and the checkpoint deletion (`actions: write`) are checked by actionlint and the matrix-key contract test only.
- Provisioning timeouts (`timeout_minutes`) are not covered by a test: the smallest value is a minute.
- The `t3-scenarios` set (needs the scripted CLI built, `T3_CLI_DIR`) declares no `[provision]` yet.
- Test data from the first development runs of `wf mimic publish` reached the real bucket before the flow-test helper was made hermetic: `verification/record-references/42/`, `.../7/` and
  `.../latest/toy-set.json` hold a masked toy set and can be deleted.

## What unblocks it

Land the checkpointed `wf mimic record`, push `[record t3-recorded-1]`, read the run, then `wf mimic fetch latest --set t3-recorded-1` from an agent session; push `[replay t3-recorded-1]` for the diff table.
