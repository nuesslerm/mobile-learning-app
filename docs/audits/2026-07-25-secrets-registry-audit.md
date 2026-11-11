# Secrets registry — what every env var is for

**Date:** 2026-07-25 · **Audited:** `clis/secrets/src/data/registry.json` · **29 keys** (was 31)

A flat list of every secret this repo registers, what actually reads it, and what
to do about it. Prompted by `wf secrets push-notes` reporting 18 keys as "not in
the vault", which looks like a cleanup backlog and isn't one — "in vault" means a
Bitwarden item exists to hang a note on, nothing more.

Decisions already applied from this audit are marked **DONE**. Everything else is
a suggestion, not a change.

> **Update 2026-07-27:** the `onepassword`/`infisical` backends (and their four
> registry entries below) were removed — see the dedicated section under §7.
> The registry now has 30 keys.

---

## 1. Networking

| Env var | What it's for | In vault | Suggestion |
| --- | --- | :-: | --- |
| `TS_VM_AUTH_KEY` | Pre-auth key that joins a container to your tailnet. The `tailscale` sidecar in all five compose stacks uses it. Nothing is addressable without it — services talk to each other only over the tailnet, which is why no public app ports are opened. | ✅ | **Keep.** Required by every deploy group. |

## 2. Service-to-service passwords (self-generated, not third-party)

Each of these is a shared password on one HTTP door. The pattern is one env-var
name on both the calling and checking side, so one vault item feeds both.

| Env var | What it's for | In vault | Suggestion |
| --- | --- | :-: | --- |
| `LEETCODE_BACKEND_ADMIN_SECRET` | Guards leetcode-backend's admin routes. The cookie-capture service sends it when pushing a freshly captured LeetCode session back. | ✅ | **Keep.** Required ×2. |
| `LEETCODE_CAPTURE_TRIGGER_SECRET` | Guards the capture listener's "start a login-capture run" endpoint (`X-Capture-Trigger-Secret`). Worst case abuse is a redundant capture run. | ✅ | **Keep.** Required ×2. |
| `TELEGRAM_NOTIFY_SECRET` | Guards the bot's private notify server (`POST /notify`), which is how a headless service asks the bot to message you. | ✅ | **Keep.** Required ×2. |
| `CHAT_AGENT_API_SECRET` | Guards remote-chat-agent's HTTP API — the only thing stopping an unauthenticated caller from driving the coding agent. Despite the Telegram-adjacent name it is **not** a Telegram credential. | ✅ | **Keep.** Required ×2. |
| `VNC_AUTH_TRIGGER_SECRET` | Guards "spin up an auth desktop" (`POST /auth/start`) on the vnc-auth-capture listener. Unrelated to `LEETCODE_CAPTURE_TRIGGER_SECRET` — different service, different door. | ✅ | **Keep.** |
| `VNC_AUTH_DESKTOP_PASSWORD` | The login password for the remote desktop stream itself. Highest-risk of this group: it guards a live desktop you type real credentials into. | ✅ | **Keep.** |
| `VNC_AUTH_OWNER_CHAT_ID` | Comma-separated Telegram id allowlist for who may drive the auth session and send `/done`. An allowlist, not a password. | ✅ | **Keep in the vault** — decided 2026-07-25. It is arguably non-secret, but keeping it here makes what we share visible in one place; `.env.defaults` is for things that don't change. |

## 3. Third-party identities

| Env var | What it's for | In vault | Suggestion |
| --- | --- | :-: | --- |
| `TELEGRAM_BOT_TOKEN` | The bot's identity with Telegram, from @BotFather. Used by the bot process, by remote-chat-agent (which sends diff/plan messages directly), and by `wf bot start`. | ✅ | **Keep.** Required ×2. |
| `GH_TOKEN` | Fine-grained GitHub PAT that remote-chat-agent uses to clone repos and push from its worktrees. Also used by `deploy-gh-pages.sh` for the Pages API. Its scope is the blast radius — it sits in plaintext on a VM that runs model-generated code. | ✅ | **Keep.** Required. Worth a short expiry and eventually a GitHub App installation token. |
| `EXPO_TOKEN` | Authenticates the EAS CLI non-interactively, for headless publish/build. | ✅ | **Keep.** In no deploy group by design — no backend VM publishes, so it belongs in your local/agent env, not on a deploy host. |

## 4. Model providers (remote-chat-agent runs an opencode server)

| Env var | What it's for | In vault | Suggestion |
| --- | --- | :-: | --- |
| `OPENCODE_GO_API_KEY` | The OpenCode API key. **The single canonical name** — the opencode binary reads it as `OPENCODE_API_KEY`, so compose and `wf bot start` remap it at spawn. | ❌ | **DONE: consolidated.** The separate `OPENCODE_API_KEY` entry pointed at the same backend service (same value, two names) and was removed. Now **required**. Add it to the vault. |
| `MINIMAX_API_KEY` | Minimax models for the agent's opencode server. | ❌ | **DONE: now required** (was optional). Add it to the vault. |
| `KIMI_API_KEY` | Kimi/Moonshot models for the agent's opencode server. | ❌ | **DONE: now required** (was optional). Add it to the vault. |
| ~~`ANTHROPIC_API_KEY`~~ | *Removed 2026-07-25.* Was Claude models for the agent's opencode server. | — | **DONE: removed** from the registry. See the updated note below — a later change (U8) added `CLAUDE_CODE_OAUTH_TOKEN` as the replacement path; this is not a like-for-like swap. |

**Why "required" mattered.** As optional, a key absent from the vault was skipped
*silently*, so a deploy succeeded and the agent then failed the first time a model
was requested. Required means `sync-env` exits non-zero and writes no `.env`.

**Update: the replacement now exists.** A follow-on plan
(`docs/superpowers/plans/2026-07-25-002-feat-dual-agent-cli-claude-gateway-plan.md`)
shipped it: `remote-chat-agent` now has a real `claudeGateway.ts` that drives
`@anthropic-ai/claude-agent-sdk` directly — opencode's server is not involved in
a claude turn at all — selected via `AGENT_CLI=claude` in `di/dataContainer.ts`
(unset, or any other value, still selects opencode). It authenticates with the
new `CLAUDE_CODE_OAUTH_TOKEN` registry entry, registered **optional** so an
opencode-only deploy is unaffected by its absence; `backend/remote-chat-agent/src/startupGate.ts`
fails fast, by name, only when `AGENT_CLI` resolves to `claude` and the token is
missing. Credential hygiene is enforced on both paths: the claude gateway strips
`ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN` from the SDK subprocess's env, and the
opencode gateway — HTTP-only, never spawning `opencode serve` itself — carries a
structural `assertNoAnthropicCredential` guard instead.

**What is still not wired:** the automated VNC-capture-to-vault pipeline.
`clis/vnc-auth-capture` has the `CredentialSink` port, a macOS-keychain sink, a
relay sink posting to the listener's own new `POST /auth/persist` route, and
`createClaudeAdapter(deps)` supplying `persist()`/`verify()` — all registered at
the `di/dataContainer.ts` composition root — but the two seams that would close
the loop end-to-end (reading `claude setup-token`'s printed output across the
capture-container boundary, and the receiving side's actual vault write) are
intentional fail-loud stubs. That is a deliberate, documented scope boundary of
this work, not a bug: today, provisioning `CLAUDE_CODE_OAUTH_TOKEN` still
requires a human to run `claude setup-token` and `wf secrets set
CLAUDE_CODE_OAUTH_TOKEN <value>` by hand. See `docs/deployment-provisioning.md`'s
provider-auth section for the full picture and the revocation/rotation runbook.

The CI copy is separate and untouched: `agent-plugins.yml` (formerly `agent-approve-guard.yml`) reads
`secrets.ANTHROPIC_API_KEY` as a GitHub Actions secret and exits 1 if empty. That
is a repo secret, not a registry entry.

## 5. Telemetry (BetterStack)

| Env var | What it's for | In vault | Suggestion |
| --- | --- | :-: | --- |
| `BETTERSTACK_SOURCE_TOKEN` | Server-side ingest token. Every backend and the secrets CLI forward logs with it; falls back to the `EXPO_PUBLIC_` one below. | ✅ | **Keep.** Optional in all five groups — a deploy succeeds without it (console-only logging). |
| `BETTERSTACK_INGEST_HOST` | Ingest hostname for that source (e.g. `sXXXX.betterstackdata.com`). | ✅ | **Keep in the vault** — decided 2026-07-25, same reasoning as `VNC_AUTH_OWNER_CHAT_ID`. |
| `EXPO_PUBLIC_BETTERSTACK_TOKEN` | Client-side ingest token, inlined into web/EAS bundles by `metro.config.js`. Public by the `EXPO_PUBLIC_` convention. | ❌ | **Keep — do not remove.** `metro.config.js:13` calls `load(['EXPO_PUBLIC_BETTERSTACK_TOKEN'])` by name, and `registryStore.get()` **throws** on an unknown key, so deleting this entry breaks Metro startup and every web build. Verified. |

## 6. Telegram end-to-end test credentials

Used only by `scripts/telegram-auth.ts`, `scripts/cleanup-e2e.ts`, the e2e suite,
and `wf bot cleanup`. In no deploy group — these never reach a VM.

| Env var | What it's for | In vault | Suggestion |
| --- | --- | :-: | --- |
| `TELEGRAM_API_ID` | Telegram *client* app id (not the bot token) for the MTProto test client that drives the bot in e2e. | ❌ | **Keep.** Provisioned by `pnpm --filter @app/opencode-telegram-bot telegram:auth`. |
| `TELEGRAM_API_HASH` | The matching client app hash. | ❌ | **Keep.** |
| `TELEGRAM_SESSION` | The saved MTProto session string, so e2e doesn't re-do a QR login every run. Written by `telegram-auth.ts`, which imports `set` from `@cli/secrets` — that import is why these need registry entries. | ❌ | **Keep.** |
| `TELEGRAM_TEST_GROUP_ID` | The group the e2e tests post into. | ❌ | **Keep.** |

**Optional cleanup:** these are a different class from production secrets. If the
registry's mixed audience is the irritation, splitting test fixtures from deploy
secrets addresses it without breaking anything.

## 7. Vault access — the credentials used to *reach* a vault

**This is what `BWS_*`, `OP_*` and `INFISICAL_*` are.** They are not app secrets.
Each production backend is a CLI wrapper that needs its own token to authenticate,
and those tokens cannot live in the vault they unlock. They resolve via
`resolveBackendConfig` (env first, then macOS keychain, **never** the active
backend) — so they don't depend on the registry to be *read*, but `wf secrets set`
needs the entry to know where to store them.

| Env var | Backend | What it's for | Suggestion |
| --- | --- | --- | --- |
| `BWS_ADMIN_ACCESS_TOKEN` | Bitwarden Secrets Manager (**the production default**) | Read-only machine-account token. This is what every VM holds and what `sync-env` uses at deploy time. | **Keep.** Structural. |
| `BWS_ADMIN_ACCESS_TOKEN` | Bitwarden | Write-capable token, used *only* by `wf secrets push-notes`. Deliberately in **no** deploy group so no deploy can ship it to a VM — a test asserts this. | **Keep.** Never put it on a VM. |
| `BWS_PROJECT_ID` | Bitwarden | Project UUID that scopes secret lookups. Also written into each VM's `.env.local` by cloud-init. | **Keep in the vault** — decided 2026-07-25. |
| `BWS_SERVER_URL` | Bitwarden | Region endpoint. Defaults to the EU cloud because this account is EU-hosted; `bws` defaults to US and rejects EU tokens with a bare `invalid_client`. | **Keep in the vault** — decided 2026-07-25. |
| ~~`OP_SERVICE_ACCOUNT_TOKEN`~~ | ~~1Password~~ | Removed 2026-07-27 — see below. | **Removed.** |
| ~~`OP_VAULT`~~ | ~~1Password~~ | Removed 2026-07-27 — see below. | **Removed.** |
| ~~`INFISICAL_TOKEN`~~ | ~~Infisical~~ | Removed 2026-07-27 — see below. | **Removed.** |
| ~~`INFISICAL_PROJECT_ID`~~ | ~~Infisical~~ | Removed 2026-07-27 — see below. | **Removed.** |

**The open question here was decided 2026-07-27: dropped.** Every `deploy.sh`
was already `SECRETS_BACKEND="${SECRETS_BACKEND:-bitwarden}"`; the 1Password and
Infisical backends were reachable only by exporting that var by hand, and
nothing did. Keeping them would have bought vendor portability — a vault
migration becomes a flag change — but dropping them was the largest single
reduction still available, and this audit's own framing (never selected by any
deploy) is exactly the `lastpass` precedent below. See "`onepassword` /
`infisical` — removed 2026-07-27" below.

### `onepassword` / `infisical` — removed 2026-07-27 **DONE**

Removed from the allowed backends entirely, the same way `lastpass` was:
the adapters (`onepassword-backend.ts`, `infisical-backend.ts`) and their
tests, the `BackendName` union members, the DI switch cases, the now-dead
`onepasswordRef`/`infisicalSecret` registry-schema fields, the `setup.sh`
installers (`install_op`/`install_infisical`), the `OP_*`/`INFISICAL_*`
registry entries above, and every doc/CLI-help claim. Both were fully
implemented, tested, read-only production backends — this was a clean removal
of dead optionality, not a migration: neither was ever selected by a real
deploy (`SECRETS_BACKEND` always defaulted to `bitwarden`).

### `lastpass` — removed 2026-07-25 **DONE**

Removed from the allowed backends entirely: adapter, tests, the `BackendName`
union, the DI case, the now-dead `lastpassItem` schema field, the `setup.sh`
installer, and every doc claim. It needed a durable, already-unlocked `lpass login`
session (human-satisfied MFA) on each production host and offered folder-level
scoping only — no machine token, no per-item ACL. It had **no registry keys**, so
this removed code and a false doc claim rather than any secret.

That false claim was the inconsistency this audit found: `AGENTS.md` said LastPass
backed `backend/leetcode-backend`'s Oracle VM, while that VM's `deploy.sh` had
always defaulted to `bitwarden`. The doc was wrong, not the deploy.

---

## Summary of what changed

| Change | Effect |
| --- | --- |
| `OPENCODE_API_KEY` entry removed, `OPENCODE_GO_API_KEY` canonical | 31 → 30 keys; no more shared-service alias pair |
| `ANTHROPIC_API_KEY` entry removed | 30 → 29 keys at audit time; a later change (U8) added `CLAUDE_CODE_OAUTH_TOKEN` (optional, wired to `AGENT_CLI=claude`) — Claude is reachable again via the Agent SDK (see the updated note above), not unavailable |
| `OPENCODE_GO_API_KEY`, `MINIMAX_API_KEY`, `KIMI_API_KEY` → **required** | Deploy fails loudly instead of shipping a provider-less agent |
| `lastpass` backend deleted | Left 4 backends: `macos`, `bitwarden`, `onepassword`, `infisical` |
| `onepassword`/`infisical` backends deleted (2026-07-27) | 2 backends remain: `macos`, `bitwarden` |
| Non-secrets stay in the registry | Rejected demoting to `.env.defaults` — visibility over purity |

**Still to do:** add `OPENCODE_GO_API_KEY`, `MINIMAX_API_KEY` and `KIMI_API_KEY`
to Bitwarden. They are required now, so `wf secrets sync-env --for remote-chat-agent`
will fail until they exist — which is the intended behaviour, but it does mean the
next agent deploy is blocked until they're added.
