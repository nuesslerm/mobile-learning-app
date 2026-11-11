---
title: "vm-ops-api deploy fails with Docker manifest arch mismatch — arm64 VMs, single-arch build-and-push.sh"
date: 2026-07-30
category: build-errors
module: infra/oracle-cloud/vm-ops-api
problem_type: build_error
component: tooling
severity: high
symptoms:
  - "docker compose pull fails with: Error response from daemon: no matching manifest for linux/arm64/v8 in the manifest list entries: no match for platform in manifest: not found (image: nuesslerm/vm-ops-api:latest)"
  - "docker manifest inspect nuesslerm/vm-ops-api:latest shows the manifest list contains only an amd64 platform entry (plus a benign unknown/unknown attestation manifest)"
  - "Both production Oracle Cloud VMs (light=203.0.113.11, heavy=203.0.113.12) are arm64 (Ampere A1 shape; uname -m -> aarch64, docker server arch -> arm64), so deploy.sh fails identically on both hosts"
  - "Failure only appears after build-and-push.sh was last run from a non-arm64 (e.g. amd64) machine, silently overwriting the :latest tag the arm64 VMs depend on"
root_cause: config_error
resolution_type: environment_setup
related_components:
  - build-and-push.sh
  - deploy.sh
  - docker-compose.yml
tags: [docker, multi-arch, arm64, oracle-cloud, buildx, docker-hub, deploy]
---

# vm-ops-api deploy fails with Docker manifest arch mismatch — arm64 VMs, single-arch build-and-push.sh

## Problem

Redeploying `vm-ops-api` to either Oracle Cloud VM (`light` = 203.0.113.11, `heavy` = 203.0.113.12) via `infra/oracle-cloud/vm-ops-api/deploy.sh` failed at the `docker compose pull` step, because the `:latest` image on Docker Hub had been silently overwritten with an amd64-only build by vm-ops-api's `build-and-push.sh` (deleted with the Node service in Oct 2026), which had no multi-arch build step.

## Symptoms

Running `deploy.sh` against the light VM (`VM_HOST=ubuntu@203.0.113.11 HOST_LABEL=light OPS_API_DOMAIN=203.0.113.11.sslip.io ./deploy.sh`) failed during the remote `docker compose pull` invoked at `infra/oracle-cloud/vm-ops-api/deploy.sh:100` (the line number at the time; it is line 98 today), with:

```
Error response from daemon: no matching manifest for linux/arm64/v8 in the manifest list entries: no match for platform in manifest: not found
```

for image `nuesslerm/vm-ops-api:latest`.

## What Didn't Work

- **Assumed the failure was transient / re-run worthy.** It was not — the error is deterministic given the pushed manifest, and reproduces identically on every subsequent `deploy.sh` run against either VM until the image itself is fixed.
- **Suspected this sandbox lacked real SSH access to the VMs**, based on a stale claim in a prior runbook handoff. Disproved directly: `ssh ubuntu@203.0.113.11 echo ok` succeeded without issue from this environment, so SSH access was never the blocker.
- **Suspected the `wf` CLI bundle build was stale or the vault token was missing.** `deploy.sh` depends on an executable `wf` (`$WF_BIN`, checked at `infra/oracle-cloud/vm-ops-api/deploy.sh:29-30`; a JS bundle at the time, now the Rust wf built from `clis/wf/src/main.rs`) and on resolving `BWS_ADMIN_ACCESS_TOKEN` (`infra/oracle-cloud/vm-ops-api/deploy.sh:48-64`). Both were confirmed already fine in this environment (bundle cache hit, token already in the shell env) — neither was the cause of the pull failure, which happens downstream on the remote VM at the `docker compose pull` step, unrelated to local bundle/token state.

## Solution

1. SSH'd into both VMs and confirmed CPU architecture and Docker daemon architecture:
   - `uname -m` → `aarch64` on both `light` and `heavy`.
   - `docker version --format '{{.Server.Arch}}'` → `arm64` on both — confirming both are Oracle Cloud Ampere A1 (ARM64) shapes.
2. Checked this environment's own Docker: the client is `darwin/arm64`, but the Docker *daemon* it talks to reports `linux/arm64` — i.e., the sandbox's Docker daemon is itself an arm64 Linux VM.
3. Ran `docker manifest inspect nuesslerm/vm-ops-api:latest` and found the manifest list contained only a single real platform entry, `"architecture": "amd64"` (plus one benign `unknown/unknown` attestation/provenance manifest from a prior buildx run — not a second real platform, and not something to mistake for arm64 support).
4. Read `build-and-push.sh` and confirmed it invoked, at the time, a plain, single-architecture `docker build` — no `buildx`, no `--platform` (line numbers below are the pre-fix file's):
   - Line 70 (default path): `docker build "${BUILD_ARGS[@]}" -t "${IMAGE}:${TAG}" -t "${IMAGE}:latest" .`
   - Line 67 (sandbox-proxy CA-trust variant, same shape): `docker build "${BUILD_ARGS[@]}" -f - -t "${IMAGE}:${TAG}" -t "${IMAGE}:latest" . <<< "$DOCKERFILE_CONTENT"`

   Whatever machine last ran this script built and pushed `:latest` for *that machine's own* architecture (evidently amd64), silently clobbering whatever arch had been there before — this is the origin of the amd64-only manifest found in step 3.
5. Applied a one-off workaround (not a durable code fix): ran `./build-and-push.sh` (no args, so `TAG` auto-derives to `sha-<git-short-sha>` per `build-and-push.sh:19` at the time, pushed alongside `:latest` by the separate `docker push` calls then at lines 74-75) from this same sandbox environment, whose Docker daemon happens to already be `linux/arm64`. This rebuilt and pushed a fresh arm64-only image as `nuesslerm/vm-ops-api:latest`.
6. Re-ran `docker manifest inspect nuesslerm/vm-ops-api:latest` and confirmed the new manifest's platform is `"architecture": "arm64"`.
7. Re-ran `deploy.sh` for both hosts:
   - `VM_HOST=ubuntu@203.0.113.11 HOST_LABEL=light OPS_API_DOMAIN=203.0.113.11.sslip.io ./deploy.sh`
   - `VM_HOST=ubuntu@203.0.113.12 HOST_LABEL=heavy OPS_API_DOMAIN=203.0.113.12.sslip.io ./deploy.sh`

   Both now pulled the new arm64 image successfully at the `docker compose pull` step (`deploy.sh:100` at the time, line 98 today), and `docker compose up -d` (`deploy.sh:102` then, line 100 today) recreated and started the `ops-api` container on each host.
8. Verified with the Bearer-token-protected health check (`deploy.sh:107`'s printed command at the time, a direct `curl` to `https://${OPS_API_DOMAIN}/health` with the `VM_OPS_API_TOKEN` bearer; today line 105 prints a vm-gateway Funnel `curl` authorized with `VM_GATEWAY_AUTH_SEED` instead): `GET /health` returned `{"ok":true,"host":"light",...}` from the light VM and `{"ok":true,"host":"heavy",...}` from the heavy VM.

This fix is a manual, uncommitted rebuild/push performed directly against Docker Hub from this session — it changed no files in the repository and is not associated with any merged PR. `build-and-push.sh` itself was left unmodified, so at that point **the bug was not actually fixed**; see Prevention for the later fix.

**Update (same day, later session):** the bug reproduced exactly as predicted — a subsequent sandbox session had an amd64 Docker daemon, ran `build-and-push.sh` with no code changes, and silently overwrote `nuesslerm/vm-ops-api:latest` with an amd64-only manifest again, breaking `/redeploy` on both VMs (caught immediately via `GET /redeploy/status`, itself added earlier that day — see `docs/archive/solutions/infra/vm-code-deployment-flow.md`). Both `build-and-push.sh` and `scripts/build-and-push-image.sh` were then fixed per this doc's own Prevention section, scoped to `--platform linux/arm64` specifically (not full multi-arch) since nothing consumes an amd64 build of either — a `TARGET_PLATFORM` env var override is available if that ever changes. Both scripts now pass `--platform "${TARGET_PLATFORM:-linux/arm64}"` unconditionally, so the build host's architecture no longer matters. Verified end-to-end: rebuilt+pushed `vm-ops-api` from an amd64 sandbox host (using `tonistiigi/binfmt --install arm64` for QEMU emulation), confirmed `docker manifest inspect` showed `"architecture": "arm64"`, and redeployed successfully on both VMs.

## Why This Works

Both production VMs were Oracle Cloud Ampere A1 (ARM64) instances (today the single `main` VM, also A1). `docker compose pull` (`deploy.sh:100` at the time, line 98 today) requests whatever platform matches the local Docker daemon on the VM — arm64 — from the `:latest` tag on Docker Hub. `build-and-push.sh`'s `docker build` calls at the time (lines 67 and 70 of the pre-fix file) were plain single-architecture builds: they produced and pushed an image matching only the architecture of the host that ran the script, and always retagged it `:latest` (`-t "${IMAGE}:latest"` on both lines), unconditionally overwriting whatever arch was previously live there (the script has since been pinned to arm64; see the Update above and Prevention). The prior `:latest` push had evidently come from an amd64 host, so its manifest had no arm64 entry — hence Docker's generic "no matching manifest for linux/arm64/v8" error on both VMs. Rebuilding from a host whose daemon is itself arm64 (this sandbox) produced and pushed an arm64 manifest, which is why the same `deploy.sh` invocation succeeded immediately afterward with no other code change.

**This repo already had the correct pattern for this — just not wired up here.** As of 2026-07-30, `.github/workflows/build-images.yml` built four sibling backend services (`remote-chat-agent`, `opencode-telegram-bot`, `leetcode-backend`, `leetcode-auth-cookie-capture`) via `docker/setup-qemu-action` + `docker/setup-buildx-action` + `docker/build-push-action`, with `platforms: ${{ github.event_name == 'push' && 'linux/amd64,linux/arm64' || 'linux/amd64' }}` (`build-images.yml:246-286` at the time) — multi-arch on every push to the default branch, amd64-only (faster) on PR builds. `vm-ops-api` was not in that workflow's matrix at all then; it had its own separate, manual `build-and-push.sh` that had not adopted this convention. Later, before the workflow was removed, `vm-ops-api` was added to its matrix as an arm64-only (`platforms: linux/arm64`), `always_build` entry alongside further images, and the platforms expression became `matrix.platforms || (needs.plan.outputs.should_push == 'true' && 'linux/amd64,linux/arm64' || 'linux/amd64')`. The former `.github/workflows/build-images.yml` was removed in 218533389 (#299), when image builds moved onto the VM via wf's then-new `agent-shell build` and `agent-shell release` verbs (gone since); today they ship through `wf ship` in `deploy.yml`. `docs/archive/solutions/infra/deployment-provisioning.md` (~lines 423-428) and `infra/oracle-cloud/README.md` (lines 46-48) both already describe "the deploy workflows cross-build linux/arm64 images via buildx" as if it covers all Oracle Cloud deployments — at the time, that statement was accurate for the four CI-built services and silently false for `vm-ops-api`, which is exactly where this bug occurred. Today every image is built as `linux/arm64` on the VM: `wf ship` builds the others with `docker buildx build --platform linux/arm64` in `clis/deploy/src/build_image.rs`, while `wf ship vm-ops-api` triggers its self-redeploy (`POST /redeploy?ref=`, `clis/deploy/src/ship.rs`), which builds it with `docker buildx build --platform linux/arm64 --push` (a plain `docker build` and `docker push` until 22 Oct 2026) in `self_build.rs` (since deleted: `wf ship` builds) (its `VM_BUILDABLE_IMAGES` entry serves only `--build-only`).

## Prevention

**Make `build-and-push.sh` multi-arch, following the pattern `build-images.yml` had established**, so the builder's own host architecture never matters again. This is done, in the narrower arm64-only form the Update above describes: `TARGET_PLATFORM` defaults to `linux/arm64` (`build-and-push.sh:56`) and is overridable, `BUILD_ARGS` carries `--platform "${TARGET_PLATFORM}"` (line 61), and each branch is now one `docker buildx build ... --push` (line 155, the sandbox CA-trust variant, which builds a patched Dockerfile via `-f "$DOCKERFILE_TO_BUILD"`; line 158, the default path), with no separate `docker push` calls. The original proposal, against the pre-fix file, follows. Its two single-arch `docker build` calls should become a single `docker buildx build --platform linux/amd64,linux/arm64 ... --push` invocation. For example, the default (non-sandbox-proxy) branch at `build-and-push.sh:69-70` read, before the fix:

```bash
echo "[build-and-push] Building ${IMAGE}:${TAG}..."
docker build "${BUILD_ARGS[@]}" -t "${IMAGE}:${TAG}" -t "${IMAGE}:latest" .
```

and was proposed to become something like:

```bash
echo "[build-and-push] Building + pushing ${IMAGE}:${TAG} (linux/amd64,linux/arm64)..."
docker buildx build "${BUILD_ARGS[@]}" --platform linux/amd64,linux/arm64 \
  -t "${IMAGE}:${TAG}" -t "${IMAGE}:latest" --push .
```

Note this isn't a one-line flag addition: per Docker's own buildx documentation (not something asserted elsewhere in this repo), a multi-platform buildx result is a manifest list that can't be loaded into the local Docker image store the way a single-arch `docker build` output can, so `--push` is mandatory as part of the build step itself. That meant the pre-fix two-step flow — `docker build` (lines 67/70 then) followed by separate `docker push` calls for `:TAG` and `:latest` (lines 74-75 then) — needed to be restructured into one combined build-and-push invocation per branch (the sandbox-proxy CA-trust variant, then at line 67, needed the same `buildx --platform ... --push` treatment; it then used a `-f -` heredoc Dockerfile, and today passes a patched Dockerfile file instead).

**Add a fast pre-deploy arch-compatibility check to `deploy.sh`.** Before the `docker compose pull` (line 98 today; line 100 at the time), run `docker manifest inspect` on the target image/tag and compare its available platforms against the VM's own `uname -m` / `docker version --format '{{.Server.Arch}}'` (already known to be `aarch64`/`arm64` for the VM: `main`, Ampere A1; `light` and `heavy` at the time of this incident). Failing fast with an explicit message like "image nuesslerm/vm-ops-api:latest has no arm64 manifest (found: amd64) — rebuild with build-and-push.sh's multi-arch buildx path" would have saved the entire diagnostic trail above; Docker's own "no matching manifest for linux/arm64/v8" error names neither the arch it needed nor the arch it found, which is what made this genuinely non-obvious to root-cause on first encounter.

**Fix the stale docs that describe the arm64 build.** The script itself is already pinned to arm64 (see above). `docs/archive/solutions/infra/deployment-provisioning.md` (archived since, so only `infra/oracle-cloud/README.md` is still a live doc to change) and `infra/oracle-cloud/README.md` both describe the arm64-via-buildx pattern as covering "the deploy workflows" generally. The one remaining stale mention in a live doc is `infra/oracle-cloud/README.md` lines 46-48, which says to change `--platform` in `.github/workflows/deploy-*.yml`, workflows that no longer exist. It should say instead that images are built as `linux/arm64` on the VM, by `wf ship` (`clis/deploy/src/build_image.rs`) and, for vm-ops-api, by its own `selfBuild.ts`, and that `build-and-push.sh` takes a `TARGET_PLATFORM` override.

## Related Context

- **(session history)** The `build-and-push.sh` + `deploy.sh` pipeline itself was only built and first exercised the day before this bug (2026-07-29, same branch), and pulled successfully on both VMs at that time with no manifest error — so the amd64-only push that caused this bug happened sometime *after* that session, from a different (non-arm64) machine or context. No prior session shows this exact failure; this is a fresh regression against a day-old pipeline, not a repeat of a previously-hit bug.
- **(session history)** A different bug in the same file was fixed in this window: `POST /redeploy`'s self-recreate raced with its own container teardown, because the recreate was spawned from *inside* the container being killed. Fixed by launching the recreate as a sibling container over `docker.sock` (then `buildRedeploySiblingArgs` in `server.js`, now `self_redeploy_sibling_args` in `backend/vm-ops-api/core/src/application/scripts/redeploy.rs`), part of PR #175 ("Extend vm-ops-api to production apps + CLI bundle unification"), open/unmerged as of this writing. Worth knowing `server.js` (since deleted; its redeploy logic now lives in `src/application/scripts/redeploy.ts`) was actively changing in this same period when reasoning about current file state.
- At the time, `VM_OPS_API_TOKEN` was a single bearer token shared by `light` and `heavy` for the PoC phase. It has since been renamed `VM_GATEWAY_AUTH_SEED` (`clis/secrets/src/data/registry.json`), vm-ops-api's own routes check a separate `VM_OPS_API_AUTH_TOKEN`, and there is one VM (`main`).
