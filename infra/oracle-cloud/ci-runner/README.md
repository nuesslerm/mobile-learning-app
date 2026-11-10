# ci-runner

The GitHub Actions runner (label `vm`) in its own compose project, with its own Tailscale node `ci-runner-<host>` and its own volumes
(`ci-runner_tailscale-state`, `ci-runner_gh-runner-work`, `ci-runner_gh-runner-pnpm-store`). It was part of agent-shell; every agent-shell
deploy recreated it and killed the job that shipped agent-shell.

- **Isolation.** `docker compose` in `/home/ubuntu/agent-shell` and in `/home/ubuntu/ci-runner` act on different projects, so a redeploy of one never
  recreates a container of the other (`tests/compose-isolation.test.mjs`; the vm-ops-api redeploy case that runs both).
- **Builder sidecar.** The runner mounts no docker socket. Its `DOCKER_HOST` is `tcp://127.0.0.1:2376`, the `builder` service (docker-in-docker, mutual
  TLS, layer cache in the `builder-data` volume, client certificates in `builder-certs`, shared with the runner read-only). The `build-image` action
  (`wf ship build`) builds and pushes there; vm-ops-api only pulls and swaps (`tests/compose-builder.test.mjs`).
- **Shared volume.** `clis` (the one wf build) is written by the runner and mounted read-only by agent-shell and vm-ops-api. Both projects declare it
  `external: true, name: clis`; cloud-init and vm-ops-api's redeploy run `docker volume create clis` first.
- **Secrets.** Group `ci-runner` in `clis/secrets/src/secrets/groups.json`: `TS_VM_AUTH_KEY` (the shared reusable tailnet key, one node per sidecar). The runner's environment holds
  no `VM_OPS_API_AUTH_TOKEN` and no `VM_GATEWAY_AUTH_SEED`: jobs read the CI seed and the rest from the vault. The runner holds no PAT: `wf ship ci-runner` mints a one-hour registration token with
  `VM_PROVISION_GITHUB_TOKEN` (from agent-shell) and passes it as `RUNNER_TOKEN`; the registration persists in `gh-runner-config-3`.
- **Own tailnet tag.** `wf ship ci-runner` also mints a one-hour, single-use, preauthorized `tag:ci-self-hosted` key and sends it as `TS_CI_RUNNER_AUTH_KEY` (nothing stored); the sidecar uses `TS_CI_RUNNER_AUTH_KEY` and falls back to the shared `TS_VM_AUTH_KEY` when none was minted (the ship then prints one line saying so). The key matters only for the first join: the node's state persists in the `tailscale-state` volume, and a node keeps the tags it joined with. To move an existing runner node onto the new tag, wipe the volume on the next ship:
  1. Mint the key where the Tailscale credentials are readable (a one-hour, single-use, preauthorized `tag:ci-self-hosted` key through the Tailscale API: the request is `mint_runner_auth_key` in `clis/deploy/src/tailscale.rs`) and hand it to agent-shell in `TS_CI_RUNNER_AUTH_KEY`.
  2. From agent-shell: `TS_CI_RUNNER_AUTH_KEY=... wf ship ci-runner --ref dev --wipe-state` (the wipe removes `ci-runner_tailscale-state`, so the node joins afresh with the new key; the registration token is fresh anyway).
  3. Check the runner is `online` and listening (the old session can hold the first job for a few minutes).
  4. Retire the old device record, which now holds the bare hostname: `wf vm net device retire` (dry run first, then `--apply`), so the new node is not suffixed `-1`.
  Check with `wf vm net device list` that `ci-runner-<host>` carries `tag:ci-self-hosted`.
- **Deploy.** By hand: `wf ship ci-runner --ref dev` (no image to build; it redeploys). CI never deploys it, since a redeploy kills the running job.
  Order: deploy vm-ops-api first (push to `dev`, or `wf ship vm-ops-api --ref <sha> --redeploy-only --tag sha-<8>` for a tag CI pushed), since only the new one writes the `RUNNER_TOKEN` body into the runner's `.env`; an older one ignores it
  and the recreated runner has no credential. Then, from agent-shell, `wf ship ci-runner --ref dev`. A body with any name other than `RUNNER_TOKEN` and `TS_CI_RUNNER_AUTH_KEY` (a `tskey-auth-*` value, at most 160 characters) for this app is a 400.
  Verify on the VM (`wf vm ops`, no SSH):
  - `wf vm ops stats` lists `ci-runner-tailscale`, `ci-runner-builder` and `ci-runner-gh-runner` running, and the builder has no restart loop.
  - `wf vm ops logs ci-runner` shows the runner `Listening for Jobs`, and the runner is `online` in the repository's runner list.
  - Restart behavior with a spent token: restart only the runner container (`docker restart ci-runner-gh-runner` through a vm-ops-api sibling) and read the log;
    it must reuse the registration in `gh-runner-config-3` and listen again. If it fails on the spent token, the image re-registers on every start, so a restart
    means re-running `wf ship ci-runner --ref dev`; record that here.
  - From inside the runner, `docker info` with the runner's `DOCKER_HOST` (tcp to 127.0.0.1:2376, mutual TLS) answers the builder's daemon, not the host's, and there
    is no `/var/run/docker.sock` in the runner.
  - `env` in the runner has `RUNNER_TOKEN` only if the image keeps it, and no `ACCESS_TOKEN`, `BWS_*` or `VM_PROVISION_GITHUB_TOKEN`.

- **9 Nov finding.** Without `DISABLE_AUTOMATIC_DEREGISTRATION=true` the image deregisters the runner when its container stops (a registration token is set), and the next start reuses credentials of a runner GitHub no longer knows: the container exits at once and restarts forever. The first deploy hit this; the fix is the variable plus a fresh volume (`gh-runner-config-3`) so the next `wf ship ci-runner` registers again.
- **9 Nov, second finding.** Deleting the runner's record on GitHub (to clear a stale session) leaves the credentials in the config volume, and the image then loops on "The runner registration has been deleted from the server, please re-configure" because it skips registration while `.runner` exists. After deleting a record, rename the volume (`gh-runner-config-N`) so the next `wf ship ci-runner` registers fresh; `--wipe-state` does not touch it.
