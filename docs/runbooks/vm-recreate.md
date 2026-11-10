---
title: vm-recreate - destroy and recreate the one VM, bring every project back, verify
verifies: After `wf vm terraform apply --recreate` every project of vm-apps.json is healthy on the new VM, ci-runner is online on its own tailnet node, agent-shell runs the rebuilt image with the storage guard, the state volumes are restored from the latest verified backup, and one full CI run is green.
tier: manual
maxAgeDays: 90
---

# vm-recreate

The VM (`oci_core_instance.main`, A1.Flex 2 OCPU / 12 GB, 150 GB boot volume) is cattle. A recreate is one command after one preflight; cloud-init brings every project of `infra/oracle-cloud/vm-apps.json` back from the vault. Downtime is not a concern (`infra/oracle-cloud/AGENTS.md`). What matters is that nothing irreplaceable is lost and that the recreate can be repeated without a human. Run it from a session that reaches the Actions API, Bitwarden, the OCI API and the tailnet (a laptop, or the VM's own runner when it is not the thing being destroyed).

## What a recreate destroys, and how each piece comes back

Terraform replaces only `oci_core_instance.main` (and the boot volume with it) and, in the same apply, the `libs_read` pre-authenticated request. Block storage cannot be preserved: the Always-Free cap is 200 GB, the boot volume is 150 GB and the dummy box 50 GB, so `preserve_boot_volume` or a data volume beside the new boot volume is `QuotaExceeded`. Everything on the boot volume is therefore rebuilt, none of it kept.

| Lost with the boot volume | Comes back by |
| --- | --- |
| Tailscale device identities, **deliberately not backed up** (a restored identity collides with the one the node registers on the new VM, and the sidecars re-authenticate with `TS_VM_AUTH_KEY`, so there is nothing to keep) (the `tailscale-state` volume of every project: vm-ops-api, agent-shell, ci-runner, libs-proxy, leetcode-backend, remote-chat-agent, vnc-auth-session, reporting-service, t3-serve, orca-serve, leetcode-auth-cookie-capture) | Each sidecar joins with `TS_VM_AUTH_KEY` (reusable, from the vault). `wf vm terraform apply --recreate` first deletes the old node records, so the new nodes claim the bare names (`ci-runner-main`, `agent-shell-main`, `vm-ops-api-main`), not `-1`. |
| Runner registration (`gh-runner-work`, `.runner` credentials) | `ci-runner` registers with a one-hour `RUNNER_TOKEN` that `wf ship ci-runner --ref dev` mints from `VM_PROVISION_GITHUB_TOKEN` (needs Administration: read and write on the repo) and passes in; the container holds no PAT, so a recreated VM has no runner until that command is run by hand from agent-shell. The old runner record stays offline on GitHub until the new one replaces it or it is deleted (step 7). |
| `gh-runner-pnpm-store`, `docker-state` (agent-shell's docker-in-docker image and build cache), the Cargo target dir | Rebuilt cold by the first CI run (`flow-tests` fills the pnpm store; the first image builds pull and compile everything). Slow, never wrong. |
| `wf-bin`: the `clis` volume (`/opt/clis/current`, `/usr/local/bin/wf` links to it) | Cloud-init creates the empty volume and the link; the first CI run's `preflight` job builds this commit's `wf`, and the `clis` job installs and publishes it (a cold build, up to 60 minutes; the `preflight` job allows 70). Until then `wf` inside agent-shell and the runner is absent. Run `wf` from your own machine meanwhile. |
| agent-shell: the `agent-workspace` volume (worktrees, `~/.claude`, `~/.codex`, `~/.config`, `~/.local`, opencode sessions and history), `leetcode-backend-data`, `orca-state` (orca-serve's paired devices), `vnc-auth-credentials` | Cloud-init restores each from the newest verified restic snapshot in the bucket `mobile-learning-vm-state` **before any app starts** (step 8 only checks it). What the backup leaves out of the workspace is rebuilt or gone: `rca-asks`, `repos`, build caches, `target` dirs, `node_modules`, the pnpm store, `tools/` (mise, npm, cargo homes: `wf vm deps` replays them), `~/.npm`, `~/.cache`. Anything written after the last backup (up to a day) is lost: push every branch before the recreate anyway. |
| `/home/ubuntu/<app>/.env`, compose files | Rewritten by `provision-apps.js` from `vm-apps.json`, the `.env.defaults` of the repo at `provision_ref` and the vault. Hand-appended `.env.local` overrides are lost: there should be none, every override belongs in `.env.defaults` or the vault. |
| `/home/ubuntu/vm-ops-api/state` (redeploy status history), session logs, tmux sessions, shell history | Lost. Not needed. |
| Public IP | New. Everything automated uses MagicDNS names. `VM_HOST` in the CI vault project tracks the IP (step 6). |
| SSH access | Keys baked at create from `ssh_public_key_path` (`ORACLE_DEPLOY_SSH_PUBLIC_KEY` in the vault) and `break_glass_ssh_public_key`. |

What survives, outside the boot volume: the Terraform state (OCI bucket `mobile-learning-tfstate`, object versions kept), the build-cache and libs buckets (crates, Nx cache, pnpm store object), the verification-artifacts bucket, the state backup bucket `mobile-learning-vm-state` (the restic repository, versioned, 14 snapshots per volume), Docker Hub images, Bitwarden, the Autonomous DB, budgets, quotas, policies, the VCN, GitHub, the `dummy-test-infra` box.

Order matters after the apply: the first CI run needs every sidecar up (`libs-proxy` is how Cargo resolves the `mla` registry), and cloud-init takes about 8 minutes (the tailnet shows `libs-proxy` and `reporting-service` when it is done). A push that lands earlier fails `preflight` with `Could not resolve host: libs-proxy`, and a failed run cannot be rerun from the agent session (the Actions API is denied), so push only after `wf vm net device list` shows every app. Ubuntu's `rclone` has no `oracleobjectstorage` backend (the restore fails with `didn't find backend`), so cloud-init and the agent-shell image install the pinned upstream binary.

Refresh `LIBS_READ_PAR_URL` in the vault before the apply that creates the VM, not after: cloud-init writes `libs-proxy`'s `.env` from the vault once, and until the first CI run publishes `wf` no `wf ship` can rewrite it (the redeploy script runs `wf secret sync-env` on the host, which has no `wf` yet). A stale value makes `libs-proxy` answer 502, the first run's `preflight` then cannot resolve the `mla` registry, and the only way out is another recreate.

Until a first backup exists, a recreate also loses remote-chat-agent's device bearers (`device-bearers.json`): `t3-serve` and `orca-serve` then fail their smoke test with `401 Unauthorized` and their deploy legs go red. Mint new ones before the first CI run: `wf offload bearers mint --label t3` and `--label orca` (with `CHAT_AGENT_API_SECRET` in the environment), stored with `wf secret update T3_RCA_BEARER|ORCA_RCA_BEARER --value-stdin --apply`. The Android job needs the host's `binder_linux` module; cloud-init loads it from the base kernel modules (the 7.0 kernels have no `linux-modules-extra` package).

The one other change in the same apply is the `libs_read` PAR: its URL changes, and `LIBS_READ_PAR_URL` in the vault goes stale (step 5). Nothing else may be destroyed; the preflight fails if the plan says otherwise.

Why the image id drift is not an `ignore_changes` casualty: `source_details[0].source_id` and `metadata` are ignored on the instance so no later image publish or cloud-init edit can plan a replacement of a live VM. The recreate still takes the newest Ubuntu image, because a create always uses the current value. The preflight plans with `-replace=oci_core_instance.main`, so it shows the destroy and create that a plain plan (correctly) does not.

## Before: preflight

```sh
git fetch origin dev && git status -sb          # on dev, nothing unpushed
wf vm state backup && wf vm state verify        # a fresh backup of every state volume, proven readable (the nightly one is fine if under 24 h)
wf secret sync-env --help >/dev/null            # wf and the vault credentials work here
wf vm terraform preflight                       # exit 0 only when every check passes; JSON on stdout
```

The verdict lists seven checks and the plan. Each failed check carries its `next` step.

| Check | Passes when |
| --- | --- |
| `tfstate` | The remote state pulls, holds `oci_core_instance.main`, and an identical copy is on disk (`~/.local/state/wf/tfstate-backups/tfstate-<epoch>-serial<N>.json`, mode 600; `WF_TFSTATE_BACKUP_DIR` moves it). Keep the file until step 8 is done. |
| `iam` | The apply creates the dynamic group and policy that let the new VM read its backup bucket (`vm-state.tf`). **A dynamic group is tenancy-scoped, so the Terraform user needs tenancy-level IAM rights** (`manage dynamic-groups` and `manage policies` in the tenancy); without them the apply fails, or the recreated VM cannot read its backup and starts no stateful app. The check passes when the remote state already holds the VM state group and policy, or any other dynamic group (`budget-guard.tf` creates one at the tenancy the same way, which proves the rights); otherwise it fails with the grants to add, or the one-off administrator apply that creates just those two resources. |
| `secrets` | Every group of `vm-apps.json` (so `ci-runner` and `agent-shell` too), `oracle-terraform` and `vm-state-backup` (the restic password; **without it the first boot starts no stateful app**, so a missing one fails here with the mint command of "The restic password" below) verify present and usable (`wf secret verify --for <app>`), plus the Terraform inputs and `VM_PROVISION_GITHUB_TOKEN` for the runner. |
| `backups` | Every app with `stateVolumes` in `vm-apps.json` has a backup under 24 hours old whose newest snapshot of **every** declared volume restores into a scratch directory with its SQLite databases passing `integrity_check`, and the repository is under its budget (`wf vm state verify` on agent-shell; the row of each app says which volume failed and the next step). |
| `tailnet` | The Tailscale admin API lists devices with `TS_VM_OAUTH_CLIENT_SECRET` and `TS_TAILNET_ID`, and a DELETE of a device id that cannot exist answers 404 (no side effect). A 403 there means the OAuth client is read-only (`devices:core:read`): the check fails naming the missing write scope `devices:core`, so a revoked, rotated or read-only client fails here and not after the VM is destroyed. Fix the values or regenerate the client with `devices:core` write in the vault. |
| `ci` | No workflow run is queued, waiting, pending or in progress. Wait or `gh run cancel <id>`. |
| `plan` | The plan of `-replace=oci_core_instance.main` destroys the VM and nothing but the VM and `libs_read`, and the free-tier guard accepts it. Read `plan.changes`: every resource with its actions. Anything else destroyed: find the cause with `wf vm terraform exec infra/oracle-cloud plan` and fix the drift; `--expect <address>` accepts a destroy you intend. |

Also, by hand, before destroying: push every branch worktree of agent-shell that holds work (the backup is at most a day old and leaves out build output).

## State backup

The state that cannot be rebuilt (orca-serve's paired devices, the leetcode backend's data, the saved vnc-auth login, the agent workspace) is snapshotted every night at 03:30 Berlin time by `.github/workflows/vm-state-backup.yml` with **restic**, so a recreate needs no manual backup. What it does:

- `infra/oracle-cloud/vm-apps.json` lists each app's `stateVolumes`: name, `excludes`, and `maxBytes`, the size above which the backup refuses the volume (required; there is no default). The caps are set from measurements with a margin: the agent workspace's kept part measured 0.25 GB on the live VM (7 Nov 2026: 11.8 GB in all, of which `rca-asks` 9.2 GB, `~/.npm` 1.2 GB, `~/.cache` 0.5 GB, `repos` 0.36 GB and `tools` 0.3 GB are regenerable and excluded), so its cap is 1 GB; `leetcode-backend-data` (`session.json`) and `orca-state` (`runtime-id`, `host-key.json`, `devices.json`) are KB, capped at 50 MB each; `vnc-auth-credentials` (login JSON under the kiosk's home) at 200 MB. Add a volume there, and mount it in agent-shell's compose file (`/state/<app>/<volume>`; a test keeps the two in step), to have it backed up.
- `wf vm state backup` runs on agent-shell through vm-ops-api's exec route (agent-shell holds the OCI key and mounts the volumes; the runner holds neither). For each volume it **measures the size from the files' metadata before reading anything** and refuses a volume over its `maxBytes`; then restic reads the volume in place (the excludes are passed to restic, so nothing big is ever read) and uploads only the chunks the repository does not hold: encrypted with `VM_STATE_RESTIC_PASSWORD`, deduplicated, compressed. No tar and no plaintext copy of a volume exists anywhere. The repository is the bucket `mobile-learning-vm-state`, reached through restic's rclone backend (rclone's `oracleobjectstorage` remote with the key in `~/.oci/config` on agent-shell). Retention is `restic forget --group-by host --keep-last 14 --prune`: the newest 14 snapshots of every volume, 14 nightly runs.
- A snapshot is two kinds, told apart by the restic host: `<app>.<volume>.files` (the volume, its live SQLite databases left out) and `<app>.<volume>.sqlite` (consistent copies made with `sqlite3 .backup` into a private scratch directory, removed on every way out, so the live file and its `-wal` are never read; **every regular file under the volume that starts with the SQLite magic bytes is copied this way, whatever its name or extension**, the `-wal`, `-shm` and `-journal` siblings are left out, and a database that appears while the volume is read makes the backup drop its snapshots and read once more, and fail the second time rather than keep a live read). The agent workspace holds databases (the codex and opencode session stores); the other three volumes are plain files. Tags: `app:`, `volume:`, `kind:`, `run:<stamp>`, `ms:<epoch ms>`, `sqlite`, and `verified`.
- `wf vm state verify` covers **every volume `vm-apps.json` declares**: it restores the newest snapshot (and the sqlite snapshot of its run) into a private scratch directory (restic `--verify`), runs `PRAGMA integrity_check` on every database in the restored copy, removes the scratch directory, and only then tags the snapshots `verified`; it also runs `restic check --read-data-subset 5%` on the repository. A missing volume, a damaged snapshot, a failed integrity check or a repository over budget fails the app: any failure is a failure. The workflow runs it right after the backup and the recreate preflight runs it again. `wf vm state list` shows the snapshots.
- The size is bounded from the same measurements: the live repository may hold twice the sum of the volumes' `maxBytes` (2.6 GB: `vm_state_repository_bytes` in `vm-state.tf`, `vmStateRepositoryBytes` in `clis/blob-cache/data/budget.json`, and `2 x sum(maxBytes)` of `vm-apps.json`) and the **bucket, every object version included, four times** (5.2 GB: `vm_state_budget_bytes`, `vmStateBytes`, `4 x sum`; a test keeps all of them equal), counted with the cache under the tenancy's 20 GB. restic's own count sees only the live repository, but the bucket is versioned and every pack a nightly `forget --prune` deletes stays as a previous version, billed, until the lifecycle rule removes it (one day, the shortest OCI takes; a prune deletes at most what the repository held, hence the second repository in the budget). So the backup lists every object version (`oci os object list-object-versions`) and **refuses to start when the bucket plus the most one run can add (the sum of the caps) would pass its budget**, and fails after its prune when the bucket or the live repository is over; the storage guard lists the bucket and never deletes from it. The bucket has no lifecycle rule that expires current objects (the repository's old packs are live data). The cap of a volume is also enforced **during the read**: restic gets `--exclude-larger-than <cap>`, and after the read the backup checks that it read no more than the cap and that the volume still measures under it, and otherwise drops its own snapshots and fails (a volume that grew between the measure and the read is never kept).
- Redeploying agent-shell restarts the container under a run: it is done by hand, so no workflow concurrency group can cover it. Instead a verb whose answer is missing ("agent-shell did not answer", "printed no result") is asked again up to 4 times with a growing pause (60 s, 120 s, 180 s).
- Failures: the job fails and writes the verdict to its summary; the verb reports `ci.vmstate.ok` and `ci.vmstate.age_seconds` and the event `vm.state.<verb>` through the cache-health path (`docs/runbooks/cache-health-alerts.md`, rule 7).

**Scratch is RAM.** The staged copies and the restore of a verify are plaintext credentials and databases, so agent-shell mounts a `tmpfs` at `/scratch` (`size=1280m`: the largest cap, the 1 GB agent workspace, plus a quarter; RAM is used only while a run holds files there) and the verb's script puts every scratch directory there and refuses to run without it. A kill, an OOM or a restart takes the directory with the mount, so nothing plaintext reaches a disk; the traps and the sweep of a dead run's directory stay as well.

**Backend, and why.** The nightly verbs run on agent-shell with the admin key it already holds (rclone `user_principal_auth` over `~/.oci/config`), so they need no new secret. The restore on a fresh VM runs on the host in cloud-init with the VM's own identity: `vm-state.tf` grants every instance of the compartment (a dynamic group; the recreated VM has a new id, so matching one id would race the restore) `manage objects` on this bucket only, and rclone's `instance_principal_auth` uses it. The one secret is the restic password, which cloud-init already reads from the vault with the other secrets. The S3-compatible endpoint with a minted customer secret key was the alternative: it would need a secret that Terraform cannot store in the vault and that a human would have to mint.

**The restic password.** `VM_STATE_RESTIC_PASSWORD` is in the vault (project `mobile-learning-deploy-secrets`), group `vm-state-backup`, minted once and never printed. It replaces the age keypair; the old `VM_STATE_AGE_*` vault entries are no longer read by anything (they stay in the registry marked ORPHANED, RETIRED, do not use, because vault secrets are never destroyed and the secrets check refuses a vault entry the registry does not name). Never replace it: a new password makes the repository unreadable; `restic key passwd` rotates it in place. To mint it, if the vault has none:

```sh
openssl rand -base64 32 | tr '+/' '-_' | tr -d '=\n' | wf secret create VM_STATE_RESTIC_PASSWORD <PROJECT_ID> --value-stdin --apply
```

**Not backed up, on purpose:** Tailscale node state (a restored identity collides with the one the node registers on the new VM; every sidecar re-authenticates with `TS_VM_AUTH_KEY`), the `clis` volume, the pnpm store, build caches and the docker-in-docker state (all rebuilt), and `remote-chat-agent`'s state volume (not in the owner's list: add it to `vm-apps.json` if it should be).

**Restore on a fresh VM.** `provision-apps.js` in cloud-init runs `restore-state.js` after Docker is ready and **before any app is started** (so no app writes its starter files while the volumes are filled): for each state volume it creates the Docker volume, and if it is empty restores the newest snapshot tagged `verified` (and the sqlite snapshot of the same run) with the host's restic. A volume that already holds data is left alone. A repository that does not exist, or a volume that was never backed up, continues without error and says so in `/home/ubuntu/provision-apps.log` (the first boot ever). The first read of the bucket waits **up to 15 minutes** for the new dynamic group and policy to propagate (an instance principal that is not authorized yet is told "not found", which looks like an empty bucket, so "no repository" counts as a first boot only when `rclone lsf` can list the bucket), and the snapshot listing and every restore are retried with a doubling pause (20 s, 40 s, ... up to 2 minutes, 6 attempts), each attempt one JSON line in `/home/ubuntu/provision-apps.log`; a wrong password is never retried. A repository that stays unreadable, a missing restic password, a volume with snapshots but none verified, or a restore that fails is loud: the volume is emptied again, the app that owns it (and any app whose compose file mounts it) is **not started**, and provisioning exits non-zero naming the volume and **the exact command to rerun on the VM once the cause is fixed**: `HOST_LABEL=<label> node /home/ubuntu/provision-apps.js` (volumes that already hold data are kept, so it only fills the ones that failed and starts the apps). There is no `--force` and no way to start an app empty over an existing snapshot.

**Apps never touch restored state.** Each app creates its first-start files only when they are missing: orca-serve (`runtime-id`, `host-key.json`; a corrupt key is an error, not a reset), leetcode-backend (`session.json` is written only by a captured session), vnc-auth-session (the listener reads and deletes credentials only on an explicit login or logout) and agent-shell (its entrypoint only `mkdir -p`s the home layout). A flow test per app starts it on a pre-populated state directory and asserts every file is byte-identical afterwards.


**Schedule.** GitHub's `schedule` runs in UTC; 03:30 Berlin is 02:30 UTC in winter (CET) and 01:30 UTC in summer (CEST). The workflow has both cron entries and its first step lets through only the one that falls in the 03:xx hour in Berlin. GitHub starts scheduled runs late in this private repo, and a run that starts after 03:59 Berlin time is skipped, so the same job runs for an empty `dev` commit whose message carries `[vm-state-backup]` (a Claude scheduled task, `CRON_TZ=Europe/Berlin 30 3 * * *`, pushes it), and by hand.

## Run

1. **Recreate.** One command; it checks the state holds the VM, destroys the VM, deletes the old tailnet records, then runs the guarded apply (free-tier guard, post-apply convergence check, state rollback on failure). A tailnet cleanup that fails is retried 4 times with a growing pause and then stops the command before the VM is created (exit 1, one JSON verdict with `"vmDestroyed": true`, `"created": false`, the error of each attempt under `evidence` and the commands to continue under `next`). Continue with `wf vm net device retire --replaced-vm --apply` (it deletes every tagged VM node record whatever its age, the same selection the recreate uses; plain `retire --apply` only takes records older than the stale window, which right after the destroy can be none), then `wf vm terraform apply infra/oracle-cloud <OCI_COMPARTMENT_OCID>` (the plain guarded apply, which creates the VM): a node that registers while an old record holds its name becomes `<name>-1`, which vm-gateway cannot reach. Run it in the background with the log in a file; the apply retries `QuotaExceeded` for up to ten minutes while OCI releases the old instance.

   ```sh
   wf vm terraform apply --recreate > .agents/tmp/recreate.log 2>&1
   ```

   Never `terraform apply -replace` by hand (`wf vm terraform exec` refuses it): a raw replace leaves the old nodes holding the bare hostnames.

2. **Wait for cloud-init** (15 to 25 minutes: Docker, Node, `bws`, then `provision-apps.js` pulls and starts every project in `vm-apps.json` order). Do not run `docker compose` by hand: it races cloud-init. Poll `wf vm ops health` until vm-ops-api answers at the new commit; `wf vm logs --service cloud-init` shows the `[provision]` lines (BetterStack).

3. **Out of capacity.** If apply fails with `Out of host capacity`, retry with another availability domain: `wf vm terraform apply --recreate` as the guarded apply of the rollback section with `TF_VAR_availability_domain_index=1` (then `2`) in the environment; the state holds nothing of the old VM, so it is safe to repeat.

4. **Rebuilt agent-shell and `wf-bin`.** Cloud-init pulled the images at `latest`. The agent-shell image with the storage guard and `cpu_shares: 128` is built and shipped by the first CI run; to do it before that, redeploy the tag CI pushed: `wf ship agent-shell --ref <sha> --redeploy-only --tag sha-<8>` (agent-shell is deployed by hand: the pipeline never restarts its own runner), then check below.

5. **New `LIBS_READ_PAR_URL`.** The plan's `afterApply` names the commands. The new URL replaces the stale one in the vault and libs-proxy takes it up:

   ```sh
   wf vm terraform exec infra/oracle-cloud output -raw libs_read_par_url | wf secret update LIBS_READ_PAR_URL --value-stdin --apply
   wf ship libs-proxy --ref dev --redeploy-only   # redeploys the tag at latest; CI's next push builds a new one
   ```

6. **`VM_HOST`.** The public IP changed. Update the CI vault project's `VM_HOST` (`ubuntu@<ip>`, from `wf vm terraform exec infra/oracle-cloud output -raw vm_host`) with the command in `infra/oracle-cloud/AGENTS.md` ("CI secrets live in Bitwarden"). It is not a GitHub Actions secret.

7. **Stale runner record.** `gh api repos/nuesslerm/expo-hello-world-app-2/actions/runners --jq '.runners[] | [.id,.name,.status] | @tsv'`. A second `vm-main` that is `offline` is the old one: `gh api -X DELETE repos/nuesslerm/expo-hello-world-app-2/actions/runners/<id>`. The new runner is `online` with label `vm`.

8. **Check the restored state.** Nothing to run: cloud-init already restored every state volume from its newest verified snapshot before it started the first app (see "State backup", "Restore on a fresh VM"). Read `/home/ubuntu/provision-apps.log` (through `wf vm exec`) for one `state <volume>: restored (...)` line per volume; `none (no snapshot...)` is the first boot ever. A `FAILED` line names the volume that was not restored and the app that was therefore **not started**: fix the cause it names (the restic password in the vault, the bucket, a snapshot that never verified: `wf vm state verify`), then re-run `HOST_LABEL=main node /home/ubuntu/provision-apps.js` over `wf vm exec`, which restores into the still empty volume and starts the app. Then:

   ```sh
   wf vm state verify
   wf vm ops probe orca-serve leetcode-backend vnc-auth-session
   ```

   Orca-serve answers `status.get` for the paired phone without pairing again, and `wf secret login <tool>` is needed only for a login the backup did not hold.

   **Restoring state by hand** (one damaged volume of a live VM, rare): `wf vm state restore --app <app>` refuses while the app has a running container and while any of its volumes holds data, and prints the steps: it only ever fills an empty volume of a stopped app, there is no `--force`, and vm-ops-api has no route that stops an app, so stop it (`wf vm exec`: `docker compose -p <app> stop` in `/home/ubuntu/<app>`), empty the volume, run the verb, and the CLI recreates the app (**agent-shell's own workspace is restored only by cloud-init on a fresh VM**: it holds the running wf and the session that runs it, so `wf vm state restore --app agent-shell` refuses outright).

## Verify

Stop at the first failure and take its rollback.

| Check | Command | Expect |
| --- | --- | --- |
| Every project healthy | `wf vm ops probe $(jq -r '.[].name' infra/oracle-cloud/vm-apps.json)` and `wf check --health` | Exit 0; each of the eleven apps answers on its tailnet name (the apps without `/health`, `ci-runner` and `agent-shell`, show as healthy containers). |
| Tailnet names | `wf vm net resolve vm-ops-api-main agent-shell-main ci-runner-main libs-proxy-main` and `wf vm net device list` | Every name resolves to itself, no `-1` suffix; `ci-runner-main` is its own node, not the agent-shell one. |
| ci-runner on its own node | `docker`-less: `wf vm ops logs ci-runner` and the runner list of step 7 | The listener says `Listening for Jobs`; `online`. Redeploying agent-shell (`wf ship agent-shell --ref <sha> --redeploy-only --tag sha-<8>`) leaves `ci-runner-gh-runner`'s start time unchanged (`wf vm ops stats`). |
| agent-shell guard active | `wf vm exec` of a script that prints `docker inspect agent-shell --format '{{.HostConfig.CpuShares}}'`, and `wf cache guard` | `128`; the guard (it runs in agent-shell through vm-ops-api's exec route, so it only answers when the rebuilt agent-shell and `wf-bin` are there) prints its result line with `action` set, not an error. The CI run's "Storage guard" step reports the same. |
| State restored | `wf vm state verify` and `wf vm ops probe orca-serve leetcode-backend vnc-auth-session` | Every app `ok`; no device has to pair again; the next nightly backup is green. |
| One full CI run | Push a commit that changes a file to dev, tagged `[services:all] [cache:no]` for a cold run (an empty commit starts no `deploy.yml` run), once the first run has published `wf`; follow `gh run watch` | Every job green: `plan`, `flow-tests`, `typecheck-lint-unit-tests`, `clis`, each `deploy (<service>)`, `integration-tests`, `cli-e2e-tests`, `deployment-verification-tests`, the browser and Android e2e jobs, `eas-update`. The first run is cold and long. |

## Rollback

- **Apply failed after the destroy.** The VM is already out of the state, so `--recreate` refuses (it would strand nothing, but it checks the state holds the VM). Run the guarded apply instead, which creates it: `wf vm terraform apply infra/oracle-cloud "$(wf secret get OCI_COMPARTMENT_OCID)"`. A post-apply plan that still shows changes is rolled back to the pre-apply state by `wf` itself. Capacity errors: step 3.
- **State damaged.** `wf vm terraform exec infra/oracle-cloud state push -force <backup file from the preflight>`, then `plan`. The bucket also keeps object versions.
- **A project does not come up.** `wf vm recover` health-sweeps every app and redeploys the unhealthy ones; for one project `wf ship --redeploy-only <app> --ref dev --tag <tag>`. A node that came up as `-1`: `wf vm net device retire --apply` (the old record is stale by then), then restart that project's sidecar.
- **Nothing of the VM works at all.** Recreate again; there is no in-place repair of a boot volume to attempt first (break-glass via the dummy box is in `infra/oracle-cloud/AGENTS.md`).

## After the recreate

1. **Mint `ORCA_VERIFY_PAIR_URL`** (only if the vault has none, or the restore of `orca-state` did not bring the device back). Once `orca-serve` runs, pair one device with it. The verb prints only the `orca://pair?code=...` link, a credential, so pipe it and never print it into a log or a PR:

   ```sh
   wf vm ops pair orca-serve | wf secret update ORCA_VERIFY_PAIR_URL --value-stdin --apply
   ```

   `wf vm ops pair` calls `POST /services/orca-serve/pair` on vm-ops-api, which runs `orca-serve pair` inside the container. Exit 1 with a JSON error on stderr means no link was minted; `wf vm ops logs orca-serve` says why.

2. **Merge `origin/main` into `dev`** before anything else starts (issue #364 phase 2; `docs/solutions/workflow/dev-parity-with-main.md`): `git fetch origin && git merge origin/main`, regenerate lockfiles and generated files with the repo's tooling (never by hand), run the gate, push.

3. Tick the phase 1 items of #364 with the sha and the verdict of the preflight, the verify table and the green CI run.
