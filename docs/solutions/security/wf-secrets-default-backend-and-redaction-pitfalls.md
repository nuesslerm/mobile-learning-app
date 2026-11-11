---
title: "wf secret get silently no-ops on the macos backend in sandboxed sessions — always pass --backend bitwarden"
date: 2026-08-10
category: security
module: clis/secrets
problem_type: workflow_issue
component: tooling
severity: medium
applies_when:
  - "querying Bitwarden-vault secrets via `wf secret get`/`wf secret list` from a sandboxed Linux Claude Code session with no macOS Keychain"
  - "verifying whether a secret exists in Bitwarden without ever needing to print its value"
symptoms:
  - "`wf secret get VM_HOST` fails with \"Unknown key\" for a secret that is genuinely not registered in clis/secrets/src/data/registry.json, not because the vault lacks it (VM_HOST was unregistered when this was written; it has since been registered)"
  - "`wf secret get <KEY>` with no `--backend` flag intermittently returns \"not found\" for a key confirmed present in the vault via `bws secret list`, then succeeds on a later retry with no code change"
  - "`wf secret get <KEY> --backend bitwarden` prints only the bare secret value with no `KEY: ` prefix, breaking `sed`-based redaction patterns written for the prefixed no-flag output shape"
related_components:
  - clis/wf
tags: [secrets-management, bitwarden, wf-secrets-cli, default-backend, secret-redaction, sandboxed-linux, bws-cli]
---

# wf secret get silently no-ops on the macos backend in sandboxed sessions — always pass --backend bitwarden

## Context

An agent working in a sandboxed, non-macOS Claude Code Remote session against this repo needed to resolve two secrets through `wf secret` (which the wf dispatcher routes straight into `clis/secrets/src/secrets/mod.rs` via `fn secret` in `clis/wf/src/routes.rs`): `VM_HOST` and `EXPO_TOKEN`.

`wf secret get VM_HOST` correctly failed with `Unknown key: VM_HOST` plus the full valid-keys list. That is the CLI's hard-allowlist gate firing (`clis/secrets/src/secrets/mod.rs:90-101`, `valid_key` and `unknown_key`): the key is checked against the registry (`self.registry.iter().position(|e| e.key == key)`) *before* the backend resolves anything, so no vault is ever touched. At the time `VM_HOST` had zero matches anywhere in `clis/secrets/src/data/registry.json` — confirmed by grepping the full file — because it was then a GitHub-Actions-only SSH deploy secret (`ubuntu@<ip>` shape) deliberately never mirrored into Bitwarden. It has since been registered (`registry.json:214-219`, service `vm-host`, its description placing it in the gh-ci Bitwarden project that CI reads), so the same command no longer hits "Unknown key" today. "Unknown key" is a *registry* rejection, not a vault lookup at all; no `bws` call happens on this path.

`wf secret get EXPO_TOKEN` (no `--backend` flag) was a different story: it printed `EXPO_TOKEN: not found` (the failure branch at `clis/secrets/src/secrets/mod.rs:167-170`), and `wf secret list` showed it `missing (missing)`. `EXPO_TOKEN` **is** a registered key (`registry.json:173-177`), so this was not an "Unknown key" rejection — the backend genuinely failed to resolve a value for a key the registry says should exist.

The user pushed back hard, confident the secret existed and that the admin/EDIT machine account had visibility into it. Bypassing `wf secret` entirely and querying `bws` directly (`BWS_ACCESS_TOKEN="$BWS_ADMIN_ACCESS_TOKEN" bws secret list --server-url "$BWS_SERVER_URL"`, piped through a filter that printed only `key`, `projectId`, `len(value)`, `revisionDate` — never the raw value) confirmed 25 secrets visible to the READ token, including `EXPO_TOKEN` with a `revisionDate` from earlier the same day. The secret had just been added to the vault; that timing is the most likely explanation for the earlier "not found." `VM_HOST` was, separately, confirmed genuinely absent from both the READ and ADMIN token's visible sets — consistent with the registry-level finding above, not contradicting it.

Re-running `wf secret get EXPO_TOKEN` with no flag then succeeded. But a follow-up comparison against a long-established secret, `VM_GATEWAY_AUTH_SEED` (`registry.json:309-313`), showed the identical no-flag command intermittently returning "not found" and then succeeding on a later, identical retry — while `wf secret get VM_GATEWAY_AUTH_SEED --backend bitwarden` (explicit flag) succeeded reliably on every attempt. That surfaced the real trap this learning is about: **the CLI's bare default backend was `macos`, not `bitwarden`** — and in this sandboxed environment `macos` is a silent no-op, not an error. Since then (first in the TypeScript `defaultBackendName()`, now `default_backend_name()` in `clis/secrets/src/secrets/backend.rs`) the bare default off darwin is `bitwarden`, so the trap now needs `SECRETS_BACKEND` or `--backend` set to something other than `bitwarden` (see Guidance).

## Guidance

**Always pass `--backend bitwarden` explicitly, or export `SECRETS_BACKEND=bitwarden` once per session, when running as an agent in a sandboxed/non-macOS Claude Code session.** Never trust the bare default.

When this session ran, the CLI resolved its backend from the `--backend` flag, then `SECRETS_BACKEND`, then a hard-coded `'macos'`. The resolution is now spelled out at `clis/secrets/src/secrets/backend.rs:375-379`:
```rust
pub fn backend_name(flag: Option<&str>) -> String {
    flag.map(str::to_string)
        .or_else(|| std::env::var("SECRETS_BACKEND").ok())
        .unwrap_or_else(|| default_backend_name().to_string())
}
```
where `default_backend_name()` (`clis/secrets/src/secrets/backend.rs:367-373`) returns `'macos'` only on darwin and `'bitwarden'` everywhere else, so a bare call in a Linux sandbox now lands on Bitwarden. The `macos` backend itself still cannot work there: its `keychain_lookup` (`clis/secrets/src/secrets/backend.rs:70-74`) is documented as "a no-op off darwin" and returns no value. Any `SECRETS_BACKEND`/`--backend` value other than `bitwarden` (an inherited `SECRETS_BACKEND=macos` or an explicit `--backend macos`, but also an empty `SECRETS_BACKEND=` or a typo such as `--backend bitwarde`) falls through to the `macos` backend (`build`, `clis/secrets/src/secrets/backend.rs:360-365`) and reproduces exactly the miss described here. Every `wf secret` subcommand that resolves a value builds its backend through `backend_name()` (`Ctx::new`, `clis/secrets/src/secrets/mod.rs:47-54`), so within `wf secret` this function is the single source of truth for which backend actually runs; `cache clear` and `registry sync` build none themselves (`registry sync` spawns `wf secret doctor --backend bitwarden`); `login` resolves its listener credentials through `load_values`, i.e. `Ctx::new(None)`, so only `SECRETS_BACKEND` steers it. Outside `wf secret`, the tailnet join behind `wf vm net up` keeps its own copy of the fallback (`clis/deploy/src/tailscale_up.rs:95`), which, unlike `backend_name()`, treats an empty value as unset. On a Linux sandbox there is no macOS Keychain to no-op *against* in the sense of erroring; it simply cannot resolve anything, which surfaces as an ordinary "not found," visually indistinguishable from a secret genuinely never provisioned. This mechanism explains the *original* `EXPO_TOKEN` miss and is the most defensible read of the later intermittent `VM_GATEWAY_AUTH_SEED` behavior too — some agent invocation in the chain likely dropped or never set `--backend`/`SECRETS_BACKEND` on a subset of calls. The exact intermittency mechanism (why the *same* literal no-flag command differed run to run within one session) was not fully root-caused this session — treat it as observed, not diagnosed. The practical, verified, repeatable fix that held on every retry was the explicit flag: `wf secret get VM_GATEWAY_AUTH_SEED --backend bitwarden` never failed once.

**Understand the `resolve()` chain before concluding a secret is missing from the vault.** `clis/secrets/src/secrets/backend.rs:338-358` (`Bitwarden::resolve`) sets which sources are tried and in what order, and `list_through_copy` (`clis/secrets/src/secrets/backend.rs:268-297`) sets where each source's listing comes from:
- A normal (unmarked) registry entry tries the READ-only `BWS_ADMIN_ACCESS_TOKEN` first, then falls back to the write-capable `BWS_ADMIN_ACCESS_TOKEN` — line 346: `vec![Source::Read, Source::Admin]`.
- An entry marked `vault: 'admin'` in the registry uses **`Source::Admin` only**, no READ fallback — line 344: `vec![Source::Admin]`. The reason, carried over from the deleted TypeScript backend's comments (`backend.rs` states no rationale of its own): those secrets (e.g. `OCI_*`) were deliberately excluded from the read-only deploy project grant so a leaked deploy token can't reach them; silently falling back to the read grant here would quietly re-widen exactly what that exclusion exists to narrow. A key the CI runner (deploy read token only) must read moves to the deploy project with `wf secret move <KEY> <PROJECT_ID> --apply`, which keeps its id, value and note, and loses its `vault: 'admin'` mark in the same commit; the four `CH_BACKEND_*` query keys moved this way on 19 Oct 2026 for the deployed-deployment verification tests.
- An entry marked `vault: 'gh-ci'` tries the gh-ci token first (the intended credential), then falls back through the read and admin tokens — line 345: `vec![Source::GhCi, Source::Read, Source::Admin]`. The read and admin fallbacks exist only because the deploy grants currently span both projects, and are expected to stop finding anything, harmlessly, once that overlap is removed (this rationale, too, comes from the TypeScript backend's comments). The inline comment at lines 348-349 notes that a source is listed only when the ones before it did not hold the key, so a hit in the read source never waits on (or fails with) the write-capable one.
- So the **write-capable EDIT/admin token is genuinely needed as a fallback for ordinary secrets too**, not just for `vault: 'admin'`-scoped ones — but only ever to *read* wider (`bws secret list`), never to mutate anything through this path. `store()`/`remove()` on the bitwarden backend both unconditionally throw (`clis/secrets/src/secrets/backend.rs:332-337`): this backend is read-only regardless of which token resolved the value.
- The read and gh-ci listings may come from the local vault copy (`clis/secrets/src/secrets/cache.rs`, `DEFAULT_TTL_SEC` 3600, so a 1 h TTL): a fresh copy is used without calling `bws` at all. Only the admin listing is always fetched live. `wf secret create`/`update`/`rename`/`move` clear the copy after a successful vault write (so does a successful keychain `delete`; on bitwarden `delete` always errors), and `wf secret sync-env` refreshes it. `wf secret set` on the bitwarden backend writes to the vault without clearing it (`clis/secrets/src/secrets/mod.rs`), and a secret added in the web vault or with `bws` directly reaches neither, so for up to an hour the read listing can serve a just-set value stale, and a session without `BWS_ADMIN_ACCESS_TOKEN` (no live admin fallback) reports a just-added registered key as `not found`, which is the same just-added `EXPO_TOKEN` shape this doc describes.

**Check `registry.json` presence before concluding a key "isn't in the vault."** These are two different failure modes with two different fixes:
- **"Unknown key"** (`clis/secrets/src/secrets/mod.rs:90-101`) — the key was never registered in `registry.json` at all. This happens *before* any backend touches `bws`. Fix: the key genuinely doesn't exist as a concept in this system (as `VM_HOST` was by design when this session ran), or it needs a new registry entry (which `VM_HOST` has since received).
- **"`<KEY>`: not found"** (`clis/secrets/src/secrets/mod.rs:167-170`) — the key is registered but every source in its `resolve()` chain came back empty for it. Fix: check the actual backend being used first (see above) before concluding the secret was never provisioned. Then rule out a stale local vault copy: the read and gh-ci listings may come from it (1 h TTL; the admin listing is always live), so before concluding a secret just added outside wf is missing, run `wf secret cache clear` or set `WF_SECRETS_CACHE=refresh` (or `off`) and retry.

**Never print a raw secret value to verify existence.** The safe pattern used this session: run `bws secret list --output json` and pipe it through a filter that keeps only `key`, `projectId`, `len(value)`, and `revisionDate` — never the `value` field itself. That is enough to confirm presence, which project it lives in, and roughly when it was last touched (useful for spotting a just-added secret, as with `EXPO_TOKEN`'s same-day `revisionDate`), with zero exposure risk.

**Be aware that different invocation shapes can print secret output in different formats, and a redaction script keyed to one shape can silently fail on another.** This session observed exactly that: a `sed` redaction pattern written assuming a `KEY: <value>` prefix (as seen from a no-flag `get`) did not match the output shape produced by an explicit `--backend bitwarden` invocation, and two real secret values (`EXPO_TOKEN`, `VM_GATEWAY_AUTH_SEED`) leaked unredacted into the agent's own visible tool output/context as a result. One caveat on the mechanism: re-reading the `get` command as currently written (`clis/secrets/src/secrets/mod.rs:156-176`) shows its success path is unconditionally `out(&format!("{v}\n"));` (line 172) — a bare value with no `KEY:` prefix — regardless of whether `--backend` was passed; the failure path is the only place a `{key}: ` prefix appears, and only on the *not-found* branch (line 168). So the exact code path producing the differing success-format observed this session could not be pinned down against this file, and should be treated as an open, unreconciled detail rather than a fully-diagnosed mechanism. The conclusion holds regardless of the exact cause: **never build a redaction/parsing script that assumes one specific output shape.** Better still, never let a raw secret value reach stdout/a visible tool-output context at all — capture it into a shell variable (`TOKEN=$(wf secret get KEY --backend bitwarden)`) and use it only inside something like an `-H "Authorization: Bearer $TOKEN"` flag, never echoed or piped through a display command.

## What the approve guard refuses, and what it deliberately allows

`scripts/guards/block-secret-exposure.mjs` (the `condition` script behind the secret rules in `libraries/hook-runtime/src/deny.json`) used to refuse any Bash call in which a secret fetch and any dump word (`head`, `cat`, `base64`, ...) or file redirect appeared anywhere in the same command string. That was safe but too coarse: `TOK=$(wf secret get K); curl ... | head -c 200`, or a fetch beside a `head` of an unrelated file, was refused, and an agent that read the refusal as "secrets are off limits" stopped instead of splitting the call. On the owner's instruction (2026-10-09) it now refuses only when the fetched value itself reaches a leak-shaped sink:

- The fetch's own pipeline continues into a dump word, `tee`, `xargs` or a network-egress tool (`wf secret get K | head -c 40`, `... | curl -d @- host`).
- The fetch, or an `echo`/`printf` of a variable captured from one, is redirected to a real file (`wf secret get K > f`, the shape that leaked `BWS_ADMIN_ACCESS_TOKEN`; `echo "$TOK" >> notes`). `/dev/null` and `2>&1` stay allowed.
- A dump word or `tee` is handed the value by a here-string, a process substitution or a captured variable (`base64 <<< "$TOK"`, `cat <(wf secret get K)`), or an egress tool is handed it as a here-string.
- After `eval "$(wf secret env)"` loaded the whole credential set, an environment listing (`printenv`, bare `env`/`set`, `export -p`, `/proc/*/environ`) or a secret-named variable printed into one of the sinks above.

The sanctioned pattern is unchanged and now composes with other commands: capture with `TOK=$(wf secret get KEY --backend bitwarden)` (or substitute inline) and use the value only inside a header or argument, for example `curl -s -H "Authorization: Bearer $TOK" https://host/api | head -c 200`. A command the scanner cannot parse (an unbalanced quote or parenthesis) falls back to the old coarse rule, and any scanner failure, not only an unbalanced quote, falls back to that coarse rule (`blocks()` catches every throw and returns `coarse()`), so a crash is never a blanket allow, but an unparseable command with no dump word, redirect or egress pipe is still allowed: `wf secret get X "` passes, while `wf secret get X | head -c 5 "` (still unbalanced, now with a dump word) is refused (`scripts/guards/block-secret-exposure.test.mjs`). The trade-off is explicit: a dump word beside an unrelated fetch is no longer refused, while every shape that has actually leaked a value still is. The guard is a tripwire for those shapes, not a sandbox: a value an agent prints deliberately, or smuggles through a construct the scanner does not model (a `while read` loop, a `{ ...; }` group), is out of its reach, so the rule "never print, echo, redirect or write a secret value" still holds on its own. The unit tests are in `scripts/guards/block-secret-exposure.test.mjs`; `clis/secrets/tests/flow/secrets.rs` (`the_value_revealing_subcommands_are_refused_to_agents_by_both_guards_and_no_default_file_holds_a_vault_key`) keeps the script's subcommand list, the deny list and the guard's behaviour in sync.

## Why This Matters

A misleading "not found" is visually identical to "this secret was genuinely never provisioned." An agent that ends up on the `macos` backend in a sandbox (the bare default when this was written) will hit that false negative and may report to a human that a credential is missing — which, if the human happens to know better (as happened this session), wastes their time re-confirming something already sitting in the vault the whole time, and, if the human doesn't happen to know better, can trigger an unnecessary and potentially disruptive re-provisioning of a secret that was never actually lost.

Separately, an accidental secret-value leak into an agent's own transcript/context is a real credential-exposure risk even when no external party ever sees it — that transcript can be logged, reviewed, forwarded, or persisted in ways the agent doesn't control. The platform's own approval-guard classifier treats this seriously enough to outright block even a diagnostic 6-character value-prefix print (a third attempt this session, blocked with "Permission for this action was denied by the Claude Code auto mode classifier") — a strong external signal that "just print a little bit of it to check" is not a safe verification strategy, and that the metadata-only `bws secret list --output json` filtering approach is the correct default instead.

## When to Apply

Any time an agent — in this repo, or in any similarly-structured repo that surfaces a `wf secret get` command — needs to resolve a Bitwarden-backed secret from inside a non-macOS or sandboxed session. In practice this means essentially always for Claude Code Remote / cloud sandbox sessions in this repo: the `macos` backend (`keychain_lookup` and `Macos`, `clis/secrets/src/secrets/backend.rs:70-74` and `118-126`) can never work there, since there is no macOS Keychain to back it, so ending up on it is never the right outcome — it is only ever a silent miss waiting to be mistaken for "the secret doesn't exist."

Apply this specifically:
- Before running `wf secret get`/`list`/`env`/`sync-env` for the first time in a fresh sandbox session — set `SECRETS_BACKEND=bitwarden` once, or pass `--backend bitwarden` on every invocation.
- Before telling a human a secret "is missing" based on a `not found` result — first confirm which backend actually ran, and separately confirm the key is even registered (`registry.json`) versus genuinely absent.
- Before writing any script that parses or redacts `wf secret` output — assume the format can vary by invocation shape and write it defensively, or avoid parsing raw output altogether by capturing values into variables instead.
- Any time verifying "does this secret exist in the vault" is the actual goal — reach for the metadata-only `bws secret list --output json` filter, never a raw `get`/`sync-env --reveal` just to check presence.

## Examples

**Failing pattern — trusting the bare default backend:**
```bash
# In a sandboxed Claude Code Remote session, no SECRETS_BACKEND set:
wf secret get EXPO_TOKEN
# EXPO_TOKEN: not found      <-- misleading: resolved against the macOS backend,
#                                 which is a silent no-op off darwin, not against bitwarden
```
Since then (first in the TypeScript `defaultBackendName()`, now `default_backend_name()` in `clis/secrets/src/secrets/backend.rs`) this bare call lands on `bitwarden` off darwin; the same miss now needs `SECRETS_BACKEND` or `--backend` set to anything other than `bitwarden` (including empty or misspelled).

**Reliable pattern — explicit backend every time:**
```bash
wf secret get EXPO_TOKEN --backend bitwarden
# <redacted>                 <-- succeeds reliably; confirmed across every retry this session

# Or, once per session:
export SECRETS_BACKEND=bitwarden
wf secret get VM_GATEWAY_AUTH_SEED
# <redacted>
```

**Unsafe verification — redaction keyed to one output shape, silently failing on another:**
```bash
# Written assuming a "KEY: value" prefix; does not match a bare-value success line,
# so the real secret passes through unredacted into the agent's own visible output.
wf secret get EXPO_TOKEN --backend bitwarden | sed 's/^EXPO_TOKEN: //'
# Actual output was just the bare value already -- the sed pattern matched nothing,
# and the raw secret landed in tool output/context unredacted.
```

**Safe verification — metadata only, value never touched:**
```bash
BWS_ACCESS_TOKEN="$BWS_ADMIN_ACCESS_TOKEN" bws secret list \
  --server-url "$BWS_SERVER_URL" --output json \
| python3 -c '
import json, sys
for s in json.load(sys.stdin):
    print(s["key"], s.get("projectId"), len(s["value"]), s.get("revisionDate"))
'
# EXPO_TOKEN <projectId> <len> 2026-08-10T...   <-- presence + recency confirmed,
#                                                     raw value never printed
```

**Blocked pattern — do not attempt, even for "just a diagnostic prefix":**
```bash
python3 -c "print(open_secret_value[:6])"
# Permission for this action was denied by the Claude Code auto mode classifier
```

## Related

- `docs/solutions/security/keychain-secrets-not-env.md` — the canonical `clis/secrets` architecture doc (the `macos`/`bitwarden` backend split, `--backend`/`SECRETS_BACKEND` selection, the registry contract). It documents the mechanism this doc's trap depends on and summarizes the hazard (an explicit `--backend macos`/`SECRETS_BACKEND=macos` off darwin silently resolves nothing), and links here for the failure modes and metadata-only verification.
- `docs/archive/README.md` — a different root cause (registry↔vault key-name drift in either direction) but an adjacent failure-mode taxonomy: "unregistered" vs "registered but unresolvable."
- `docs/solutions/security/sync-env-stdout-redaction.md` — the same `wf secret` module (`clis/secrets/src/secrets/mod.rs`) and the same secret-into-transcript leak domain, but a different command (`sync-env` printing every value with no `--out`) and a different root cause (missing flag, not an output-format assumption in a hand-written redaction script).
