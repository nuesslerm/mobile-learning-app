---
title: "A RUN step with `|| true` still counts as a cached success — retrying a flaky Docker build just re-pushes the same broken image"
date: 2026-08-17
category: build-errors
module: docker-build
problem_type: build_error
component: tooling
severity: medium
symptoms:
  - "A Dockerfile RUN step ending in `|| true` (e.g. `bash /opt/vnc-auth/setup.sh all || true`) fails its underlying command, but `docker buildx build` reports that layer as CACHED or DONE on the very next attempt with zero output"
  - "Re-running docker buildx build with no other change silently reproduces the identical broken image — a fix applied inside the shell command never actually re-executes"
  - "The built image is missing something the RUN step was supposed to install, with no error anywhere in the build log"
root_cause: logic_error
resolution_type: config_change
related_components:
  - clis/vnc-auth-capture
tags: [docker, buildkit, cache-busting, run-step, argument, cachebust]
---

# A RUN step with `|| true` still counts as a cached success — retrying a flaky Docker build just re-pushes the same broken image

## Problem

While iterating on a Dockerfile fix for a flaky `npm install -g` step (see the
companion npm/ECONNRESET doc), a plain retry of `docker buildx build` kept
producing an image with the same missing CLI binaries as the previous,
known-broken attempt — even after the underlying fix had been applied to the
shell command inside the `RUN` step.

## Symptoms

The `RUN` step in question, at `clis/vnc-auth-capture/Dockerfile:122-123`:

```dockerfile
RUN --mount=type=secret,id=extra_ca_cert,required=false \
  NODE_EXTRA_CA_CERTS=/run/secrets/extra_ca_cert bash /opt/vnc-auth/setup.sh all || true
```

`setup.sh all`'s underlying `npm install -g` calls failed intermittently
(ECONNRESET on large tarballs — see the companion doc), but the step's own
`|| true` meant the shell command that BuildKit executes always exits `0`
regardless of what happened inside it. On the next `docker buildx build`
invocation with no Dockerfile change, BuildKit reused its cached result for
that layer — reported as `CACHED` or a fast `DONE` with no re-execution —
reproducing the same missing binaries with no indication anything was wrong.

## What Didn't Work

- **Assuming a plain re-run of `docker buildx build` would retry the flaky
  step.** It did not — BuildKit's cache is keyed on the Dockerfile
  instruction and its inputs, not on whether the previous run's shell
  command "really" succeeded from the caller's perspective. A `|| true`'d
  command that fails still produces a layer BuildKit is entitled to cache as
  successful, because from BuildKit's point of view, it exited `0`.
- **Inserting `ARG CACHEBUST` mid-instruction** (attempted via a line-based
  `awk` edit that inserted the `ARG` line inside the existing multi-line
  `RUN … \` continuation). This broke Dockerfile syntax outright — `ARG`
  must be its own standalone instruction, not a line spliced into another
  instruction's backslash-continued body.

## Solution

Place `ARG CACHEBUST=<value>` as its own standalone Dockerfile line
immediately **before** the `RUN` instruction that needs busting (not
mid-instruction), and reference `$CACHEBUST` inside that `RUN`'s command so
changing the ARG's value invalidates BuildKit's cache for that layer:

```dockerfile
ARG CACHEBUST=1
RUN --mount=type=secret,id=extra_ca_cert,required=false \
  echo "cachebust=$CACHEBUST" && \
  NODE_EXTRA_CA_CERTS=/run/secrets/extra_ca_cert bash /opt/vnc-auth/setup.sh all || true
```

Passing a new value on each build attempt (`--build-arg CACHEBUST=$(date +%s)`
or any other changing value) forces BuildKit to treat the instruction's
inputs as different from the previous run, so the shell command inside
genuinely re-executes instead of returning the stale cached result. Applied
this way in a local, uncommitted patch of the Dockerfile while iterating on
the underlying npm-install fix; each real code change to the `RUN` command
also naturally busts the cache on its own (BuildKit hashes the instruction
text itself), so `CACHEBUST` is mainly needed when re-running the *identical*
command to retry something non-deterministic like a flaky network fetch.

## Why This Works

BuildKit's layer cache key is derived from the Dockerfile instruction text
and its build-time inputs (base image digest, `COPY`'d file hashes,
`ARG`/`ENV` values in scope), not from any semantic notion of "did the
command truly succeed." A `RUN cmd || true` instruction's text and inputs
are unchanged between two otherwise-identical build attempts, so BuildKit
reuses the cached layer regardless of what `cmd` actually did the first
time — the `|| true` guarantees an exit code of `0`, and BuildKit has no way
to know that `0` doesn't mean what it usually means here. Threading a
changing `ARG` value into the instruction changes its inputs, which is the
only lever BuildKit actually keys cache invalidation on.

## Prevention

- **Treat `|| true` in a `RUN` step as a cache-invalidation trap, not just
  an error-tolerance choice.** Any command wrapped in `|| true` (or any
  other exit-code-suppressing pattern) will look identically "successful" to
  BuildKit's cache whether it worked or not — a bare retry can never
  re-attempt it. If the step is expected to sometimes need a genuine retry
  (flaky network fetch, best-effort tool install), give it an explicit
  `ARG CACHEBUST` from the start rather than discovering the need mid-debug.
- **`ARG` must be its own standalone line.** It cannot be spliced into the
  middle of another instruction's multi-line body — place it immediately
  before the `RUN` (or other) instruction it is meant to invalidate.
- **Prefer making the step itself observably fail** over `|| true` where
  possible (per `docs/solutions/build-errors/every-layer-that-needs-the-proxy-ca-separately.md`'s
  own prevention note: "a guarded step that does nothing reports success" —
  the same principle applies to a step that fails but is told to report
  success). A step that surfaces its own real failure doesn't need a
  cache-buster to be retried correctly; `docker buildx build --no-cache` (or
  simply not exit-code-suppressing it) already handles the retry.

## Related Issues

- `docs/solutions/build-errors/npm-global-install-econnreset-large-tarballs-buildkit.md`
  — the flaky `npm install -g` failure this cache-busting fix was needed to
  actually retry while iterating on a solution.
- `docs/solutions/build-errors/every-layer-that-needs-the-proxy-ca-separately.md`
  — "a guarded step that does nothing reports success" documents the same
  underlying pattern (an exit code that doesn't mean what it normally means)
  for a different Dockerfile construct (a conditional CA-trust guard rather
  than `|| true`).
