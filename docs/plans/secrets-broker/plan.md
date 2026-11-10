# Secrets broker: implementation plan

Status: deferred 2026-11-08 (superseded by `docs/plans/public-mirror/plan.md`: no broker for now); written as proposed 2026-11-08. Decision record: `docs/solutions/architecture/secrets-broker.md`. Inventory of today's consumers: `docs/solutions/secrets/consumer-inventory.md`. Owner decisions (8 Nov): public mirror of the repo, hosted and VM runners equal, every secret behind the broker, broker work first and independent of the mirror, `build-env` signatures verified by `wf` (not by each workflow).

## Reconciling with the GitHub access broker plan

`docs/plans/github-access-broker/plan.md` (proposed 2 Nov) puts an allowlisting broker for GitHub power inside `vm-ops-api`, with a GitHub App key as the one broad secret. Same idea, one credential class. Decision for this plan: the secrets broker is the one broker; the GitHub App key becomes a vault secret only it holds, and "mint a narrowed installation token for caller X" is an operation class (`github-token <scope>`) in its catalogue, reusing that plan's per-caller allowlists as policy rows. Landing order: the GitHub access plan's own landing 0 (inventory of today's PAT) is unaffected; its broker unit is replaced by Wave 5 here. **Needs the owner's confirmation before Wave 5** (open question 1).

## Units

Each unit is 1-3 files of intent, lands on its own with its gate green, and is verified at the tier named. Waves run in order; units inside a wave are independent unless noted. Rust gates run as `flock /tmp/cargo.lock <command>`; units in the same crate run one after another.

### Wave 0: foundations (no behavior change)
- **U0.1** Dead-reference and registry prep: add a `public: bool` field to every entry of `clis/secrets/src/data/registry.json` (default false; true only for the `EXPO_PUBLIC_*` keys and `BETTERSTACK_INGEST_HOST`), with the schema and its loader in `clis/secrets/src/secrets/registry.rs`. Gate: unit + flow of `clis/secrets`. Evidence: a registry test reads the file and asserts every `EXPO_PUBLIC_*` key is public and nothing else is (derived from the file, not copied).
- **U0.2** Remove `secrets: inherit` from the two reusable-workflow calls in `.github/workflows/deploy.yml` (L931, L945); pass only what each callee reads. No broker needed; cheapest exposure cut. Gate: `actionlint`, a CI run of `deploy.yml` (dispatch) green. Verification: CI run summary.
- **U0.3** Scaffold `backend/secrets-broker/{core,rs}` modelled on `backend/vm-ops-api` (Cargo workspace members in `backend/Cargo.toml`, `project.json`, Dockerfile, `/health`), empty domain. Gate: `pnpm run arch:services`, build, `GET /health` flow test. Deploys as a no-op service.

### Wave 1: pure core (domain, no I/O)
- **U1.1** Policy types and evaluation in `core/src/domain/policy.rs`: `evaluate(identity, operation, presented_pins, policy) -> Allow | Deny(reason)`; parse and schema-validate the policy file. Unit tests, Given/When/Then, one per deny reason (unknown operation, wrong repo, wrong ref, `pull_request` event, workflow sha mismatch, `wf` hash mismatch, missing pin, empty policy fails closed).
- **U1.2** Operation catalogue types in `core/src/domain/operation.rs` (name, argument schema, result schema, which secret keys it may touch, whether it needs a pin set) plus result scrubbing `scrub(output, touched_values)` in `domain/scrub.rs`. Unit tests: scrubbing removes a value in plain, quoted, base64 and multi-line form.
- **U1.3** Ports in `core/src/ports.rs`: `SecretStore`, `IdentityVerifier`, `PolicyStore`, `AuditSink`, `OperationRunner`, with doubles in the crate's `testing` entry. Use case `Broker::handle(request)` orchestrating verify, evaluate, run, scrub, audit. Flow tests through `handle` with all doubles: allow path, each deny path writes an audit line, a runner error leaks no value.

### Wave 2: adapters and entry point
- **U2.1** `IdentityVerifier` for GitHub OIDC: fetch and cache JWKS, verify signature, expiry, audience, issuer; map claims to `Identity`. Integration test against a fake JWKS server with a locally signed token (valid, expired, wrong audience, wrong issuer, tampered).
- **U2.2** `IdentityVerifier` for the tailnet: whois lookup of the peer address, tag check. Integration test against a fake local API socket.
- **U2.3** `SecretStore` Bitwarden adapter: move the `bws` access from `clis/secrets/src/secrets/backend.rs` behind the port (the CLI keeps only its `macos` keychain backend; the Bitwarden backend stays in the CLI until Wave 6 so nothing breaks mid-way). One read credential, one edit credential, edit reachable only from `rotate-*` runners. Integration test with a fake `bws` binary (an executable stub, as `clis/secrets` tests do).
- **U2.4** `PolicyStore` (reads the committed policy file at the deployed commit), `AuditSink` (the logging sink; `Logger.*` with static event names), HTTP routes in `rs/src/api`, composition root in `rs/src/main.rs`. Black-box integration test: the built binary, every dependency a fake, a request per catalogue entry.
- **U2.5** Deploy wiring: compose project under `infra/oracle-cloud/`, tailnet-only listener, `wf ship secrets-broker` leg in `deploy.yml`, `wf secret verify` entry for its own two credentials in `registry.json`. Gate: a deploy of the no-op service plus the deployment verification test (`test:verify:deployed`) hitting `/health` over the tailnet.

### Wave 3: first operations (read-only, shadow mode)
- **U3.1** `verify-secrets <app>` runner: reuse the logic of `wf secret verify`, moved behind `OperationRunner`; returns present/usable per key. Shadow: `wf ship` preflight calls both, logs a difference, still trusts the old path.
- **U3.2** `build-env <app>`: returns only registry keys with `public: true`, as a signed envelope (key id, issued-at, sha, values, signature). Signing key lives only in the broker. Unit test: a non-public key requested is refused.
- **U3.3** `wf broker` verb (new crate `clis/broker`, surfaced in `clis/wf` router and `wf --help`; search `wf --help` first): `wf broker run <op> [args] --json`. In CI it mints the OIDC token (`ACTIONS_ID_TOKEN_REQUEST_URL`), reaches the broker over the tailnet (`wf vm net up` on the VM, the ephemeral node on hosted), prints the result plus evidence (matched rule, deny reason, next step). Verifies the `build-env` signature with the broker's public key (pinned in the crate, rotated by a reviewed commit). Flow tests through the thin binary with the broker doubled; parity run through the real `wf` router.
- **U3.4** Shadow comparison harness in CI: a job that runs `wf broker run verify-secrets` and `build-env` beside the old calls in `deploy.yml` and `e2e-tests.yml` and fails on a difference. Run on three consecutive pushes before enforcing.

### Wave 4: policy and pins
- **U4.1** Policy file `backend/secrets-broker/policy.json` with rows for the Wave 3 operations: repository, ref `refs/heads/dev`, the reusable workflow at a full sha, event `push`/`workflow_dispatch`, runner kinds. A test loads the real file and checks it against the schema and the operation catalogue (every operation in the catalogue has a row; no row names an unknown operation).
- **U4.2** `wf` binary pin: the `clis` job publishes the binary's sha256 with a GitHub artifact attestation; `PolicyStore` reads the allowed hash from a file the same job writes in a reviewed commit (or from the attestation's subject); `wf broker` sends its own hash. Negative probes (below) include a modified binary.
- **U4.3** Pin bump flow: `wf broker policy bump --workflow --wf-hash` prints the policy diff for a PR (agent-first JSON). A pin that moves needs the PR; document in `docs/solutions/secrets/pin-bump.md`.

### Wave 5: write and privileged operations
- **U5.1** `deploy <app> --ref`: the broker signs and calls `vm-ops-api` `/redeploy` (the request-signing seed leaves the runner). `wf ship` calls it; old path kept until enforced.
- **U5.2** `eas-publish`, `gem-sync`, `tailscale-acl apply`, `rotate-ts-key`, `check-ts-key-expiry` runners (one unit each; rotate uses the edit credential). Each ported workflow keeps its trigger and drops its BWS GitHub secrets.
- **U5.3** `github-token <scope>` (pending open question 1): mints a narrowed GitHub App installation token per caller allowlist, absorbing the GitHub access broker plan's broker unit.

### Wave 6: service injection and cutover
- **U6.1** Injection: the broker writes a per-service file set on tmpfs at start; `docker-entrypoint.sh` of each service reads those files; scope from `registry.json`/`groups.json`; identity by image digest and compose project. Start with `leetcode-backend`. Verification: deployment verification (service up with its secrets) plus a negative probe (the container asks for another service's key, denied and audited).
- **U6.2** Enforce per operation: flip `wf ship`, `deploy.yml`, `e2e-tests.yml`, the `tailscale-*` and `repomix-gemini-context` workflows to broker-only; delete the old calls.
- **U6.3** Remove vault credentials from callers: `BWS_*` from `infra/oracle-cloud/ci-runner/docker-compose.prod.yml` and `backend/remote-chat-agent/docker-compose.prod.yml`; the Bitwarden backend from `clis/secrets`; `bws` from `infra/docker/agent-base/Dockerfile`; `.github/actions/install-bws`; the bws pin in `mise.toml`. Gate: `wf check --budget`, `pnpm run sync:check`.
- **U6.4** Rotate every previous credential (deploy read, CI read, the copies in compose env, Tailscale auth keys, Docker Hub and EAS tokens); the broker's two credentials are the only live ones. Done through the `rotate-*` operations where they exist; otherwise record the manual step in `docs/runbooks/`.
- **U6.5** Update `.agents/instructions/root.md` ("CI runs on the VM", "Secrets", "Act, don't ask" credential lookup) and run `pnpm run sync:agent-config`.

## Verification ladder (what each wave proves)

Order of reporting per `docs/solutions/testing/verification-ladder.md`.
- Unit: Waves 0-1 (domain only).
- Flow: Wave 1.3, Wave 3.3 (through the real entry point, doubles).
- Integration: Wave 2 (the built binary as a black box, fakes for JWKS, whois, `bws`).
- Deployment verification: Waves 2.5, 6.1 (real calls against the deployed broker over the tailnet).
- Negative probes that prove the request arrived and was denied (each asserts an audit line at the sink, not only an error): wrong `sha`; unpinned workflow; `pull_request` event; wrong tailnet tag; modified `wf` binary; a non-public key through `build-env`; a container asking for another service's secrets; an expired and a wrong-audience token.
- Environment dump: a CI job after cutover prints `env` and the process list with values masked and asserts no registry key's value appears.
- Each broker test run publishes its report and run bundle (`wf test publish`).

## Risks and mitigations
- The broker becomes a single point of failure for deploys: keep the old path behind a flag until each operation has three green shadow runs; a broker outage must be recoverable without a human (compose restart policy, health check, `wf vm recover` covers it).
- The broker host is the VM already trusted with the vault; the gain is shrinking who can reach the credential, not removing the VM from the trust base.
- Hosted runners need the ephemeral tailnet node (`tag:ci-gh-hosted`, ACL change in `infra/tailscale/policy.hujson`): that is part of the public-mirror work (Part 2 of the plan), not a prerequisite for this plan; the broker works for the VM runner alone first.
- Timing: the broker deploys beside `vm-ops-api`; `vm-ops-api` still ships first (it builds itself at the commit).

## Benchmarks
A new workflow and a new `wf` verb: log a cheap benchmark (the extra seconds per job for the OIDC exchange and one broker call, one run per runner kind) in `docs/audits/2026-09-02-token-diet-decision-log.md`; no model spend is involved, so no `wf offload` run.

## Open questions
1. Absorb the GitHub access broker plan's broker into this service (recommended, Wave 5.3) or keep it in `vm-ops-api`?
2. Where does the broker's `build-env` public verification key live: pinned in the `clis/broker` crate (recommended) or served by the broker and trusted on first use?
3. Hosted-runner tailnet transport is decided (workload identity federation); confirm the Tailscale plan supports it before Part 2, else use the OAuth-client fallback.
