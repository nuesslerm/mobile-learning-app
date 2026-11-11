---
title: "Which vault keys each CI job reads: Wave 0 of the CI-scoped vault plan"
date: 2026-11-08
category: ci
module: ci-vault-scopes
problem_type: workflow_issue
component: development_workflow
severity: high
applies_when:
  - "Splitting the CI vault token into ci-open and ci-deploy scopes (docs/plans/ci-scoped-vault-access/plan.md)"
  - "Moving a job to a GitHub-hosted runner or changing what a workflow passes to a job"
  - "Adding a `wf` verb or a test that resolves a secret"
tags: [ci, secrets, vault, bitwarden, runners, scopes]
---

# Which vault keys each CI job reads

Wave 0 of `docs/plans/ci-scoped-vault-access/plan.md` (issue #367), units U0.1 and U0.2. It was derived by reading code only, at dev `789e35096` on 8 Nov 2026: no credential was used, no network was reached, no value was read, and no job was run. Everything below is key names and counts. Because nothing ran, every entry is an upper bound taken from code paths: a key marked optional in `clis/secrets/src/secrets/groups.json` counts as read, and a verb counts as read on every run even when it exits early on some inputs. The plan asks for a dry run in Wave 3 to confirm; this table is what that dry run should be compared against.

## How a job gets a secret today

No workflow in `.github/workflows` hands a vault token to a job on the VM runner. The `ci-runner` container carries `BWS_ADMIN_ACCESS_TOKEN` and `BWS_SERVER_URL` in its own environment (`infra/oracle-cloud/ci-runner/docker-compose.prod.yml:74-75`), together with `VM_OPS_API_AUTH_TOKEN` and `VM_GATEWAY_AUTH_SEED` (`:71-72`). So every step of every job on that runner can resolve any key the deploy-read token reads, whether or not the job needs it. The sections below say what each job uses, not what it could reach.

**Update, CI-port glue (#367 phases 4 to 6, code only, nothing deployed).** The compose file no longer puts `VM_OPS_API_AUTH_TOKEN` or `VM_GATEWAY_AUTH_SEED` in the runner's environment: `VM_GATEWAY_AUTH_SEED` was not dead (`eas-update` and the browser export baked it as the backend token), so both steps now read it with `wf secret get` at step time, as they already did for the BetterStack pair. `deploy.yml` and `e2e-tests.yml` set `WF_VM_CI_PORT=8081` for every job: the CI routes go to the CI port signed with `VM_OPS_API_CI_AUTH_TOKEN` (resolved from the vault), the admin-only routes still go to 8080 with `VM_OPS_API_AUTH_TOKEN` (also resolved from the vault now). The jobs that still read the admin seed, and why: `plan` (`wf cache guard` relays through the exec-script), `vm-state-backup` (`wf vm state backup|verify` are exec-scripts), `e2e-tests` on android (`wf android start-device|stop-device` lease through the exec-script), `deploy` (the old `build_on=agent-shell` path is an exec-script, and the failure report's `wf vm exec --status`; the leg's group verify also lists the key for `vm-ops-api` and `vnc-auth-session`). `preflight` and `eas-update` read only the CI seed. So `VM_OPS_API_AUTH_TOKEN` stays scope `ci-deploy` until those calls leave CI jobs.

Keys reach a job in one of five ways:

- `wf` verbs resolve them through `secrets::load_values` (environment first, then the vault). The verbs that matter are listed under "Verbs that resolve keys".
- A step runs `wf secret get <KEY>` (the BetterStack pair in `eas-update` and the browser export, `GCLOUD_DRIVE_CREDENTIALS_DB_B64` in the Gem sync).
- A step reads the runner's own environment (`VM_GATEWAY_AUTH_SEED` in `eas-update` and the browser export).
- `bitwarden/sm-action` fetches keys by id with the CI-read token (the three `tailscale-*` workflows, which run on `ubuntu-latest`, so that token already reaches GitHub-hosted runners today).
- Test code calls `wf secret get` itself (`backend/service-kit/src/e2e.rs`, `clis/orca-serve/tests/deployed_verification.rs`, `clis/offload/tests/e2e/parity.rs`).

`wf test run` strips `BWS_*`, `GITHUB_TOKEN`, `GH_TOKEN` and every registered env var from the processes of its fixtures (`clis/wf-testkit/src/run.rs:95-115`), and sets `WF_USAGE=off` for them, so a fixture's own `wf` reads nothing. The deployed-verification suites above are the exception that matters, because they ask the vault by name.

## Verbs that resolve keys

Shorthand used in the table.

| Name | Keys | Where in code |
| --- | --- | --- |
| usage pair | `WF_USAGE_ORDS_BASE_URL`, `HEALTH_REPORTS_ORDS_PASSWORD` | `clis/wf/src/usage_row.rs:8-9,61-67`: every `wf` command posts one row unless `WF_USAGE=off`; the password is a database user's password that inserts rows |
| ledger pair | `HEALTH_REPORTS_ORDS_BASE_URL`, `HEALTH_REPORTS_ORDS_PASSWORD` | `clis/pipeline/src/ci_ledger.rs:218`: `wf ci preflight`, `wf ci audit`, and the Nx ledger of `wf test run` (`nx_ledger.rs:134`) |
| test-run pair | `TEST_RUNS_ORDS_BASE_URL`, `HEALTH_REPORTS_ORDS_PASSWORD` | `clis/pipeline/src/test_metrics.rs:9-10,233`: `wf test run` and `wf test publish` write a row per run |
| cache pair | `BUILD_CACHE_READ_PAR_URL`, `BUILD_CACHE_WRITE_PAR_URL` | `clis/wf-bucket/src/cache.rs:189`, `cache/pnpm.rs:49`: both are always loaded; the cache is off if either is missing. Whether a run may write is decided afterwards by git facts (`cache.rs:68`: a clean checkout of a commit on `origin/dev` with the pinned node and pnpm), not by the event or a token Since wave 2 (U2.5) the write link is asked for only when git facts allow a write, and in GitHub Actions only on `push` to `dev`; on any other event the shim loads the read link alone and runs read-only |
| metrics pair | `BETTERSTACK_METRICS_HOST`, `BETTERSTACK_METRICS_TOKEN` | `clis/wf-bucket/src/ci_metrics.rs:16-17,85`: cache events; the token is an ingest (write) token |
| relay pair | `VM_OPS_API_AUTH_TOKEN`, `VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY` | `clis/wf-vmops/src/vmops.rs:70-83`: every signed call to vm-ops-api (`wf ship`, `wf vm ops`, `wf vm exec`, `wf vm state`, the relay of `wf cache guard`, `wf android start-device`) |
| bundle URL | `VERIFICATION_ARTIFACTS_PAR_URL` | `clis/wf-bucket/src/artifacts.rs:54-64`: `wf test publish`, `wf mimic publish`, and the source maps of `wf ship eas`; it is a write URL. In GitHub Actions it resolves only for `push` to `dev` (U2.5); elsewhere `wf test publish` and `wf mimic publish` fail with `write URL refused` |

`DOCKERHUB_TOKEN` is read by the `build-image` action in the CI job itself (the push step, masked): the image is built on the runner's builder sidecar and vm-ops-api only pulls the pushed tag, so no remote build is started through the relay pair. The deploy leg no longer verifies its app's group before building (that preflight belonged to the retired agent-shell build; `secrets::verify_for` now serves `wf ci preflight` and `wf vm terraform preflight`). `wf cache guard` on the plan job is also a relay: the OCI key stays on agent-shell (`clis/wf-bucket/src/cache/guard.rs:1-5`), so no job on the runner reads an OCI key. `.github/actions/install-oci` is used by no workflow.

## (1) Job table

Scope column: what the plan's two scopes would call the job given the keys it reads in today's code. "open" means every key it reads is in the plan's `ci-open` list; "deploy" means at least one key is in the plan's `ci-deploy` list. A key that the plan lists in neither scope is flagged in the last column of section (2), not here.

Events. `deploy.yml` runs on `push` to `dev` (paths filter) and `workflow_dispatch`; it has no `pull_request` trigger. The jobs of `deploy.yml` also run for a dispatch of any ref, which then runs that ref's copy of the workflow.

| Job | Keys it reads (names) | Scope | Events | Write URL? | Token beyond `GITHUB_TOKEN`? |
| --- | --- | --- | --- | --- | --- |
| `CI` / preflight | leg verify of group `ci` (`BUILD_CACHE_READ_PAR_URL`, `BUILD_CACHE_WRITE_PAR_URL`, `HEALTH_REPORTS_ORDS_BASE_URL`, `HEALTH_REPORTS_ORDS_PASSWORD`, `ORCA_VERIFY_PAIR_URL`, plus `TEST_RUNS_ORDS_BASE_URL` for the password's live check); build-cache probe; ledger pair; relay pair (signed `/health`); usage pair | deploy (relay pair, cache write) | push `dev`, dispatch | yes: writes a probe object through the cache write URL on every run, with no ref gate (`ci_preflight.rs:79-95`) | no (`GITHUB_TOKEN`, `actions: read`) |
| `CI` / plan | relay pair (`wf ship --changed`, `wf cache guard` relay); usage pair | deploy (relay pair) | push `dev`, dispatch | no URL; the guard relay deletes bucket objects through agent-shell's OCI key | no (`GH_TOKEN` is `github.token`) |
| `CI` / flow-tests | cache pair; ledger pair; test-run pair; metrics pair; bundle URL; usage pair | open keys plus the cache write URL | push `dev`, dispatch | yes: cache (pnpm store save, Nx replays) and the flow run bundle | no |
| `CI` / typecheck-lint-unit-tests | cache pair (pnpm, Metro, Nx); ledger pair; test-run pair; metrics pair; usage pair. It does not publish a bundle | open keys plus the cache write URL | push `dev`, dispatch | yes: cache | no |
| `CI` / clis (`clis.yml`) | usage pair only, and only when a vault token is present (no `install-bws` step); builds and installs `wf`, the `current` pointer is VM-local | open | push `dev` (via `deploy.yml`), dispatch | no | no |
| `CI` / deploy (matrix, one leg per changed service) | leg verify of the leg's group plus `DOCKERHUB_TOKEN` (names in the per-leg table); relay pair; cache pair (image layer cache export); usage pair | deploy | push `dev`, dispatch | yes: image cache export | no (a failure comment uses `github.token`); but `VM_PROVISION_GITHUB_TOKEN` is read by the legs `vm-ops-api` and `reporting-service` (optional group key, live-checked against GitHub) |
| `CI` / integration-tests | cache pair; ledger pair; test-run pair; metrics pair; bundle URL (publish step); usage pair | open keys plus the cache write URL | push `dev`, dispatch | yes: cache and bundles | no |
| `CI` / cli-e2e-tests | same as integration-tests, plus `OPENCODE_GO_API_KEY` (`clis/offload/tests/e2e/parity.rs:24`); no publish step in the job | open keys plus the cache write URL, plus a model key | push `dev`, dispatch | yes: cache | no |
| `CI` / deployment-verification-tests | cache pair; ledger pair; test-run pair; usage pair; and by the suites' own `wf secret get`: `CHAT_AGENT_API_SECRET`, `CH_BACKEND_HOST`, `CH_BACKEND_USER`, `CH_BACKEND_PASSWORD`, `CH_BACKEND_TABLE`, `ORCA_VERIFY_PAIR_URL` | deploy-type (service secrets, a database password) | push `dev`, dispatch | yes: cache | no |
| `CI` / eas-update | `EXPO_TOKEN`; `VM_GATEWAY_AUTH_SEED` (runner env, baked into the bundle as the backend token); `EXPO_PUBLIC_BETTERSTACK_TOKEN` (vault name `BETTERSTACK_SOURCE_TOKEN`), `BETTERSTACK_INGEST_HOST`; bundle URL (source maps); relay pair (`wf vm ops stats`); cache pair (pnpm, Metro); usage pair | deploy | push `dev`, dispatch | yes: cache and source maps | no |
| `CI` / browser-e2e-tests (`e2e-tests.yml`) | cache pair (pnpm, Metro, Nx); ledger pair; test-run pair; bundle URL (`WF_TEST_PUBLISH=1`); relay pair (`wf vm ops probe`, `stats`); `VM_GATEWAY_AUTH_SEED` (runner env, baked into the export); BetterStack pair as in eas-update; usage pair | deploy (seed, relay pair) | push `dev` (workflow_call), dispatch of any ref | yes: cache and every test's bundle | no; but `secrets: inherit` (`deploy.yml:931`) passes every repo secret to the called workflow |
| `CI` / android-e2e-tests (`e2e-tests.yml`) | as browser, without the export: relay pair also drives `wf android start-device`/`stop-device`; no BetterStack pair | deploy | push `dev` (workflow_call), dispatch | yes: cache and bundles | no; `secrets: inherit` (`deploy.yml:945`) |
| `CI` / report | usage pair; `wf ci comment`, `wf test results`, `wf ci cache-health` read the ledger and bundles; no cache write found | open | push `dev`, dispatch | no (not found in code) | no: `GITHUB_TOKEN` with `contents: write`, `issues: write`, `pull-requests: write`, `actions: read` |
| `CI audit` (`ci-audit.yml`) | cache pair (`wf cache purge nx`, Nx runs with `--fresh`); ledger pair; test-run pair; bundle URL (`wf test publish`); usage pair | open keys plus the cache write URL | `schedule` (02:00 and 03:00 UTC), push `dev` with `[ci-audit]`, dispatch | yes: Nx cache writes, cache purge, audit bundle | no |
| `vm-state-backup.yml` | relay pair (`wf vm state backup`, `verify`); usage pair. The restic password stays on agent-shell: `VM_STATE_RESTIC_PASSWORD` is read only on the `--local` path (`clis/deploy/src/vm_state.rs:217`) | deploy (relay pair) | `schedule` (01:30 and 02:30 UTC), push `dev` with `[vm-state-backup]`, dispatch | no | no |
| `track-contracts.yml` plan, capture, report | `wf parity track plan` and `wf mimic trigger` read none; `capture` installs `bws` after the capture and then `wf mimic publish` reads the bundle URL; `report` runs `wf test publish` (bundle URL); usage pair | open keys plus a write URL | push `dev` with a `[track ...]` marker | yes: bundle URL | no (`github.token`, `actions: write` on capture) |
| `record-references.yml` plan, capture | as `track-contracts.yml`: bundle URL in `wf mimic publish` (record only) | open keys plus a write URL | push `dev` with a `[record ...]` or `[replay ...]` marker | yes: bundle URL | no |
| `repomix-gemini-context.yml` | `GCLOUD_DRIVE_CREDENTIALS_DB_B64` (`wf secret get`); `CHAT_AGENT_API_SECRET` and `VM_HOST` (`wf offload ask`, `clis/offload/src/rca_client.rs:11`, `dispatch_run.rs:847`); usage pair | deploy-type (a Google login) | push `dev` with `[gem-sync`; no cron | no | no |
| `tailscale-acl.yml` | by id through `sm-action` with the CI-read token: `TS_TAILNET_ID`, `TS_CI_OAUTH_CLIENT_ID`, `TS_CI_OAUTH_CLIENT_SECRET` | deploy (Tailscale OAuth) | dispatch only (`ubuntu-latest`) | no | no |
| `tailscale-auth-key-expiry.yml` | by id with the CI-read token: `TS_TAILNET_ID`, `TS_VM_OAUTH_CLIENT_ID`, `TS_VM_OAUTH_CLIENT_SECRET`; `wf` runs with `BWS_ACCESS_TOKEN` set to the same token, so its usage row and any other lookup go to the CI-read project | deploy (Tailscale OAuth) | dispatch, push `dev` with `[ts-key-watch]` (`ubuntu-latest`) | no | no |
| `tailscale-rotate-auth-key.yml` | the same three keys as expiry, by id; names a GitHub repo secret `BWS_ADMIN_ACCESS_TOKEN` (an edit-capable vault token) that is "absent by design"; `--apply` writes `TS_VM_AUTH_KEY` | deploy, and the only workflow that references an edit token | dispatch only (`ubuntu-latest`; the schedule is commented out) | no | no |
| `ci-budget.yml` | none; `gh variable set CI_BUDGET_LOW` with `github.token` | open | dispatch, push `dev` with `[ci-budget-reset]` | no | possibly: see open question 8 |
| `scheduled-triggers.yml` | none; `github.token` reads one commit and starts a workflow | open | `push` | no | no |
| `architecture.yml`, `ci.yml`, `actionlint.yml` | none (already `ubuntu-latest`, no vault token, `wf` finds no credential) | open | dispatch only | no | no |

### Per-leg keys of the deploy job

`wf ship <app>` verifies the group of the leg plus `DOCKERHUB_TOKEN` from the runner (`verify_for`). The legs the pipeline ships are the ones of `RECOVER_APPS` in `clis/wf-vmops/src/services.rs:8-19` except the two deployed by hand (`agent-shell`, `ci-runner`), plus `vm-ops-api`. The number is the group's required plus optional keys, before adding `DOCKERHUB_TOKEN`.

| Leg | Group keys | Notable keys beyond the common ones |
| --- | --- | --- |
| `vm-ops-api` | 6 | `VM_OPS_API_AUTH_TOKEN` (the admin seed), `VM_OPS_API_CI_AUTH_TOKEN` (the CI port's seed, added by U1.6), `TS_VM_AUTH_KEY`, `VM_PROVISION_GITHUB_TOKEN` |
| `leetcode-backend` | 7 | `LEETCODE_BACKEND_ADMIN_SECRET`, `LEETCODE_CAPTURE_TRIGGER_SECRET`, `VM_GATEWAY_AUTH_SEED` |
| `remote-chat-agent` | 19 | `TELEGRAM_BOT_TOKEN`, `OPENCODE_GO_API_KEY`, `MINIMAX_API_KEY`, `KIMI_API_KEY`, `GROQ_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`, `HEALTH_REPORTS_ORDS_PASSWORD`, `EDIT_STATS_ORDS_BASE_URL`, `POOL_COOLDOWNS_ORDS_BASE_URL` |
| `leetcode-auth-cookie-capture` | 7 | `TELEGRAM_NOTIFY_SECRET` |
| `vnc-auth-session` | 10 | `VNC_AUTH_*` (4), `VM_OPS_API_AUTH_TOKEN` |
| `reporting-service` | 9 | `HEALTH_REPORTS_ORDS_*` (2), `BETTERSTACK_*` (5), `VM_PROVISION_GITHUB_TOKEN` |
| `libs-proxy` | 2 | `LIBS_READ_PAR_URL` |
| `t3-serve` | 2 | `T3_RCA_BEARER` |
| `orca-serve` | 2 | `ORCA_RCA_BEARER` |

## (2) Counts per scope and the comparison with the plan

Counting every key any workflow job above reads on the runner side, with `registry.json` as the source of names:

- From the deploy-read project: 48 keys.
- From the CI-read (`gh-ci`) project: 5 keys (`TS_TAILNET_ID`, `TS_CI_OAUTH_CLIENT_ID`, `TS_CI_OAUTH_CLIENT_SECRET`, `TS_VM_OAUTH_CLIENT_ID`, `TS_VM_OAUTH_CLIENT_SECRET`). These are the only five `registry.json` entries marked `vault: gh-ci`, and the only keys the `sm-action` blocks name.
- Union: 53 keys, all present in `registry.json`.

Against the plan's target scopes:

| Bucket | Count | Keys |
| --- | --- | --- |
| In the plan's `ci-open` list and read by a job | 8 | `BUILD_CACHE_READ_PAR_URL`, `LIBS_READ_PAR_URL`, `HEALTH_REPORTS_ORDS_BASE_URL`, `TEST_RUNS_ORDS_BASE_URL`, `WF_USAGE_ORDS_BASE_URL`, `EDIT_STATS_ORDS_BASE_URL`, `POOL_COOLDOWNS_ORDS_BASE_URL`, `VERIFICATION_ARTIFACTS_PAR_URL` |
| In the plan's `ci-deploy` list and read by a job | 11 | `VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY`, `VM_GATEWAY_AUTH_SEED`, `VM_OPS_API_AUTH_TOKEN`, `DOCKERHUB_TOKEN`, `EXPO_TOKEN`, `BUILD_CACHE_WRITE_PAR_URL`, the five Tailscale keys above |
| Read by a job, in neither list | 34 | see below |

The plan's `ci-deploy` list also names OCI. No job on the runner reads any OCI key (`OCI_*`, seven keys): the only readers are `wf cache guard --local` on agent-shell and Terraform. `DOCKERHUB_USERNAME` is a public value, not read by a job either.

The 34 keys the plan does not place in a scope:

- Read by almost every job (3): `HEALTH_REPORTS_ORDS_PASSWORD` (a database user's password that inserts rows, so it is not a "read-only value"), `BETTERSTACK_METRICS_HOST`, `BETTERSTACK_METRICS_TOKEN` (an ingest token).
- Baked into the app by `eas-update` and the browser export (2), which U2.4 turns into repo variables: `EXPO_PUBLIC_BETTERSTACK_TOKEN` (vault name `BETTERSTACK_SOURCE_TOKEN`), `BETTERSTACK_INGEST_HOST`.
- Read by test code in the verification jobs (7): `CHAT_AGENT_API_SECRET`, `CH_BACKEND_HOST`, `CH_BACKEND_USER`, `CH_BACKEND_PASSWORD`, `CH_BACKEND_TABLE`, `ORCA_VERIFY_PAIR_URL`, `OPENCODE_GO_API_KEY`.
- Read by the Gem sync (2): `GCLOUD_DRIVE_CREDENTIALS_DB_B64`, `VM_HOST` (the latter also by any `wf offload ask --live`).
- Read only because a deploy leg verifies its service's group from the runner (20): `TS_VM_AUTH_KEY`, `TELEGRAM_BOT_TOKEN`, `TELEGRAM_NOTIFY_SECRET`, `LEETCODE_BACKEND_ADMIN_SECRET`, `LEETCODE_CAPTURE_TRIGGER_SECRET`, `VNC_AUTH_TRIGGER_SECRET`, `VNC_AUTH_OWNER_CHAT_ID`, `VNC_AUTH_DESKTOP_PASSWORD`, `VNC_AUTH_OWNER_NOTIFY_SECRET`, `T3_RCA_BEARER`, `ORCA_RCA_BEARER`, `CLAUDE_CODE_OAUTH_TOKEN`, `GROQ_API_KEY`, `KIMI_API_KEY`, `MINIMAX_API_KEY`, `OPENCODE_SERVER_PASSWORD`, `BETTERSTACK_OTLP_HOST`, `BETTERSTACK_OTLP_TOKEN`, `BETTERSTACK_HEARTBEAT_URL`, `VM_PROVISION_GITHUB_TOKEN`.

Does the union equal what the plan says (deploy-read 75 keys, CI-read 18 keys)? No, and it cannot be checked fully from the repo:

- Deploy-read project. A job on the runner reads 48 keys of it. `registry.json` has 80 keys that resolve through the deploy-read token (98 entries, minus 4 bootstrap tokens, 5 `gh-ci`, 8 `admin`, and `GITHUB_TOKEN`, which is an environment name, not a vault key); the plan measured 75 live. The 5 that the registry has and the vault does not are not named by the repo. The other 32 registry keys (27 if five are absent from the vault) are read by no job on the runner: Telegram client keys (6), the SSH keys, `OCI_*` (7), PagerDuty and budget alert settings, the restic and age keys, `ADB_ADMIN_PASSWORD`, `LIBS_WRITE_PAR_URL`, `LIBS_INDEX_WRITE_PAR_URL`, and the two legacy cache URLs (`PNPM_CACHE_PAR_URL`, `NX_REMOTE_CACHE_PAR_URL`, which no code resolves).
- CI-read project. Statically it needs 5 keys. The plan's 18 includes OCI, Expo, Docker Hub and the signing seed; no workflow that uses the CI-read token resolves any of them (`sm-action` names three Tailscale keys per workflow, and `wf` in `tailscale-auth-key-expiry.yml` finds its keys through the same token). So 13 of the 18 are either unused by this repo's workflows or reached through the read fallback (`Source::GhCi` first, then `Source::Read`, `clis/secrets/src/secrets/backend.rs:352-356`), which the CI-read token cannot satisfy for keys outside its project. The live listing of that project would settle which.

Findings that change the plan:

1. The `ci-open` / `ci-deploy` split cannot hold for the jobs the plan calls open until four things change. `plan` and `preflight` call vm-ops-api with the relay pair, and `preflight` writes a probe through the cache write URL on every event. Every cache-using job loads the cache write URL regardless of ref. Every `wf` command loads the ORDS password. The browser e2e job reads the gateway seed and bakes it into the export.
2. The plan's write-URL rule (U2.5: write URLs only for `push` to `dev`) is not what the code does. Write permission is decided from git facts (`clis/wf-bucket/src/cache.rs:68`), the write URL is loaded in every run, the bundle URL has no gate at all, and a `workflow_dispatch` of any ref runs the same code. **Changed by U2.5 (code only):** `wf secret` now refuses `BUILD_CACHE_WRITE_PAR_URL` and `VERIFICATION_ARTIFACTS_PAR_URL` in GitHub Actions unless `GITHUB_EVENT_NAME=push` and `GITHUB_REF=refs/heads/dev` (`clis/secrets/src/secrets/ci.rs`); the git-fact gate stays as a second check.
3. Finding 3b of the plan says no workflow step reads `VM_PROVISION_GITHUB_TOKEN` and that the only CI-side touch is the `wf ship` preflight of the `ci-runner` group. Both statements are off. The `ci-runner` group check lives in `wf vm terraform preflight` (`clis/deploy/src/infra/preflight.rs:112`), a recreate check run from agent-shell, not in `wf ship`. The CI-side read is `verify_for` of the `vm-ops-api` and `reporting-service` legs, whose optional group keys include `VM_PROVISION_GITHUB_TOKEN`; it is resolved on the runner and live-checked against `api.github.com/rate_limit`.
4. `VM_GATEWAY_AUTH_SEED` reaches the job from the runner's environment (not the vault) and is baked into the published EAS update and the web export as the backend token (`deploy.yml:886-888`, `e2e-tests.yml:232-234`). A hosted job has no such env, so U1.5's removal of it from the runner env means those jobs must take it from `ci-deploy`, and the value is in a client bundle either way.
5. `tailscale-rotate-auth-key.yml` references a GitHub secret `BWS_ADMIN_ACCESS_TOKEN`, an edit-capable vault token. The plan's Wave 5 and U3.1 do not list it.
6. The plan's line references moved: the `BWS` lines in the runner compose file are `:74-75`, not `:73-75`; `registry.json:242` is now `:228`; `docker-compose.prod.yml:265-266` of agent-shell, cited by `docs/plans/github-access-broker/plan.md`, is now `:79-82`. The `secrets: inherit` lines (`deploy.yml:931,945`) and the BetterStack reads (`deploy.yml:891-892`, `e2e-tests.yml:237-238`) match.
7. Hosted runners already exist for vault-reading jobs: the three `tailscale-*` workflows run on `ubuntu-latest` with the CI-read token. Anything the plan says about the first hosted run holds for them today.

## GitHub token beyond `GITHUB_TOKEN`

None found for any job. Every GitHub call in a workflow uses `github.token` or `secrets.GITHUB_TOKEN`, with these permissions: `deploy.yml` top level `actions: read`, `contents: read`, `pull-requests: write`; the `report` job adds `contents: write` and `issues: write`; `track-contracts.yml` capture adds `actions: write`; `ci-budget.yml` has `actions: write`. Two jobs may need more than they declare (open question 8). `VM_PROVISION_GITHUB_TOKEN` is read on the runner only as a verified key of two deploy legs (finding 3); the plan's owner decision (a dedicated fine-grained token, never this one) is untouched because no job turned out to need a repo token.

## `VM_PROVISION_GITHUB_TOKEN`: what the repo claims about its scope

The live scope is not verified here, and no agent should verify it: probing a credential was refused in an earlier session. The repo contradicts itself, in two camps.

Contents read only:

- `clis/secrets/src/data/registry.json:228` (the description): "fine-grained GitHub PAT scoped ONLY to Contents=Read on nuesslerm/expo-hello-world-app-2 -- no Write, no Pull requests, no Actions, no other repos".
- `infra/oracle-cloud/agent-shell/docker-compose.prod.yml:79`: "Same Contents:Read-only PAT vm-ops-api already uses".

Administration (and Actions) as well:

- `infra/oracle-cloud/ci-runner/docker-compose.prod.yml:65-66`: "The shared provisioning PAT; it needs Administration: read and write on this repo to register the runner", passed as `ACCESS_TOKEN`.
- `docs/runbooks/vm-recreate.md:19`: the runner image "mints a registration token from `VM_PROVISION_GITHUB_TOKEN` (needs Administration: read and write on the repo)".
- `docs/runbooks/health-alerts.md:17`: reporting-service reads GitHub with it, "Actions and Administration read are enough".
- `docs/plans/github-access-broker/plan.md:12,133-135`: asserts the runner uses it as an Administration:write token and that the Contents-only text is stale.

Both cannot be true of one fine-grained token. The runner's registration works today only if it carries Administration write, so the registry description and the agent-shell comment are the likely stale ones; but that is an inference, not a measurement. The live check needs the owner (the GitHub token settings page) or a CI step that asks GitHub for the token's permissions at the point of use (for a fine-grained token, a read of an endpoint that Administration alone permits and a refusal of one it must not, run in the container that already holds it, printing only the HTTP statuses).

**The probe exists now (code only, not yet run):** `wf vm ops token-permissions` asks GitHub with read-only GETs and prints only yes/no and the HTTP status per probe, plus the token's expiry date, never the token or a body: `/user` (is the token accepted), `/repos/{repo}` (the permissions block), `/repos/{repo}/actions/runners` (a 200 proves Administration: read) and `/repos/{repo}/contents/README.md` (Contents: read). It reads the token from the environment, so it runs where the token lives, on agent-shell: put `wf vm ops token-permissions` in a one-line script and run it with `wf vm exec <script>`. Administration: write, which the registration-token mint needs, cannot be proven by a read, so it is not probed; `wf ship ci-runner` is its proof. This is also the VM-side home of the check the deploy legs used to make on the runner: the `vm-ops-api` and `reporting-service` groups mark `VM_PROVISION_GITHUB_TOKEN` as `vmOnly` in `groups.json`, so `wf secret verify --for` and `wf secret scope-check` skip it, and `wf secret scope-check --json` reports no violation on dev.

## (3) Open questions

1. Deploy legs verify their service's whole group from the runner, so the runner reads 20 service runtime secrets that neither scope lists. Should `wf ship` verify them where they are used (vm-ops-api or agent-shell), so the CI scopes stay small, or does `ci-deploy` hold them? The plan's rule "keys neither scope needs stay in the VM project" does not hold for these until that moves.
2. Which jobs may write the shared cache from a non-trunk ref? Today's gate is a git fact, not the event; `preflight` writes a probe on every run. Is a split read link (everything) and write link (`push` to `dev` only) enough, or must `preflight` write nothing?
3. Run-bundle publish URL (the plan's own question): the bundle URL is used by `flow-tests`, `integration-tests`, `eas-update`, both e2e jobs, `ci-audit`, `track-contracts` and `record-references`, with no ref gate. As a `ci-open` key it lets any job write to the verification bucket. A third scope `ci-publish`, or moving publication to a step that runs only on `dev`, would decide it.
4. `HEALTH_REPORTS_ORDS_PASSWORD` is read by every `wf` command (usage row) in every job. It writes rows and is not read-only. Does it belong to `ci-open`, does a dedicated insert-only ORDS user replace it, or is the usage row switched off for hosted jobs (`WF_USAGE=off`)?
5. Should `preflight` and `plan` keep calling vm-ops-api? They are the first jobs of every run, and using the relay pair makes them `ci-deploy` jobs. A hosted open job cannot run them as written.
6. `VM_GATEWAY_AUTH_SEED` is baked into the EAS update and web export as a backend token. Is that intended as a long-term design, or should the backend accept a narrower per-client token? Either way it is the one `ci-deploy` key that a browser-e2e job (planned as open) needs.
7. Which live-vault names make up the CI-read project's 18 keys and the deploy-read project's 75? A names-only listing of each project (not values) by the owner or a vault-reading step would let this table's static union be compared exactly and would show which 13 of the 18 nothing reads.
8. `ci-budget.yml` runs `gh variable set` with `github.token` under `actions: write`. Whether `GITHUB_TOKEN` is allowed to do it is not proven by anything in the repo; if it fails, that job is the first that needs a dedicated fine-grained `ci-deploy` token.
9. Test code resolves keys by name at run time (`service-kit::e2e::setting`, the Orca deployed suite, the offload parity test). The 7 keys listed above are what the grep found; a suite added later widens the scope silently. The Wave 2 `wf` change (a key outside the job's scope fails naming the key) would catch it; until then, add a check to `wf check --budget` that no test calls `wf secret get` for a key outside the scope of its stage.
10. `record-references.yml` and `track-contracts.yml` run a provisioned reference's code on the VM runner and say "nothing the provisioned reference ran saw the vault", yet the runner container holds the deploy-read token in its environment. Whether `wf mimic job` scrubs it from the reference's environment is not shown here; it should be a flow test.
11. `VM_PROVISION_GITHUB_TOKEN`: the live scope (previous section). Owner action.

## Registry scopes assigned from this table (issue #367, U1.1)

Every `registry.json` entry carries a `scope` (`ci-open`, `ci-deploy`, `vm` for keys no CI job may read, `public` for repo variables); `wf secret verify --scope <ci-open|ci-deploy>` checks a token's project against it. Counts: `ci-open` 7, `ci-deploy` 44, `vm` 48, `public` 2 (the two scope tokens `BWS_CI_OPEN_ACCESS_TOKEN` and `BWS_CI_DEPLOY_ACCESS_TOKEN` are `vm` bootstrap entries: GitHub repo secrets, never read from the vault). Where this table leaves a key open the safer scope was taken (`ci-deploy` over `ci-open`), and a key no job reads is `vm`:

- Open questions 3 and 4 and the keys in neither plan list (taken as `ci-deploy`): `VERIFICATION_ARTIFACTS_PAR_URL`, `HEALTH_REPORTS_ORDS_PASSWORD`, `BETTERSTACK_METRICS_HOST`, `BETTERSTACK_METRICS_TOKEN`, `OPENCODE_GO_API_KEY`.
- Open question 1, the 20 group keys a deploy leg verifies from the runner (taken as `ci-deploy`, except `VM_PROVISION_GITHUB_TOKEN`, which the plan keeps out of both CI scopes: `vm`).
- Test and Gem-sync reads (`ci-deploy`): `CHAT_AGENT_API_SECRET`, `CH_BACKEND_*`, `ORCA_VERIFY_PAIR_URL`, `GCLOUD_DRIVE_CREDENTIALS_DB_B64`, `VM_HOST`.
- The plan lists OCI as `ci-deploy`, but no job reads an `OCI_*` key, so all seven are `vm`.

## The job classes as data: `job-scopes.json`

`clis/secrets/src/data/job-scopes.json` holds this document's table in machine-readable form: per `workflow-file:job-id`, a `class` (`open` or `deploy`, the lowest scope that holds every key the job reads today), the `keys` no step names, and the `groups` it verifies. `wf secret scope-check --json` reads it with the workflow files and exits 1 when an open job asks for a `ci-deploy` key or any job for a `vm` key. The class is `deploy` for every job that reads the ORDS password or a cache or bundle write URL, which is nearly all of them (open question 4 above); only `ci-budget` and `tag-gate` read no key and are `open`. On dev it reports one violation: the `deploy` job, whose legs `vm-ops-api` and `reporting-service` verify `VM_PROVISION_GITHUB_TOKEN`, a `vm` key. Change a job's reads and this file in the same commit.

## What `wf secret` does in CI (wave 2, code only)

- Token by scope: with `BWS_CI_OPEN_ACCESS_TOKEN` and/or `BWS_CI_DEPLOY_ACCESS_TOKEN` set in a CI run, a `ci-open` key is read with the first, a `ci-deploy` key with the second, a `vm` key is refused (`not readable in CI`), and a key whose token is not set fails naming the variable. With neither set it reads as before, so the VM runner's container-level token still works.
- Every resolved vault value is announced as `::add-mask::` in GitHub Actions (stdout for `sync-env`, stderr for the verbs whose stdout carries a value or JSON).
- The two `public` BetterStack values are read from the environment variables of the same name as the registry key, never from the vault when a scope token is set.
- The write URLs follow the event: `push` to `dev` only.

## Parity gate: `wf secret parity` (U4.2, code only)

`wf secret parity --json [--nonce <text>]` prints, per CI scope, the registry keys, the keys the scope's project holds, the missing ones, a hash of each name list and an HMAC-SHA256 (key name and value under the nonce) of every held value, never a value; it exits 1 when a scope lacks a registry key. A parity job runs it on the VM runner and on a hosted runner with the same `--nonce` (one run-scoped value both read) and a last step diffs the two outputs. The nonce is printed, so the hashes protect high-entropy values only. It reads each key with the token of its scope, so it runs under the CI token mapping above. Flow tests against a stub `bws` serving two projects: `clis/secrets/tests/flow/ci_scope.rs`.
