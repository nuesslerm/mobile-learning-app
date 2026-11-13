---
title: Every deploy is one flow that fails fast with its real error, and never polls blind
date: 2026-10-25
category: workflow
tags: [deploy, ci, deploy, vm-ops-api, smoke-test, fail-fast, docker-hub]
problem_type: workflow_issue
module: clis/deploy, clis/wf-vmops, backend/vm-ops-api
---

## Context

CI run 40146009885 spent 60 minutes on the vm-ops-api deploy leg and then timed out. The Docker Hub token on the VM had expired, the build
script swallowed the `docker login` error, and `POST /redeploy` had answered 202 before the build ran. `wf ship --wait-serving` then read
only `/health`, which kept reporting the old commit, for up to an hour. Nothing in that path ever looked at the outcome the service
had recorded.

## The rule

Before any phase, the **preflight** (`wf secret verify --for <app>`, with the Docker Hub token the build pushes with) checks that each secret
the deploy carries is usable, not only present: hygiene for every value (no stray whitespace or newline), then the shape and the one
read-only provider call that the secret's own registry entry names (`check.shape`, `check.live` in `clis/secrets/src/data/registry.json`,
read by `clis/secrets/src/secrets/verify.rs`; nothing is duplicated in code). A secret that cannot be checked carries an `uncheckable`
reason beside it, and a flow test fails when a secret of any deploy group has neither, so none is silently unchecked. A missing, malformed or
rejected secret stops the deploy in seconds, naming the key and the reason and never the value; a provider that does not answer is
reported as `unchecked` and never blocks.

A deploy then has four phases, and `wf ship` prints the seconds of each (`phase <name>: <n> s`):

1. **Build** is a step of the CI job on the hosted runner's docker daemon (the `build-image` action, `wf ship build`; the VM-side `wf ship --build-only` is retired): its exit code is the result. A `docker login` that is unset or
   rejected ends the build (`${DOCKERHUB_TOKEN:?}`, `exit 1`) before any image is built.
2. **Smoke test** is its own step between the build and the swap: `POST /services/<app>/smoke` (vm-ops-api: `POST /redeploy/smoke`) runs the
   pulled image as a throwaway container (same env and mounts, compose's `run --no-deps`) on the spare port 18999 in the netns of the
   live one and passes its probe from inside (`<bin> --healthcheck`, or `curl` for remote-chat-agent), leaving it running. `wf` then
   asks it over the tailnet, the way a client would (the app's own `/health` on port 18999, a signed `/health` for vm-ops-api; the tailnet
   ACL grants every port), and removes it, whatever the answer was (`phase smoke test`, `phase tailnet probe`). vm-ops-api
   deliberately does not compare `GIT_COMMIT`: an image whose inputs are unchanged is re-tagged, not rebuilt, and keeps the commit it was
   built at. A failure ends the leg with that container's log or the probe's error, and nothing was swapped. Every managed app is evaluated the same way: it has a
   `smoke()` entry in `backend/vm-ops-api/core/src/domain/apps.rs` or a written entry in `SMOKE_EXEMPT` (a unit test enforces one or the other),
   and the gate ends with one verdict line, `smoke: {"app":...,"inside":...,"tailnet":...}`. The capture app and vnc-auth-session have no
   `/health` route, so theirs is an HTTP answer from the listener inside the container; vnc-auth-session's copy starts in smoke mode
   (`VNC_AUTH_SMOKE=1`, its own gate port) so it never repoints the live tailnet route. agent-shell is the one exemption.
3. **Swap** answers with its own result and runs no smoke test of its own. Another app's redeploy is a foreground sibling whose exit code
   is the answer. vm-ops-api replaces itself, so `POST /redeploy?ref=<commit>&tag=<tag>` answers 202 once the detached swap is spawned, and the swap script restores the old stack on a failure and records `redeploy.failed` and `restored` in
   `/redeploy/status`.
Limits are 3x the slowest of three live timings (`docs/audits/2026-09-02-token-diet-decision-log.md`): the build wait 200 s
   (`BUILD_LIMIT_MS`), an app redeploy request 90 s, vm-ops-api's request 30 s and its serving check 30 s.
4. **Serving** is one bounded `/health` check (`SERVING_LIMIT` in `clis/deploy/src/ship.rs`). For vm-ops-api it also reads
   `/redeploy/status` and fails at the first round on a failure recorded for the commit.

There is no polling of a long-running step: the build is waited as a job, every other phase answers inside its request, and the only
loop is the short health check.

## What the redesign deleted

- vm-ops-api's own build supervisor (pressure sampling, the agent-shell job launcher, the 4-hour ceiling) and its build script: CI
  builds, vm-ops-api only proves and swaps.
- The 3600 s `wait_serving` loop and `wf vm ops redeploy-self` (the fire-and-forget path).
- An app redeploy's `pull && down ; up` chain, in which a failed pull was masked by the `;` that followed: the pull is a line of its own
  and ends the script.

## Prevention

Flow tests drive `wf ship vm-ops-api` through a fake VM (failed build, refused smoke test, recorded failure, swap), the vm-ops-api
integration suite runs the smoke-test script against a docker double (healthy, dead container), and the golden scripts pin
every generated script byte for byte.
