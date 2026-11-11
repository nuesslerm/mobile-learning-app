---
title: "docker-compose's explicit environment: map silently drops new vault secrets from containers"
date: 2026-08-21
category: integration-issues
module: "opencode-telegram-bot (since removed), backend/remote-chat-agent (docker-compose.prod.yml)"
problem_type: integration_issue
component: docker
symptoms:
  - "POST /owner-notify returns 404 even though the route exists and the sibling /notify route on the same server correctly returns 401 for a bad secret"
  - "wf secret sync-env --backend bitwarden confirms every new key resolves correctly into the host-generated .env file, yet the running container's env is missing them"
  - "container startup log shows an unrelated bws: not found warning from the TypeScript secrets package's own load() call that looks like the root cause but is a red herring"
root_cause: config_error
resolution_type: config_change
severity: high
tags: [docker-compose, environment-variables, vault-secrets, silent-failure, deploy, vm-ops-api]
---

# docker-compose's explicit environment: map silently drops new vault secrets from containers

## Problem

A newly added HTTP route, `POST /owner-notify` on the `opencode-telegram-bot` service's hand-rolled notify server (the bot has since been removed), returned `404` after a full build-push-redeploy even though every secret-provisioning step (vault registration, `APP_KEY_GROUPS` (since replaced by per-app groups in `clis/secrets/src/secrets/groups.json`; the bot's group went away with the bot), `bws secret create`, `.env.defaults`, and a direct `wf secret sync-env` re-run) had been independently confirmed correct on the host.

`backend/remote-chat-agent` still uses the same explicit `environment:` mapping, but its container now fills, at boot, any registered vault secret that compose did not pass: `service_kit::secrets::load()` runs `wf secret env` with the forwarded `BWS_*` pair (see the comment at `backend/remote-chat-agent/docker-compose.prod.yml:278-291`). There the trap is limited to keys the registry does not name, such as non-secret `.env.defaults` keys (`VNC_AUTH_LISTENER_URL`) and host-written ones (`HOST_LABEL`), and to boots where the vault is unreachable.

## Symptoms

- `POST /owner-notify` against the live, redeployed container returned `404`, while `POST /notify` on the exact same process/port returned `401 {"error":"unauthorized"}` for a bad secret — proving the process was up and its router worked, and that the failure was specific to the new route (the bot's notify-server handler gated the route on `deps.ownerNotify` being truthy; when it was `undefined` the request fell through to the generic 404 branch directly below it, which answers anything other than `POST /notify`).
- `deps.ownerNotify` was only constructed when `data.config.vncAuthOwnerNotifySecret` was truthy AND `data.config.vncAuthOwnerChatIds.length > 0` (the bot's `main.ts` assigned the secret inside `main()` while wiring the notify server, and the gate sat a few lines below it in the same `createNotifyServer` call) — so the 404 meant that config was empty *inside the running container*, despite `wf secret sync-env --for opencode-telegram-bot --backend bitwarden --base .env.defaults --out <file>` resolving every new key that app needs correctly on the host moments earlier (verified by grepping the output file for `<present, len=N>` on each key, never printing raw values).
- The container's own startup log opened with: `[secrets] backend "bitwarden" unavailable, continuing with the existing environment -- Bitwarden Secrets Manager list failed (read token) -- is bws installed and the token valid? /bin/sh: 1: bws: not found`.

## What Didn't Work

The `bws: not found` log line looked like a plausible root cause — a secrets-backend failure logged at the exact moment the missing config would be expected to fail — and nearly became the chased lead. It was a dead end: that line came from the TypeScript secrets package's own `load()` call (that package has since been replaced by the Rust `wf secret`, whose `load()` at `clis/secrets/src/secrets/mod.rs:57` emits the same best-effort warning), invoked at the top of the bot's `main.ts`, running *inside* the container. The container has no `bws` binary and by design doesn't need one — it's only ever supposed to consume whatever env `docker-compose` already injected from the host-generated `.env` file, not resolve secrets itself. Two things ruled it out: it is pre-existing and benign (this warning fires on every boot of this container, unrelated to this change), and if a failing in-container `load()` actually mattered, `TELEGRAM_BOT_TOKEN` and the other long-standing secrets would have been broken too — they weren't. The symptom was scoped to exactly one new route, which a general secrets-backend failure inside the container could not explain.

## Solution

The real fix-finding step was to grep the bot's compose file for `environment:` and `VNC_AUTH`. The compose file's `environment:` block listed every var the container received *explicitly*, one line per key, with zero entries for any of the five new `VNC_AUTH_*` vars. No `env_file: .env` line anywhere passed the whole file through automatically.

The fix (the remote-chat-agent half is still present in the tree) adds each var, defaulted to `${VAR:-}` so an unset value doesn't hard-fail the compose parse, to the `environment:` block of both services:

The bot's `docker-compose.prod.yml` (removed with the bot), in its `environment:` block (the callee — needed its own secret + the listener URL):
```yaml
      # Before: these four keys were absent from this block entirely.
      # After:
      VNC_AUTH_LISTENER_URL: ${VNC_AUTH_LISTENER_URL:-}
      VNC_AUTH_TRIGGER_SECRET: ${VNC_AUTH_TRIGGER_SECRET:-}
      VNC_AUTH_OWNER_CHAT_ID: ${VNC_AUTH_OWNER_CHAT_ID:-}
      VNC_AUTH_OWNER_NOTIFY_SECRET: ${VNC_AUTH_OWNER_NOTIFY_SECRET:-}
```

`backend/remote-chat-agent/docker-compose.prod.yml:248-252` (the caller — needed all five, including the notify URL it dialed out to; that dialer, `createOpencodeAuthRecovery` in `src/lib/authRecovery.ts`, went away in the Rust rewrite, so today nothing reads `VNC_AUTH_OWNER_NOTIFY_URL` and `backend/remote-chat-agent/.env.defaults` leaves it unset; the lines below are still in the file):
```yaml
      VNC_AUTH_LISTENER_URL: ${VNC_AUTH_LISTENER_URL:-}
      VNC_AUTH_OWNER_NOTIFY_URL: ${VNC_AUTH_OWNER_NOTIFY_URL:-}
      VNC_AUTH_TRIGGER_SECRET: ${VNC_AUTH_TRIGGER_SECRET:-}
      VNC_AUTH_OWNER_CHAT_ID: ${VNC_AUTH_OWNER_CHAT_ID:-}
      VNC_AUTH_OWNER_NOTIFY_SECRET: ${VNC_AUTH_OWNER_NOTIFY_SECRET:-}
```

Committed as `fix(deploy): pass VNC_AUTH_* through to both compose services' containers`, then redeployed both services live via `POST /services/<app>/redeploy?tag=<sha>&ref=<branch>` on `vm-ops-api` (the explicit `?ref=` was required because the redeploy mechanism sticky-pins each app's config-fetch git ref, and the fix commit wasn't on `main` yet). Re-verified end-to-end, not just that the container restarted cleanly: `POST /owner-notify` now returns `200 {"ok":true}`, and separately, `/done` sent from a non-owner Telegram account returned the exact rejection string the deployed code produces ("Only the configured owner chat can use /done.") — confirming both the route and the owner-gate config are genuinely live.

## Why This Works

This repo's backend services deliver env vars to their containers via `docker-compose`'s explicit `environment:` mapping style (`KEY: ${KEY}` per line), not `env_file: <path>` (which would pass everything through automatically). Two separate, independently-maintained lists of the same variable names thus sit between "the value is correctly resolved" and "the container process can read it":

1. The *resolution* pipeline — `.env.defaults` (committed, non-secret) merged with vault secrets via `wf secret sync-env --for <app> --out .env`, gated by which keys the app's `--for` group in `clis/secrets/src/secrets/groups.json` says it needs.
2. The *delivery* pipeline — the `environment:` block in that app's `docker-compose.prod.yml`, which only forwards the keys spelled out there, regardless of what's sitting in `.env` on the host filesystem.

Nothing enforces that a key added to list 1 is also added to list 2. `wf secret sync-env` has no visibility into any compose file and no reason to — its contract ends at producing a correct `.env`. Compose, in turn, has no way to know a `.env` value was ever *supposed* to reach the container; an unlisted key is simply not in scope, silently. The failure mode has no error and no warning anywhere in this path: the container just sees an unset var, which for an `.optional()` zod-schema config field (as the bot's config parsing used) resolves to `undefined` — a value indistinguishable from "the feature was never configured at all." That's exactly why `POST /owner-notify` degraded to a plain 404 (the route construction in the bot's `main.ts` treated absent config as "feature not wired up") rather than any startup failure.

## Prevention

- **A grep-based parity check is the cheapest real guard here.** A script (or a `wf check` mode, next to `--budget` and `--dead-refs`, which already enforce repo-wide conventions) that, for each app, diffs the keys referenced in `${VAR}`/`${VAR:-...}` form across `docker-compose.prod.yml`'s `environment:` blocks against the keys that app's `.env.defaults` header comment or `clis/secrets/src/secrets/groups.json` group declares it needs, and fails on any key present in one list but not the other. This is mechanical (both are plain text/YAML) and would have caught this exact gap at commit time rather than after a full deploy-and-probe cycle.
- Alternatively/additionally, treat "add a new `VNC_AUTH_*`-style var" as a documented three-file checklist (registry → `clis/secrets/src/secrets/groups.json` → **both** the `.env.defaults` and the consuming service's `docker-compose.prod.yml` `environment:` block) in `docs/archive/solutions/infra/deployment-provisioning.md`, so the compose-file step isn't only discoverable by tracing a live 404 back through the bot's `main.ts`.
- Either way, the verification step that actually surfaces this class of bug is the one this session used to find the real cause: confirm a newly-added var reaches the container's own process env (e.g. via the `vm-ops-api` logs/exec surface), not just that `sync-env` resolved it into `.env` on the host — "present in `.env`" and "present in the container's process env" are two different claims, and `backend/remote-chat-agent/docker-compose.prod.yml` already carries a comment naming this exact trap for `BWS_ADMIN_ACCESS_TOKEN`/`BWS_SERVER_URL`, suggesting this has recurred before and is worth a standing check rather than a one-off fix.

## Related Issues

- [`docs/archive/solutions/security/registry-vault-name-drift-silently-omits-a-secret.md`](../../archive/solutions/security/registry-vault-name-drift-silently-omits-a-secret.md) — the closest sibling: the same "two independently-maintained lists of the same variable names drift apart, and every layer treats the gap as optional" shape, one link earlier in the pipeline (registry.json vs. the Bitwarden vault, closed by `wf secret doctor`). That tool's drift detection is scoped to registry↔vault and cannot see the compose-side gap this doc documents — a full correctness check needs all three lists (registry, vault, and each app's `docker-compose.prod.yml` `environment:` block) to agree, not just the first two.
- [`docs/solutions/security/keychain-secrets-not-env.md`](../security/keychain-secrets-not-env.md) — the canonical walkthrough of how a secret reaches a running container (registry → backend → `sync-env` → `.env` → `docker compose up`). It stops at `.env` generation and doesn't call out that the compose file's `environment:` block is a second, independent allow-list `sync-env`'s success says nothing about.
