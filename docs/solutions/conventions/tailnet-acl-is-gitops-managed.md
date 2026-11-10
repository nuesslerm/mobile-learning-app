---
title: "The tailnet ACL is GitOps-managed, so write scope is only safe when the deployer runs"
module: infra/tailscale
component: tailscale-acl
category: conventions
problem_type: convention
date: 2026-08-11
tags:
  - tailscale
  - acl
  - gitops
  - oauth-scopes
  - drift
applies_when:
  - "Granting or widening scopes on the Tailscale OAuth client"
  - "Changing the tailnet ACL from an agent session"
  - "Assessing whether a second writer to a config store is a real risk"
---

# The tailnet ACL is GitOps-managed, so write scope is only safe when the deployer runs

## Context

The tailnet ACL has a source of truth in this repo: `infra/tailscale/policy.hujson`.
`.github/workflows/tailscale-acl.yml` tests it on pull requests and deploys it via
`tailscale/gitops-acl-action@v1` on push to `main`. Nothing else is supposed to
write it.

That shapes how the Tailscale OAuth client should be scoped. Granting
`Policy File → Write` to a credential an agent session holds creates a **second
writer** that bypasses git: an agent edits the live ACL, and either the next push
to `main` silently reverts it (a change that works, then stops, with no PR
explaining why), or it survives and `policy.hujson` now disagrees with reality,
with no review trail.

The catch is that this argument has a hidden premise — that the GitOps deployer
is actually running. When GitHub Actions minutes are exhausted, `policy.hujson`
becomes a file nothing applies, and `Policy File → Write` stops being a redundant
second writer and becomes the *only* way to make that file real. Same scope, same
repo, opposite conclusion.

## Guidance

**Check live-vs-repo parity before and after any ACL change.** The API returns
the deployed policy; the repo holds intent. Compare them semantically rather than
byte-wise, since HuJSON permits comments and trailing commas:

```bash
curl -sS -H "Authorization: Bearer $TS_TOKEN" -H "Accept: application/hujson" \
  "https://api.tailscale.com/api/v2/tailnet/-/acl" -o /tmp/live-acl.hujson
# then strip comments/trailing commas from both and compare parsed JSON
```

**Prefer the git path while the deployer runs.** Edit `policy.hujson`, open a PR,
let CI deploy. Reviewable, revertible, and it is how the existing `funnel`
`nodeAttrs` grant arrived.

**When the deployer is not running, the agent becomes the deployer — and must do
both halves.** Push the ACL *and* commit `policy.hujson` in the same change, or
the next working CI run will revert it. A discipline that depends on remembering
is the weak form; a command that reads `policy.hujson`, pushes it, then reads the
ACL back and asserts equality makes drift impossible to leave behind.

**Scope-granting notes for this credential** (from the Tailscale scope editor):

- There is **no separate "delete device" scope**. `Devices → Core → Write`
  ("read or modify devices and their properties") is what governs
  `DELETE /api/v2/device/{id}`.
- The HTTPS-certificates toggle that Funnel TLS needs lives under
  `Settings → Networking Settings` ("settings that edit networking, e.g. HTTPS"),
  **not** under DNS.
- `Devices` and `Auth Keys` write scopes are tag-scoped (`tag:mobile-learning`
  here). The `General` section — DNS, Policy File — is **not**, so those grants
  are tailnet-wide and broader than the device grants.
- `OAuth applications → Write` lets a token widen its own scopes; `Users → Write`
  can suspend or delete users; `Policy File → Write` can lock you out of your own
  tailnet with a bad push. None serve anything this repo does.

## Why This Matters

A "second writer" objection is a statement about a *system*, not about a
permission. It holds only while the first writer runs. Stating it as a property
of the scope — "write access here is unsafe" — produces confident advice that
inverts the moment CI stops, and the person acting on it has no way to see which
premise failed.

The concrete cost here was a recommendation to withhold a scope that was, by
then, the only remaining way to deploy the ACL at all.

## When to Apply

- Before recommending for or against a write scope on any store that has a
  declared source of truth in the repo
- Whenever an ACL change is made outside the PR path
- When CI availability changes (billing, quota, outage) — several risk arguments
  that assume CI runs need re-checking, not just this one

## Examples

**Verifying parity** — the state at the time this was written:

```
live bytes: 3008  repo bytes: 3008
live keys: ['grants', 'nodeAttrs', 'ssh', 'tagOwners']
repo keys: ['grants', 'nodeAttrs', 'ssh', 'tagOwners']
SEMANTICALLY IDENTICAL
```

**The premise check that was missed:**

| Argued | Depended on | When false |
|---|---|---|
| "Policy File write creates a second writer" | The GitOps workflow deploys `policy.hujson` | Actions minutes exhausted — there is no first writer, and write scope is the only deploy path |

## Related

- `infra/tailscale/policy.hujson` — the source of truth
- `.github/workflows/tailscale-acl.yml` — test on PR, deploy on push to `main`
- `docs/archive/solutions/developer-experience/stopped-daemon-reads-as-missing-capability.md`
  — the adjacent failure: concluding a capability is absent without checking
