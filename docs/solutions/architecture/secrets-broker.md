---
module: backend/vm-ops-api
tags: [secrets, broker, bitwarden, oidc, tailscale, ci, github-actions, policy, audit]
problem_type: architecture-decision
last_updated: 2026-11-08
---

# ADR: a secrets broker holds every vault credential; callers get results, not values

Status: deferred 2026-11-08 (see docs/plans/public-mirror/plan.md); was proposed. Step 2 of the secrets-broker plan; inventory of today's consumers: `docs/solutions/secrets/consumer-inventory.md`.

## Context

Secrets reach callers directly today. CI jobs and VM containers hold Bitwarden read tokens (three exist: deploy read, deploy edit, CI read), run `wf secret get --backend bitwarden`, or inherit every GitHub secret through `secrets: inherit`. Two goals make that unacceptable:

1. Run CI on GitHub-hosted runners, from a public mirror of this repo. A hosted job and a fork PR must never be able to read a secret.
2. No raw secret reachable by any CI job or VM container, except through an allowed operation.

## Decision

A new service, `backend/secrets-broker/{core,rs}` (a `core`/`rs` crate pair like `vm-ops-api`), is the only holder of vault credentials. `vm-ops-api` stays the deploy and ops control plane and calls the broker; the broker does not call back into CI.

### 1. Operations, not reads

A caller invokes a named operation. The broker runs it on the VM and returns status, scrubbed logs and artifacts, never a secret value. There is no "get secret" API.

Initial catalogue (each row maps to the inventory):

| Operation | Returns | Replaces |
| --- | --- | --- |
| `verify-secrets <app>` | per key: present and usable | `wf secret verify` in the ship preflight |
| `deploy <app> --ref <sha>` | the deploy result | request signing for `vm-ops-api` in `wf ship` |
| `build-env <app>` | the allowlisted public keys (see below) | `wf secret get` of `EXPO_PUBLIC_*` in `pipeline.yml`, `e2e-tests.yml` |
| `eas-publish` | the update link | EAS token on the runner |
| `rotate-ts-key`, `check-ts-key-expiry` | result only | the two `tailscale-*` workflows and their BWS tokens |
| `tailscale-acl apply` | result only | `tailscale-acl.yml` |
| `gem-sync` | result only | `gcloud` credentials file written on the runner |

New operations are added to the catalogue in code and to the policy file together; an operation absent from either is denied.

### 2. Service injection (the one exception)

A VM container receives its own secrets, materialised by the broker as files on tmpfs when it starts, scoped to that service by `clis/secrets/src/data/registry.json` and `groups.json`. The container holds no vault credential and cannot ask for another service's keys. The broker identifies the container by image digest and compose project, not by a claim the container makes about itself.

### 3. What the Expo app may carry

A client bundle is extractable by anyone with the app, so no injection scheme makes a secret in it safe. The app carries only public, scoped, rotatable values, for example the BetterStack ingest token. `build-env` serves only keys that the registry marks `public`, and returns them signed so the pinned workflow can verify what it bakes in. Anything that must stay secret moves behind a service endpoint that holds it.

### 4. Identity: who is calling

The broker verifies identity itself; it trusts no header a caller sets.

- GitHub-hosted job: the GitHub OIDC JWT (issuer `token.actions.githubusercontent.com`, signature via JWKS, short expiry, audience set to the broker). Checked claims: `repository`, `ref`, `sha`, `job_workflow_ref` (a reusable workflow at a full commit sha), `runner_environment`, `event_name`. Any `pull_request*` event is denied. A fork PR gets no OIDC token at all.
- Transport for hosted runners: an ephemeral tailnet node joined by Tailscale workload identity federation (`tag:ci-gh-hosted`), no stored tailnet secret. The ACL lets that tag reach only the broker port. No public ingress to the VM. Fallback if federation is unavailable: a Tailscale OAuth client scoped to `auth_keys` for that one tag, as an environment-scoped secret on a protected branch.

### 5. Policy and version pins

One committed, schema-validated policy file maps `operation -> allowed identities -> pins`. The domain evaluates it as a pure function (`identity + operation + pins presented -> allow | deny(reason)`).

Pins, all required for the operations that need them:

- the exact reusable workflow at a full sha (`job_workflow_ref`) and the commit `sha` it runs for;
- the `wf` binary sha256, published by the `clis` job with a GitHub artifact attestation the broker verifies;
- for service injection, the image digest.

A new workflow, binary or image is denied until a reviewed commit bumps the policy. Moving a pin therefore needs a pull request, not a runner or secret change.

### 6. Audit and failure

- Every request is recorded: caller claims, operation, matched rule, allow or deny with reason, duration. A line counts only when a query at the logging sink returns it.
- Deny by default; an unreadable policy, an unreachable JWKS or an unknown operation fails closed.
- Secret values never appear in results, logs or errors: results are scrubbed against the values the operation touched.
- Human break-glass is the local `macos` keychain backend of `wf secret`; no CI path exists.

### 7. Credentials

The broker holds one Bitwarden read credential and one edit credential. The edit credential is reachable only from `rotate-*` operations. After cutover every earlier token (deploy read, CI read, the copy in the `remote-chat-agent` compose file) is rotated and removed.

## Package shape

Per `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`:

- `core`: `domain` (policy evaluation, operation catalogue types, scrubbing; no I/O), ports `SecretStore`, `IdentityVerifier`, `PolicyStore`, `AuditSink`, `OperationRunner`.
- `rs`: adapters (Bitwarden, OIDC/JWKS plus tailscale whois, file policy, log sink, operation runners calling `vm-ops-api`), HTTP, one composition root.
- Doubles ship behind a `testing` entry. Flow tests enter through the real binary; integration tests run the binary as a black box with every dependency faked.

## Threat model

| Threat | Mitigation |
| --- | --- |
| Fork PR runs workflow code | No OIDC token for forks; `pull_request*` denied; no secrets in hosted workflows; no self-hosted runner exists |
| Compromised or malicious step in an allowed job | Operations return results only; policy pins workflow sha and `wf` hash; a modified binary is denied |
| Stolen OIDC token | Short expiry, audience bound to the broker, claims tie it to one repo, ref, sha and workflow |
| Compromised container asks for other keys | Identity is image digest and compose project; scope comes from the registry |
| Secret leaks via logs or errors | Scrubbing against touched values; audit review |
| Broker compromise | Smallest surface: no shell for callers, closed catalogue, fail closed, tailnet-only listener; the cost of that compromise is the cost of the vault credential, so it is rotated at cutover and on every broker redeploy |
| Policy tampering | Policy changes are reviewed commits; the broker reads only the pinned commit's policy |
| Supply chain in hosted runner (action or dependency) | Actions pinned by sha, least-privilege `permissions:`, no secrets in the job to steal; the only credential is the short-lived OIDC token |

## Rollout

1. Shadow mode: the broker runs each operation beside the old path and the results are compared.
2. Enforce per operation, then delete the old path (`BWS_*` from compose env, GitHub secrets, `secrets: inherit`, `bws` from `agent-base`, `.github/actions/install-bws`).
3. Rotate every previous credential.
4. Negative probes that prove the request arrived and was denied: wrong `sha`, unpinned workflow, `pull_request` event, wrong tag, modified `wf` binary, a container asking for another service's secrets.

## Consequences

- Secrets never leave the VM.
- Every new secret use needs a catalogue entry and a policy rule, which is the point and the cost.
- The broker is a single point of failure for deploys and secret-using CI; the VM's own recoverability rules (`infra/oracle-cloud/AGENTS.md`) apply to it.
- Open: whether `build-env` signatures are verified by the workflow or by `wf`; decide in the implementation plan.
