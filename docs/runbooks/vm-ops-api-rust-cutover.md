---
title: vm-ops-api Rust cutover, the one VM test pass (Rust image by hand, push, health, S2 force dispatch, D2b drills)
verifies: The Rust vm-ops-api serves 8080 at the pushed commit, redeploys apps and itself, and the watchdog brings a stopped or frozen service back with no human. The Nx cache is not served from the VM: it is blob-backed (`docs/plans/blob-cache/plan.md`), so this pass has no cache step.
tier: manual
maxAgeDays: 30
---

# vm-ops-api Rust cutover: the one VM test pass

The VM half of `docs/plans/vm-ops-api-rs/plan.md`, run once by the orchestrator. The Rust service is the only vm-ops-api: there is no Node standby, no second port, no flip and no fallback (owner, 23 Oct 2026). If something breaks, it is fixed, or the whole VM is recreated (Recovery, last section). Stop at the first failed step and run its recovery.

Authority: the owner's written go of 23 Oct 2026, recorded at unit S1 of the plan. The steps marked LIVE change the VM (a push, a CI dispatch, drills that stop the service on purpose). Nothing else may send a request to the live service.

| Step | Unit | Mutates | About |
| --- | --- | --- | --- |
| 1 | preflight | no | 5 min |
| 2 | the Rust image, by hand | LIVE (a build and a push to Docker Hub) | up to 1 h cold |
| 3 | S1, the push (commit A) | LIVE | 15 min, plus the pipeline |
| 4 | health, the first Rust swap | no | 5 min |
| 5 | S2, the force dispatch | LIVE (CI) | 1 to 2 h |
| 6 | D2b, the drills | LIVE | 30 min |
| 7 | the record | no | 10 min |

## Step 1: preflight (read-only)

```sh
cd /home/user/mobile-learning-app
git fetch origin dev && git status -sb            # the commit to push is on top of origin/dev
SHA=$(git rev-parse HEAD); T=sha-$(git rev-parse --short=7 HEAD)
OUT=$(mktemp -d)                                   # tee each step's JSON here: the progress line quotes it
wf vm net up                                       # idempotent; the tailnet is how every wf vm command reaches vm-ops-api-main
wf vm ops health | tee "$OUT/1-health.json" | jq -c '{commit, imageTag}'
```

Expect the old Node service answering on 8080 (`commit` is an older dev commit). If it does not answer: Recovery.

## Step 2: build the Rust image by hand (LIVE)

The push's own swap is rendered by the old Node service, which does not build the Rust image: the new compose file asks for `vm-ops-api-rs:$T`, so that tag must be in the registry before the push. Push the commit to a scratch branch first (a branch push starts no workflow), then build at it on the VM:

```sh
git push origin "$SHA:refs/heads/vm-ops-api-rs-image"
# historical: this cutover built by hand on agent-shell. `wf ship --build-only` is gone; a push to dev builds and pushes the tag in CI (the build-image action)
curl -fsS "https://hub.docker.com/v2/repositories/nuesslerm/vm-ops-api-rs/tags/$T" >/dev/null && echo "image there"
```

If the build fails: read `wf vm exec --log <jobId>`, fix the cause in the tree, and start again from Step 1 with the new commit. Nothing on the VM changed yet. Delete the scratch branch afterwards (`git push origin :refs/heads/vm-ops-api-rs-image`).

## Step 3: push dev, watch CI (LIVE, S1)

```sh
git fetch origin dev && git rebase origin/dev && git push origin HEAD:dev
gh run list --workflow deploy.yml --branch dev --limit 1 --json databaseId,status,conclusion,headSha
gh run watch <id> --exit-status
```

The `deploy (vm-ops-api)` leg calls `POST /redeploy?ref=<sha>` on the old Node service, which builds the Node image and the watchdog once more at the commit and swaps: its new compose file runs the one `ops-api` service on `vm-ops-api-rs:$T`. The leg waits up to 15 minutes for `/health` to show the commit.

If the leg fails: the swap restored the previous files and the old Node service kept serving (F1: `restored: "ok"`). Read why with `wf vm ops logs vm-ops-api --tail 200` and `wf ship --status | jq '.redeploy.history'`; a missing image shows as a failed `compose pull` before any file was swapped. If 8080 does not answer at all: Recovery.

## Step 4: health, the first Rust swap (read-only)

```sh
git fetch origin dev && test "$(git rev-parse origin/dev)" = "$SHA"             # the remote head is the pushed commit
wf vm ops health | tee "$OUT/4-health.json" | jq -e --arg c "$SHA" '.commit == $c'
curl -fsS https://hub.docker.com/v2/repositories/nuesslerm/vm-ops-api-rs/tags/latest >/dev/null
wf vm ops logs vm-ops-api --tail 50 | grep -c vm-ops-api.listening                # the Rust service logged its start
wf ship --status | jq -c '.redeploy.history | map(select(.key == "vm-ops-api")) | max_by(.ts) | {outcome, tag, restored}'
```

Expect `true` for the commit, the Docker Hub call to exit 0 (`latest` moved after the commit served), at least one start line, and the last history line `redeploy.succeeded` with `tag == $T`. `/health` has the pre-roles key set: `{ok, commit, startedAt, host, debugMode, time, serverScriptSha, imageTag}`.

## Step 5: S2, the first pipeline through Rust (LIVE, CI)

A `deploy.yml` dispatch with `force: true`: every service redeploys and the EAS Update republishes, so the vm-ops-api leg is built by `wf ship` (a synchronous step) and then smoke-tested (the Rust service runs the pulled image as a throwaway container, `wf` asks it over the tailnet with a signed `/health`, then stops it) and swapped by the service itself (it renders its own scripts and runs its own swap), and the other legs deploy through it. Dispatch only after the push's own run is finished (one run at a time per ref).

```sh
gh workflow run deploy.yml --ref dev -f force=true
sleep 30; RUN=$(gh run list --workflow deploy.yml --branch dev --event workflow_dispatch --limit 1 --json databaseId -q '.[0].databaseId'); echo "$RUN"
gh run watch "$RUN" --exit-status        # one to two hours: tests, every leg, the e2e jobs
gh run view "$RUN" --json jobs | jq -r '.jobs[] | [.name, .conclusion] | @tsv'
wf ship --status | jq -e --arg t "$T" '.redeploy.history | map(select(.key == "vm-ops-api")) | max_by(.ts) | .outcome == "redeploy.succeeded" and .tag == $t'
```

Expect the run green, `deploy (vm-ops-api)` success and `true` from the last command (S2's done-when). The other `deploy (<service>)` legs are the proof that the Rust service redeploys every other app in one pipeline.

If it fails:
- A red test job deploys nothing, so S2 is not proven by it: read the failing job. A failure unrelated to vm-ops-api (a flaky browser or Android test) is a re-dispatch, not a Rust problem.
- To isolate the Rust self-redeploy from CI, run the leg's own command for a tag CI pushed: `wf ship vm-ops-api --ref "$SHA" --redeploy-only --tag "sha-${SHA::8}" --wait-serving` (`--ref` names a commit; it prints `phase smoke test|tailnet probe|swap|serving` seconds). A failed smoke test, or a throwaway container that does not answer over the tailnet, fails it with that container's log or the probe's error and swaps nothing; a swap that fails is read back from `GET /redeploy/status` (`redeploy.failed`, `restored`).
- `redeploy.failed` for `vm-ops-api` with `restored: "ok"`: the previous files and tag are back and the service kept serving; read `wf vm ops logs vm-ops-api --tail 300`, fix the cause, push again. `restored: "failed"` or 8080 silent: Recovery.

## Step 6: D2b, the drills (LIVE: the service stops itself and the watchdog brings it back)

Each drill sends `POST /drill` (signed), which spawns a detached sibling: `stop` is `docker stop` after 5 s, `freeze` is SIGSTOP after 5 s with a thaw of its own after 600 s. wf asserts `/health` stops answering and then reads who brought it back.

```sh
wf vm ops drill stop --json | tee "$OUT/6-1-drill-stop.json" | jq -e '.recovered == true and .by == "watchdog"'
sleep 660        # the watchdog recreates one container at most every 10 minutes (RECREATE_COOLDOWN_MS in backend/watchdog/core/src/domain/decide.rs)
wf vm ops drill freeze --json | tee "$OUT/6-2-drill-freeze.json" | jq -e '.recovered == true and .by == "watchdog"'
```

Expect, for each: `ok: true`, `container: "vm-ops-api-ops-api-1"`, `down: true` (8080 stopped answering), `recovered: true`, `by: "watchdog"`, an `event` naming the same container, and `commit` equal to `$SHA` in the closing health read. `recoveredS` is minutes: for `stop` the exited rule waits two minutes then the watchdog's sweep acts; for `freeze` the container must first turn unhealthy (about 90 s). Anything under 600 s is the watchdog; the freeze's own thaw lands at 600 s.

If it fails:
- `by: "other"` with `recovered: true`: something other than the watchdog brought it back. Record it as the watchdog rule not firing, read `wf vm ops logs vm-ops-api --tail 400` for `watchdog` lines, and do not close the gap doc.
- `by: "unknown"`: the watchdog's log could not be read (`logsError` says why); judge from the logs by hand.
- `recovered: false`, exit 1: the service did not come back within 10 minutes. There is no standby: Recovery.
- 8080 never went down (`down: false`): the sibling did not run. Check `wf vm ops logs vm-ops-api --tail 200` for the `/drill` event; nothing to recover.

## Step 7: the record (read-only)

```sh
wf vm ops health | jq -e --arg c "$SHA" '.commit == $c'
```

Then record, in this order: one dated line in `docs/plans/after-device-lab/progress.md` quoting `$OUT/*.json` (what ran, what passed, the drills' `recoveredS` and `by`); in `docs/plans/vm-ops-api-rs/plan.md`, remove the `[implemented, not yet verified on the VM]` tag from each unit whose step passed (table below) and keep it on the rest; the drills' numbers in `docs/audits/2026-09-02-token-diet-decision-log.md`; the gap `docs/gaps/vm-has-no-second-door.md` per Step 6. Then push commit B (plan unit S4 with A2, already written in the tree: the Node source, its rules and its tests are deleted, and the integration suite, the dashboard page and the script snapshots now live under `backend/vm-ops-api/`). The Rust service builds the push's images and swaps itself in, so the leg needs nothing by hand; it ships because `backend/vm-ops-api` and `infra/oracle-cloud/vm-ops-api` changed, and Step 4's reads apply to it unchanged.

| Unit | Proven by |
| --- | --- |
| S1, C1b, F1, F3, R1 to R8 (real traffic) | Steps 2 to 4 (the build log, the swap history, the health read) |
| S2 | Step 5 |
| D2a, D2b | Step 6 |

## Recovery: fix it and recreate the VM

There is no standby, no flip and no fallback. In this order:

1. A failed swap restores itself (F1: `restored: "ok"`); read `wf vm ops logs vm-ops-api --tail 300`, fix the cause and push again.
2. 8080 silent: `wf vm recover --dry-run`, then `wf vm recover --instance-id "$OCI_INSTANCE_ID"`. It waits up to 600 s for vm-ops-api, then an OCI soft reboot, then a hard reset, then redeploys each unhealthy app with `recreate=all` from `dev` and re-verifies. A reboot restarts containers by Docker's `unless-stopped`, which never restarts a container that was stopped on purpose; the watchdog's exited rule does, within about three minutes.
3. Last resort: recreate the whole VM. A Terraform recreate is pre-authorised (`infra/oracle-cloud/AGENTS.md`; check the tfstate before destroying) and cloud-init brings vm-ops-api up unattended from the compose file at `PROVISION_REF` with `latest` images. The Nx and pnpm caches are in blob storage, not on a VM volume (`docs/plans/blob-cache/plan.md`), so the recreate keeps them. Confirm with `GET /health` on the recreated host; do not race cloud-init with a manual `docker compose up`.
