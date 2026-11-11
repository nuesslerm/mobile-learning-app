# Public mirror with hosted runners: the simple design

Status: superseded on 2026-11-10 for the public repository: it runs the same pipeline as the private one on both a GitHub-hosted and a self-hosted runner, identically, with its own Actions secrets and the tailnet, kept safe by an owner-only branch ruleset and no pull request triggers (see `SECURITY.md`). The rest of this plan is the original design. Original status: proposed, 2026-11-08. Replaces `docs/plans/secrets-broker/plan.md` as the path to free GitHub-hosted compute (that plan and `docs/solutions/architecture/secrets-broker.md` are deferred, not deleted: they apply if a hosted runner ever has to deploy).

Update 2026-11-11: the repository is now `nuesslerm/mobile-learning-app`, public, with the vault token and the tailnet-join client as Actions secrets. The first push to `dev` is the end-to-end check of the GitHub-hosted pipeline on free public minutes.

Owner decisions (8 Nov): public mirror of the repo (the private repo stays canonical), any permissive license, hosted and VM runners both run CI, and no secret-handling service for now. Instead: **the mirror holds no privileged secret**. Hosted runners in the mirror run only jobs that need none; every privileged leg stays on the private repo's VM runner.

## Why this is enough
- A secret that is never present in the public repo cannot leak from it. Fork PRs get no secrets by default anyway; the mirror has nothing worth stealing even from a compromised workflow.
- The privileged legs (deploy, EAS publish, Tailscale, vault access) already run only on push to `dev` in the private repo, on the VM runner. Nothing about them changes.
- The one value the builds bake into the app, the BetterStack ingest token, is public by design (it ships in every app bundle). It becomes a plain GitHub **variable** in the mirror.

## Job classification (the core work)

Each job in `.github/workflows/deploy.yml` and `e2e-tests.yml` is either **open** (needs no privileged secret, no VM-local resource: may run on either runner kind) or **VM-only**. Verify each by running it with an empty environment, not by reading it.

Expected result, to be confirmed in Wave 1:
- Open: `plan`, `flow-tests`, `typecheck-lint-unit-tests`, `clis` build (without the publish step), `integration-tests`, `cli-e2e-tests`, `browser-e2e-tests` (builds its own web export; needs only the public BetterStack variables).
- VM-only: `deploy (<service>)` (vault, vm-ops-api, signing seed), `eas-update` (EAS token), `deployment-verification-tests` (real calls to the deployed services over the tailnet), `android-e2e-tests` (depends on the EAS Update and the VM-hosted device), record/replay jobs, `tailscale-*`, `vm-state-backup`, `repomix-gemini-context`, `tag-gate`.

## Waves

### Wave 1: make jobs runner-agnostic (private repo only, no visibility change)
- **U1.1** Classify every job (table in `docs/solutions/ci/open-and-vm-only-jobs.md`, derived by running each open job with no secrets and no tailnet; a job that fails names what it needed).
- **U1.2** Remove `secrets: inherit` from the two reusable-workflow calls in `deploy.yml` (L931, L945); pass only what each callee reads.
- **U1.3** Replace the two `wf secret get --backend bitwarden` calls for the BetterStack token and host (`deploy.yml` ~L891, `e2e-tests.yml` ~L237) with reads of repo variables `EXPO_PUBLIC_BETTERSTACK_TOKEN` and `BETTERSTACK_INGEST_HOST`, falling back to empty as today. Mark both keys `public` in `clis/secrets/src/data/registry.json` so the registry says why they are not vault secrets. Do not mint a new token: keep the current one for the private repo and use a separate, ingest-only token for the mirror.
- **U1.4** Make `runs-on` a per-job choice: `runs-on: ${{ fromJSON(vars.WF_RUNNER_OPEN || '["self-hosted","vm"]') }}` on open jobs, a literal `[self-hosted, vm]` on VM-only jobs. Private repo leaves the variable unset (no behavior change); the mirror sets it to `["ubuntu-24.04-arm"]` (matches the A1 ARM VM; `ubuntu-latest` as the x64 alternative).
- **U1.5** Gate the privileged legs on a repo variable instead of a repo name, so the mirror's workflow files carry no private identifiers: `if: vars.WF_CAN_DEPLOY == 'true'` on every VM-only job, set only in the private repo.
- **U1.6** Hosted-runner setup for open jobs: install pinned toolchain (`mise` pins; the repo's `install-wf` action builds on the VM, so add a hosted path that builds `wf` with the `deploy` profile and caches `target/` with `actions/cache`, 10 GB cap, 7-day eviction), Nx cache via `actions/cache` where the VM's blob-cache is unreachable. The `libs-proxy` registry (`.cargo/config.toml`) is tailnet-only: hosted runners cannot reach it, so open jobs that compile Rust need the private shared libraries (`libraries.json`) published somewhere reachable, or vendored; see open question 1.
- Gate for Wave 1: a full `dev` push is green on the VM runner as before; one dispatch with `WF_RUNNER_OPEN` set to a hosted label in the private repo (still private, so billed minutes) proves the open jobs pass on hosted runners before anything is public.

### Wave 2: audit and sanitize (read-only until the end)
- **U2.1** `gitleaks` and `trufflehog` over the full history; grep for `ocid1.`, the tailnet name and `.ts.net`, VM IPs, emails, bucket names, org and project ids. Findings list (keep / redact / drop) kept out of git.
- **U2.2** Review `docs/`, `.claude/`, `.agents/`, `.conductor/`, `.superpowers/`, `.local/` for personal data and infrastructure detail (`infra/tailscale/policy.hujson`, `infra/oracle-cloud/**`, `eas.json`, compose files, runbooks).
- **U2.3** Decide per file: ship, redact (placeholder, keep an `*.example`), or exclude. Exclusions go into an allowlist the export tool reads.
- **U2.4** Rotate anything that ever appeared real in history; assume it is exposed once a copy is public. Because the mirror starts from a fresh history (Wave 3), the old history is never published, but rotate anything the scanners flag anyway.

### Wave 3: the export tool and the mirror
- **U3.1** `wf` verb (new crate `clis/export` surfaced in the `wf` router; not a script): reads the allowlist and a replacement table, builds the public tree, and for each new private commit on `dev` creates a mirror commit (author and message kept after scrubbing) via `git filter-repo`-style rewriting from a sanitized baseline. Agent-first JSON output: files included, files excluded, replacements applied, scanner verdict. Flow tests with a fixture repo (an excluded file never appears; a replaced string never appears; the tool refuses to publish when the scanner finds a secret-shaped string).
- **U3.2** Private `.gitmodules`: drop the four `github-personal` SSH submodules from the exported tree; keep the public HTTPS reference submodules only if the build does not need them and their licenses allow.
- **U3.3** `LICENSE` (permissive; run `pnpm run audit` first and fix any dependency incompatibility), `SECURITY.md`, a public `README.md`.
- **U3.4** Export runs on push to `dev` (a VM-only job in the private repo) and pushes to the mirror with a deploy key held as a private-repo secret; the mirror has no secret of its own except the ingest-only BetterStack variable.

### Wave 4: mirror settings, then flip
Order matters: settings first, then the first push.
- Fork-PR approval required for all outside contributors; default `GITHUB_TOKEN` read-only; "allow Actions to create and approve PRs" off.
- Secret scanning with push protection, Dependabot alerts, private vulnerability reporting.
- Rulesets on `main` and `dev`: PR required, no force push, required checks.
- No Actions secrets. No self-hosted runner registered. Repository variables: `WF_RUNNER_OPEN=["ubuntu-24.04-arm"]`, the ingest-only BetterStack variables; `WF_CAN_DEPLOY` unset.
- Actions pinned by commit sha in every workflow; least-privilege `permissions:` per workflow and job; forbid `pull_request_target` with a checkout of the PR head (a check in `wf check --budget`).
- Add a secret-less `pull_request` workflow for outside PRs that runs the open jobs on hosted runners.
- First push of the exported tree; confirm Settings > Actions > Runners and Secrets are empty.

### Wave 5: verify
- Throwaway fork account opens a PR: only the secret-less workflow runs, on a hosted runner, and the VM runner receives no job (check the VM runner's job log).
- A fresh `git clone` of the mirror builds with no private access.
- `gitleaks` and `trufflehog` clean on the mirror's full history.
- Both runner kinds green on the same commit; record wall time per job on each in the run bundle (`wf test publish`) and compare.

## Limits to design for
GitHub-hosted: 20 concurrent jobs, 5 macOS, 6 h per job, ephemeral VMs, no persistent services, and the terms allow only building, testing and deploying this repo's software. Standard runners only (4 vCPU, 16 GB on public repos).

## Risks
- **Private shared libraries.** Rust builds read a private registry only reachable over the tailnet (`libs-proxy`); hosted runners cannot compile crates that depend on it (Wave 1.6, open question 1). This is the largest unknown.
- **Mirror drift.** One-way export means a mirror commit can differ from what CI ran privately; the mirror's CI results do not reach the private repo. Accepted: the private VM pipeline remains the gate for deploys.
- **A privileged job misclassified as open.** Mitigated by running each open job with no secrets and no tailnet before the mirror exists, and by the mirror holding no secrets at all.
- **Residual identifiers** in exported docs and configs. Mitigated by the allowlist, the replacement table, and the scanners in the export tool.

## Benchmarks
A new workflow lever and a new `wf` verb: one cheap run per runner kind of the open jobs, wall time and queue time, logged with the decision in `docs/audits/2026-09-02-token-diet-decision-log.md`. No model spend.

## Open questions
1. The private Rust shared libraries (`libraries.json`, served by `backend/libs-proxy` over the tailnet): vendor them into the exported tree, publish them to a public registry, or keep Rust-compiling jobs on the VM runner? Decide in Wave 1 after measuring which open jobs need them.
2. Hosted default: `ubuntu-24.04-arm` (matches the VM) or `ubuntu-latest` (x64, larger tool cache)?
3. Export cadence: per commit (this plan) or periodic snapshots?
