---
module: backend/vm-ops-api
tags: [secrets, bitwarden, ci, inventory]
problem_type: workflow-pattern
last_updated: 2026-11-08
---

# Secret consumer inventory

Step 1 of the secrets-broker plan: every place a CI job or VM container reads a secret or a vault credential directly today, and the broker operation that replaces it. Names only; no values.

Source: `grep -rnE "BWS_|bws |wf secret (get|sync-env|verify)|secrets\.[A-Z_]+|secrets: inherit"` over `.github`, `backend/*`, `infra`, `clis/deploy`, plus `clis/secrets/src/data/registry.json`. Re-run it before each cutover step; this list is a starting point, not a proof of completeness.

| Consumer | What it reads | Today | Proposed operation |
| --- | --- | --- | --- |
| `.github/workflows/deploy.yml` (~L891) | `EXPO_PUBLIC_BETTERSTACK_TOKEN`, `BETTERSTACK_INGEST_HOST` | `wf secret get --backend bitwarden` in the job | `build-env <app>` (returns only the public `EXPO_PUBLIC_*` values the web/EAS build bakes in) |
| `.github/workflows/e2e-tests.yml` (~L237) | same two keys | same | `build-env <app>` |
| `.github/workflows/deploy.yml` (L931, L945) | every repo secret | `secrets: inherit` on reusable workflow calls | none: the callee gets `id-token: write` only |
| `wf ship` preflight (`wf secret verify`) | the app's secrets, Docker Hub token | runner holds a Bitwarden read token | `verify-secrets <app>` (returns present/usable per key, never a value) |
| `wf ship`, `vm-ops-api` request signing | request-signing key (`VM_GATEWAY_AUTH_SEED`) | resolved with bws at job time | `deploy <app> --ref`: the broker signs and calls vm-ops-api |
| `infra/oracle-cloud/ci-runner/docker-compose.prod.yml` (L73-75) | `BWS_ADMIN_ACCESS_TOKEN`, `BWS_SERVER_URL` in the runner container | env passed to the runner | removed; the runner holds no vault credential |
| `backend/remote-chat-agent/docker-compose.prod.yml` (L306-307) | same two vars in a service container | env passed to the container | removed; broker-materialised tmpfs files (service injection) |
| `infra/docker/agent-base/Dockerfile` | installs `bws` for agent-shell and remote-chat-agent | `bws` in the image | `bws` dropped from the base image once both consumers use the broker |
| `.github/workflows/tailscale-acl.yml` | `BWS_CI_DEPLOY_ACCESS_TOKEN` (GitHub secret) | GH secret to read the vault | `tailscale-acl apply` |
| `.github/workflows/tailscale-rotate-auth-key.yml` | `BWS_CI_DEPLOY_ACCESS_TOKEN`, `BWS_ADMIN_ACCESS_TOKEN` (write-capable) | GH secrets | `rotate-ts-key` (the write credential lives only in the broker) |
| `.github/workflows/tailscale-auth-key-expiry.yml` | `BWS_CI_DEPLOY_ACCESS_TOKEN` | GH secret | `check-ts-key-expiry` |
| `.github/workflows/repomix-gemini-context.yml` (L73) | `GCLOUD_DRIVE_CREDENTIALS_DB_B64` | `wf secret get` writes a credentials file on the runner | `gem-sync` (the broker runs the upload; the credential never reaches the runner) |
| `backend/*/docker-entrypoint.sh`, `backend/*/.env.defaults` | per-service secrets via generated `.env` | `wf secret sync-env --for <app>` | service injection, scoped by `registry.json` / `groups.json` |
| `clis/secrets/src/secrets/backend.rs` | three Bitwarden tokens: deploy read, deploy edit, `BWS_CI_DEPLOY_ACCESS_TOKEN` | resolved from env or keychain | Bitwarden adapter moves behind the broker's `SecretStore` port; `wf` keeps only the local `macos` keychain backend |

## Findings that shape the design
- Three distinct Bitwarden credentials exist (deploy read, deploy edit, CI read). The broker should hold one read credential and one edit credential, and the edit one is reachable only from `rotate-*` operations.
- `secrets: inherit` exposes every GitHub secret to the called workflow. It goes away first; it is the cheapest win and needs no broker.
- The CI runner container and `remote-chat-agent` both carry the vault read token. After cutover neither does.
- `.github/actions/install-bws` and the `bws` pin in `mise.toml` become removable once no job calls `bws`.
