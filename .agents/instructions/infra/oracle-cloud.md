# Oracle Cloud VM, CI secrets, vault

Loaded on demand when working under `infra/oracle-cloud/`. The always-on rules live in the root `AGENTS.md`; this file carries the how-to for this area, moved here verbatim so the root stays small (known-gaps #51).

## Deploying code to the Oracle Cloud VM (no SSH)

A sandboxed agent session has HTTPS-only egress — no SSH to the VM. There is
**one** VM (`oci_core_instance.main`, host label `main`, 2 OCPU / 12 GB — the
whole Always-Free A1 allowance), running every app. The former `light`/`heavy`
pair is gone; see `infra/oracle-cloud/README.md`'s "Why one VM".
The loop: push to `dev`. The deploy leg of each app builds its image (and the sidecar images its
compose pulls on the same tag, e.g. `remote-chat-agent-stt`) in the CI job
(`wf ship build <app> --tag sha-<8>` through `.github/actions/build-image`, on the GitHub-hosted
runner, `linux/arm64`) and pushes them to Docker Hub, then runs
`wf ship --redeploy-only --smoke <app> --ref <sha> --tag sha-<8>`, which calls
`POST /services/<app>/redeploy?tag=&ref=` on `vm-ops-api` (bearer-token-gated, tailnet-only) to
pull+restart with the compose file and `.env.defaults` fetched from that ref. Nothing else builds
a production image: `vm-ops-api` has no build route and `wf ship` has no build option (a hand
build is `docker buildx` in agent-shell's own docker, never into a production tag). The images
a leg builds: `IMAGES` in `clis/pipeline/src/image_hash.rs`.
The GitHub Actions build-images/deploy-* workflows are gone: they emulated
arm64 under QEMU and could not be dispatched from an agent session. Full endpoint reference: `infra/oracle-cloud/README.md` section 10.

**An image build routinely takes well past 10 minutes — always run the build as a
background job, never as a single blocking foreground call.** A foreground
Bash call auto-backgrounds past its own timeout anyway, but by then you've
already lost the ability to choose *how* it backgrounds. Do it deliberately:
launch with `run_in_background: true` (or `Monitor` with a one-shot
`while kill -0 <pid>; do sleep N; done` loop) and check in periodically
instead of guessing a timeout long enough to cover a slow cross-build.
**Never pipe the build command through `| tail -N`** (without `-f`) when
doing this — `tail -N` buffers its *entire* input and only prints once the
upstream process exits, so a backgrounded `docker buildx build ... | tail
-60` produces a silently empty output file for the whole build, which reads
as "hung" when it is actually progressing normally. Redirect the raw output
instead (`docker buildx build ... > build.log 2>&1`) or follow it with
`tail -f` if you want a live stream; either way the log file actually shows
progress while it's running, not just a final snapshot.

**DOWNTIME ON THIS VM IS NEVER A CONCERN. This is settled and permanent — do
not re-raise it, do not ask about it, do not weigh it as a cost, do not hedge a
recommendation on it, and do not mention it as a caveat.** There are no users,
no SLA, and no traffic that matters. Stopping, rebooting, terminating,
recreating, detaching/reattaching boot volumes, taking every service down for
hours, or leaving the host down while you work on something else are all
pre-authorized, with no check-in and no warning needed. The user has stated this
repeatedly and explicitly; treating "but there will be downtime" as a reason to
pause, split a change into safer stages, or ask permission is a misread of this
repo, and the resulting question wastes a round-trip on a non-issue.

An agent that finds itself writing "this will cause downtime" should delete the
sentence and continue.

**What IS still a real constraint — and must not be confused with the above:**

- **Recoverability, not availability.** The operation must be one you can
  recover from *autonomously*. Don't leave the VM in a state that needs the user
  to intervene by hand. Being down is free; being *unrecoverable without a
  human* is the actual failure. That distinction is why `wf vm terraform apply`
  auto-rolls back and `wf vm recover` escalates to an OCI reboot.
  - **Worked example, the one that actually bites:** `terraform apply` cannot
    bring the VM back if the remote state does not describe it. When the state
    is empty or stale, terminating first strands you — not because of the
    outage, but because the *recreate* path is gone. Check state before
    destroying, then destroy freely.
- **Not spending money.** The Always-Free guardrail
  (`always-free-quota.tf`, `budget.tf`, `budget-guard.tf`) still binds. Anything
  that would incur real billing needs a check-in; anything free does not.
- **Not destroying the irreplaceable.** Vault secrets, the tfstate bucket's
  version history, and anything else with no reconstruction path are not covered
  by the downtime authorization.

Break-glass procedures without SSH — like swapping a stopped instance's boot
volume onto a rescue VM to edit files via that VM's own first-boot cloud-init —
are fair game. For that rescue path use `dummy-test-infra.tf`'s existing
E2.1.Micro box: it is on a separate compute pool and attaching the main VM's
boot volume to it as a DATA volume consumes no extra block storage, whereas
launching a throwaway VM needs a boot volume the 200 GB Always-Free storage cap
has no room for.

**Terminating and recreating a free-tier instance entirely is also explicitly
authorized, no check-in needed.** Not just stop/reboot — fully destroying
the main VM, or any other Always-Free-eligible (`VM.Standard.A1.Flex` /
`VM.Standard.E2.1.Micro`) instance and recreating it fresh via Terraform is
fair game whenever that's the simplest fix (e.g. baking a new SSH key into
`ssh_authorized_keys` from first boot, rather than performing boot-volume
surgery to inject one into a running instance). This does mean losing
whatever state isn't Terraform/cloud-init-managed on that instance (deployed
containers, `.env` files, logs) — no manual redeploy step is needed
afterward: `cloud-init.sh.tftpl`'s embedded `provision-apps.js` already
brings up every app in `local.vm_apps` (via the same secret-resolve +
`docker compose pull && up -d` mechanism `vm-ops-api`'s own `/redeploy`
uses) fully unattended on first boot. Manually re-running `docker compose
pull && up -d` yourself right after recreation just races cloud-init's own
provisioning — confirm via `GET /health` on the recreated host instead, and
only intervene if that doesn't come up within cloud-init's normal boot
window. This authorization is scoped to instances within the Always-Free guardrail
(`mobile-learning-always-free-only` OCI quota, see
`infra/oracle-cloud/README.md`) — anything that would incur real billing
still needs a check-in first.

**A recreate is `wf vm terraform preflight`, then `wf vm terraform apply --recreate`, never a raw `terraform -replace`.** The preflight (JSON, exit 0 only when all pass) checks the state backup, every secret group of `vm-apps.json`, that no CI run is in flight, and prints the recreate's plan with the actions per resource, failing on any destroy besides the VM and the `libs_read` PAR. The instance ignores `metadata` and `source_details[0].source_id` changes (a new Ubuntu image or cloud-init edit must never plan a replacement); the recreate still takes the newest image. Block storage is at its 200 GB Always-Free cap, so the boot volume cannot be preserved: what is lost and how it returns, the steps, rollback and verification are in `docs/runbooks/vm-recreate.md`.

**The VM's sensitive state is backed up, so a recreate needs no backup worry.** The `stateVolumes` of each app in `vm-apps.json` (orca-state, leetcode-backend-data, vnc-auth-credentials, agent-shell's agent-workspace without its regenerable paths) are snapshotted nightly with restic into the versioned bucket `mobile-learning-vm-state` by `wf vm state backup` (incremental, encrypted with `VM_STATE_RESTIC_PASSWORD`, `forget --keep-last 14 --prune`; `.github/workflows/vm-state-backup.yml`). Each volume declares its `maxBytes`, measured from the file sizes before anything is read; the live repository is held to twice their sum and the bucket, previous object versions a prune leaves included, to four times (`vm-state.tf`, `budget.json`; a test keeps them equal), counted before and after each run, and the backup fails instead of growing; scratch (staged SQLite copies, verify restores) is agent-shell's `/scratch` tmpfs. `wf vm state verify` restores the newest snapshot of every declared volume into a scratch directory, runs `PRAGMA integrity_check` and tags it `verified`; `list` shows them; the recreate preflight refuses a backup over 24 hours old or one that does not verify. A fresh VM's state volumes are restored by cloud-init (`restore-state.js`, host restic with the instance principal) from the newest verified snapshot **before any app starts**, and an app whose volume fails to restore is not started (transient failures retried, the IAM policy waited for up to 15 minutes); agent-shell's workspace is restored only there; `wf vm state restore` only fills an empty volume of a stopped app and has no `--force`. Every app creates its first-start files only when missing (a flow test per app proves it). The verbs run on agent-shell through vm-ops-api's exec route (it holds the OCI key and mounts the volumes); a CI job (`WF_VM_CI_PORT`, the nightly `vm-state-backup.yml` on a GitHub-hosted runner) instead asks the CI port to start the one fixed backup or verify script (`POST /state/<verb>`, `GET /state/jobs/<job>`), which fetches `vm-apps.json` from `dev` itself, so CI can start nothing else on agent-shell. A new state volume that must survive a recreate is a `stateVolumes` entry with a `maxBytes`, mounted in agent-shell's compose file. Tailscale node state is deliberately not backed up: a stale identity collides on the new VM, and nodes re-authenticate with `TS_VM_AUTH_KEY`.

**This authorization is not specific to the VM — it covers `terraform apply`/
`plan`/`destroy` for ANY resource in `infra/oracle-cloud/*.tf` whose shape is
within the Always-Free envelope** (an Autonomous DB at `is_free_tier = true`,
a `VM.Standard.A1.Flex`/`E2.1.Micro` instance, budgets, policies, quotas,
notification topics — anything Terraform itself, or the `always-free-quota.tf`
guard, would reject if it exceeded the free envelope). Running the apply is
not a decision that needs to be handed back to the user first, and a session
that hits friction running one should retry and investigate the *actual*
error before assuming it's blocked: `wf vm terraform apply <dir>
<compartmentId>` occasionally fails a single attempt with a plain `spawnSync
terraform ENOENT` for transient reasons unrelated to permissions (confirmed:
identical retries with no code change succeeded), and this has previously
been misreported in `docs/known-gaps.md` as a hard, permanent block when it
was not one — always re-run before filing or repeating that claim. The only
things that still warrant a check-in are the same two carved out above:
anything that would incur real billing, and anything destroying state with no
reconstruction path (vault secrets, tfstate history).

## CI secrets live in Bitwarden, not in GitHub Actions secrets

**A workflow that needs a secret resolves it from the vault at run time via
`bitwarden/sm-action@v2`, keyed by the secret's UUID — it does NOT read
`secrets.<NAME>`.** The `mobile-learning-gh-ci-secrets` Bitwarden project
(`b9781a83-92dc-4d99-94f3-b4930106ba6d`) is the source of truth;
`DOCKERHUB_USERNAME`/`_TOKEN`, `VM_HOST`, and `VM_GATEWAY_AUTH_SEED` live there
(CI derives the day's rotating gateway token from `VM_GATEWAY_AUTH_SEED`
itself, `HMAC-SHA256(seed, today's UTC date)` as `clis/wf-vmops/src/vmops.rs` does,
rather than holding a raw static credential). The step
looks like this (note the explicit EU URLs — the action defaults
to the US cloud, which rejects an EU token with a bare `invalid_client`):

```yaml
- uses: bitwarden/sm-action@v2
  with:
    access_token: ${{ secrets.BWS_CI_DEPLOY_ACCESS_TOKEN }}
    base_url: https://vault.bitwarden.eu
    api_url: https://vault.bitwarden.eu/api
    identity_url: https://vault.bitwarden.eu/identity
    secrets: |
      <uuid> > SOME_SECRET
```

Consume it as `${{ env.SOME_SECRET }}` in later steps.

**Only two GitHub Actions secrets remain, and both are bootstrap credentials
that cannot come from the vault** (they are what opens the vault, or what the
vault cannot carry): `BWS_CI_DEPLOY_ACCESS_TOKEN` and
`DEPLOY_SSH_KEY`. Everything else belongs in Bitwarden.

**WHY THIS MATTERS TO AN AGENT SESSION:** the egress proxy 403s
`api.github.com/repos/*/actions/*`, so no agent can ever create or rotate a
GitHub Actions secret — but `BWS_ADMIN_ACCESS_TOKEN` writes to
the vault fine, and it can see the CI project (verified: `bws project list`
shows `mobile-learning-deploy-secrets`, `mobile-learning-gh-ci-secrets`,
`mobile-learning-admin-secrets`). So a value that changes — like `VM_HOST`,
which tracks an ephemeral VM IP that changes on every recreate — is
self-serviceable here, whereas as a GitHub secret it would be a standing manual
chore for the user after every rebuild. Adding one:

```bash
BWS_ACCESS_TOKEN="$BWS_ADMIN_ACCESS_TOKEN" \
  bws secret create NAME "value" b9781a83-92dc-4d99-94f3-b4930106ba6d -o json
```

then reference the returned `id` in the workflow's `sm-action` block. This was
written after a session repeatedly asked the user to run `gh secret set VM_HOST`
— which is neither possible from here nor the pattern this repo uses.

## Secrets policy (repo-wide)

All API keys and secrets live in the vault (the macOS Keychain locally, Bitwarden in
production and CI) behind `wf secret`, never in a committed file. The env-file
convention (see the root `.gitignore`):

- **`.env.defaults`** — committed, per-package, **non-secret config only** (ports, URLs, modes,
  file paths). The source of truth; each carries a
  header comment listing the vault secrets that package needs + its `--for` group. Replaces the old
  committed `.env` / `.env.example`.
- **`.env`** — **generated at deploy** = `.env.defaults` ⊕ vault secrets, via
  `wf secret sync-env --base .env.defaults`. **Gitignored** (it may hold secrets).
- **`.env*.local`** — gitignored per-machine overrides (highest precedence, appended last).

The pre-commit `secrets` job blocks any registry **secret** key from being committed in a
`.env.defaults`. `clis/secrets/src/data/registry.json` (compiled into wf) is the source of truth for which
secret keys exist — add an entry there (key → backend `service` + `envVar` + description) before
introducing a new secret. Two different env var names that must carry the *same* secret value
(e.g. a shared trigger/notify secret between two services) can point at the same `service` string
in the registry — one `wf secret set` populates both.

A Rust service reads its environment; `service_kit::secrets::load()` asks the vault through wf for any
registered variable its container was not given. `wf secret` has two backends, chosen with `--backend <name>` /
`SECRETS_BACKEND`: `macos` (the local default, via the `security` CLI, the only writable one) and the read-only
production backend `bitwarden` (`bws`, `BWS_ADMIN_ACCESS_TOKEN`). Production hosts compose the runtime
`.env` via `wf secret sync-env --for <app> --backend <name> --base .env.defaults --out .env` immediately before each
deploy (committed non-secret base, then vault secrets, then `.env.local` overrides last).

Where each secret comes from: its registry description (`wf secret list`).

Manage entries via `wf secret list|get|set|delete|env` (`clis/secrets/src/secrets/`). If a task needs a secret that
isn't registered yet, add the registry entry and wiring, then ask the user to run
`wf secret set <KEY>` with the real value — never fabricate a value or fall back to writing it
into a `.env` file.

## CLI auth capture (`clis/vnc-auth-capture`, `wf secret login`)

One universal mechanism to perform an interactive browser login for any coding
CLI (claude, codex, opencode, gemini) or site login (leetcode), capture the
credential, and — when the agent is headless — surface the login to a human via
Telegram through a real kiosk-mode login browser on a bare virtual screen that the listener's own phone-sized viewer streams as JPEG frames,
fronted by `tailscale serve` (tailnet-only). The code is the Rust `wf` (`clis/wf-login/src/vnc/`); `clis/vnc-auth-capture`
holds the image. Local: `wf secret login <tool>` (native browser). Remote: the
auth-session listener (`wf secret login listener`'s code, built alone as `vnc-auth-listener`
from `clis/vnc-auth-capture/listener`, so the image carries no `wf`) in its container
(`docker-compose.auth.yml`) + `wf secret sync-env --for vnc-auth-session`.
Security controls (one-time tokened link, owner allowlist, loopback bind, ACL
scoping) — `clis/vnc-auth-capture/README.md`.

## Anthropic credential policy (opencode ↔ claude)

**opencode never holds an Anthropic credential of any kind** — no
`ANTHROPIC_API_KEY`, no `ANTHROPIC_AUTH_TOKEN`, no `apiKeyHelper` shim. Anthropic
models are reached **exclusively** through the `claude` CLI (no service of this repo
drives it any more), authenticated by a subscription credential
captured via `wf secret login claude` / the `vnc-auth-capture` flow above — never
by a per-token key on the opencode path. This is a standing constraint, not a
preference to re-weigh per change.

The deployed `backend/remote-chat-agent` is the Rust service `rca-service`
(`backend/remote-chat-agent/rs`), and it serves opencode alone: it reaches the
opencode server over HTTP, never spawns it and never reads an Anthropic
credential, so the constraint holds by construction. The startup check
(`rs/src/startup.rs`) refuses to start, by name, when `AGENT_CLI` names a backend
it does not serve (`claude`, `codex`) or is not a backend at all. Neither gateway exists any more: the TypeScript ones
(`claudeGateway.ts`, `codexGateway.ts`) were removed with the rest of the Node
service, and no Rust gateway drives either CLI. The opencode runs of `wf offload ask`
and `wf offload plan` (which runs `wf offload ask` per unit) strip `ANTHROPIC_API_KEY` and
`ANTHROPIC_AUTH_TOKEN` from the `opencode` child's environment (black-box test
"wf offload ask and the Anthropic credential policy").
