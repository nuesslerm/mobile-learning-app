---
title: "agent-shell's capability accretion: the SSH key is no longer the real perimeter — exec-script's shared bearer token is"
date: 2026-08-26
category: security
tags: [security, agent-shell, blast-radius, vm-ops-api, exec-script, threat-model, tailscale]
problem_type: knowledge
module: infra/oracle-cloud/agent-shell
---

## Context

Written as the analysis half of a "reconsider `agent-shell`'s blast radius"
recommendation from an interface audit of how agent sessions reach this
repo's VM and containers. The user asked for the tradeoff analysis and
recommendation only — no architecture change was requested or made. Recorded
so the reasoning survives past the chat that produced it, per this repo's
"compound a solved analysis" convention.

## How it actually grew

`agent-shell/README.md`'s "Why this exists" section states the founding
invariant plainly:

> No `/var/run/docker.sock` mount, no `docker` CLI, no host bind-mount of
> any kind. A fully compromised session inside it has nothing to escalate
> to — it can't reach the host, the other apps' containers, or anything
> outside its own tailnet identity.

That was true at first. Three additions since then, each individually
well-reasoned in its README section:

1. **Nested Docker-in-Docker + Playwright** (`## Docker-in-Docker +
   Playwright`) — added so this container could exercise the same
   build/push/screenshot capabilities a Claude Code Remote sandbox has, for
   CI-parity e2e tests (`e2e-mobile-web-screenshot`,
   `e2e-backend-deploy-smoke`). Genuinely isolated — own nested `dockerd`,
   no host socket, no host mount — so the "can't reach the host" invariant
   survives this addition intact. Confirmed via the Dockerfile's comment:
   "no host `docker.sock` mount... A fully compromised session
   here can build/push images and tear down its own nested Docker
   environment, but still cannot reach the host."
2. **`redeploy-app`** (`## Redeploying another app from inside this
   container`) — a thin wrapper around `vm-ops-api`'s own `POST
   /services/:app/redeploy`, letting this container trigger a redeploy of
   any of the VM's other production apps to an already-published tag.
3. **`exec-script`** (`## Ad-hoc script execution, without SSH or a
   redeploy`) — explicitly documented in `vm-ops-api/README.md` section 10
   as *"the one deliberate exception to vm-ops-api's 'never a generic exec
   endpoint' rule"*: a caller-supplied script body runs inside this
   container, full stop.

## The non-obvious finding: the SSH key stopped being the real perimeter

The interesting part isn't the list of additions — it's what `exec-script`
did to the actual security boundary. It is reached over `vm-ops-api`'s HTTP
API (`POST /services/agent-shell/exec-script`), gated by
`VM_GATEWAY_AUTH_SEED` — **not** the SSH key. And per an existing `TODO` in
`vm-ops-api/server.js`:

> this token is now ALSO reused as the bearer credential for
> leetcode-backend's usage routes ... a leak scoped to that service's own
> attack surface (e.g. a public Tailscale Funnel) also compromises this
> API's ability to redeploy/terminate both VMs.

So today, "arbitrary code execution inside agent-shell" has **two
independent front doors**:

- The SSH identity (personal `SSHID_URL`, or the narrow
  `AGENT_SHELL_E2E_SSH_PUBLIC_KEY` test identity minted this session).
- `VM_GATEWAY_AUTH_SEED` — a token whose scope already extends to an
  unrelated service's public-facing API surface.

Narrowing the SSH side (a personal identity vs. a scoped test identity) is
real, good practice, but it no longer defines the actual perimeter for
agent-shell: a leak of the *other* credential grants the same
arbitrary-code-execution outcome without touching SSH.

## What's actually at stake in a compromise

**Not** host access — that invariant holds; the Docker-in-Docker addition
was executed carefully enough to preserve it. What a compromise of either
credential above reaches:

- Whatever secrets/coding-agent-CLI credentials (`claude`/`codex`/`opencode`)
  are resolved into this container's environment.
- Docker Hub push access, if `DOCKERHUB_TOKEN` is set — a
  supply-chain lever: a pushed tag could later be pulled by a redeploy.
- The ability to redeploy or roll back any of the VM's other production
  apps to any already-published tag via `redeploy-app`/`exec-script` calling
  out to `vm-ops-api` — denial-of-service via a forced crash-loop, or a
  downgrade to a known-vulnerable prior build. (Not unique to agent-shell —
  any holder of `VM_GATEWAY_AUTH_SEED` already
  has it directly against `vm-ops-api`; agent-shell is simply a second path
  to the same token-gated action.)

## Options considered, with real tradeoffs

- **Status quo.** No incident has occurred; every addition was reviewed and
  justified independently. Cost: the perimeter is harder to reason about
  than the README's stated invariant implies, and the same
  one-good-reason-at-a-time path that produced today's shape will keep
  extending it.
- **Split the exec/ops surface from the interactive dev shell.** A second,
  narrower container carries `redeploy-app`/`exec-script`; `agent-shell`
  stays the human/agent-facing SSH dev shell with Docker + Playwright +
  coding CLIs. Real cost: another container image and Tailscale identity to
  provision and keep patched; every `exec-script` call site (this session's
  own `wf vm exec`/`pty` commands included) would need a different
  target host.
- **Scope `VM_GATEWAY_AUTH_SEED` per purpose** (already flagged as a `TODO`
  in `server.js` for the leetcode-backend reuse specifically). This is the
  cheaper, more targeted fix: it directly shrinks the blast radius of the
  credential that is doing the most work in the CURRENT threat model,
  without touching agent-shell's container topology.

## Recommendation

The split is defensible but not urgent — there is no incident, and the
Docker-in-Docker addition already proves this repo can add real capability
to agent-shell without regressing the host-escape invariant that matters
most. Splitting `VM_GATEWAY_AUTH_SEED` into purpose-scoped tokens is the
higher-leverage move for the effort, since it directly narrows
today's *actual* perimeter (two credentials granting equivalent
arbitrary-code-execution access) rather than reorganizing containers around
a threat model that hasn't materialized.

Revisit the container split specifically once `exec-script` usage grows
past ad hoc/test scripts into routine, scheduled, or multi-caller
automation — that is the point where "one container, one purpose" stops
being a nice-to-have and becomes the thing standing between routine
automation and a full compromise of the dev-shell credentials sitting next
to it.
