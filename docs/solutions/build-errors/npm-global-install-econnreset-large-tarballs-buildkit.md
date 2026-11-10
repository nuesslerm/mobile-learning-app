---
title: "npm's own fetch client, not the network, fails on large global-CLI tarballs inside BuildKit"
date: 2026-08-17
category: build-errors
module: docker-build
problem_type: build_error
component: tooling
severity: high
symptoms:
  - "npm i -g @anthropic-ai/claude-code and npm i -g @openai/codex fail intermittently with ECONNRESET / network aborted inside a BuildKit RUN step, specifically on the large (~90-115MB) platform-specific binary tarball, not on smaller dependencies in the same install"
  - "npm pack <same package> (no install/link complexity, just a download) fails identically — ruling out anything about global-install linking"
  - "A direct curl to the exact same tarball URL, from the exact same container, succeeds instantly and reliably every time"
  - "docker exec into a running container and re-running the identical npm command also fails — ruling out a BuildKit-RUN-specific quirk (cache mount, layer isolation) as the cause"
root_cause: config_error
resolution_type: environment_setup
related_components:
  - clis/vnc-auth-capture
tags: [docker, buildkit, npm, econnreset, flaky-install, npm-cache, curl-prefetch]
---

# npm's own fetch client, not the network, fails on large global-CLI tarballs inside BuildKit

## Problem

Building `clis/vnc-auth-capture`'s Docker image installs four CLI tools via
`bash /opt/vnc-auth/setup.sh all` in `clis/vnc-auth-capture/Dockerfile` (it ran
as `/app/scripts/setup.sh` when this was written; the path moved when the image
lost its build stage, the step itself is unchanged)
— three of them (`install_claude`, `install_codex`, `install_gemini` in
`clis/vnc-auth-capture/scripts/setup.sh`) via `npm i -g`, and the fourth
(`install_opencode`) via a curl-piped shell installer, a different mechanism
not covered by this doc. Of the three npm-installed tools, the two large
ones — `@anthropic-ai/claude-code` and `@openai/codex`, whose install pulls
a ~90-115MB platform-specific native binary tarball — failed unpredictably
with `ECONNRESET` / `network aborted`, both inside a BuildKit `RUN` step and
via a plain `docker exec` into a running container. `@google/gemini-cli`
(smaller) and every tool's smaller dependencies never failed.

## Symptoms

```
npm error code ECONNRESET
npm error network This is a problem related to network connectivity.
npm error network In most cases you are behind a proxy or have bad network settings.
```

surfacing partway through `npm install -g --force @anthropic-ai/claude-code`
or `@openai/codex`, non-deterministically — sometimes on the first attempt,
sometimes only after several retries, always on the large binary tarball
rather than the package manifest or smaller transitive dependencies.

## What Didn't Work

- **Assuming it was this sandbox's egress proxy.** `docs/solutions/build-errors/every-layer-that-needs-the-proxy-ca-separately.md`
  already documents multiple proxy/CA failure modes for Docker builds in this
  environment, making proxy trust the natural first suspect. Ruled out
  directly: a plain `curl` to the *exact same* tarball URL, from the *same*
  container, at the *same* point in the build, succeeded instantly and
  reliably every time it was tried. If the proxy or its CA trust were the
  problem, curl would fail too.
- **Assuming it was BuildKit-RUN-step specific** (a cache-mount quirk, a
  layer-isolation network gap). Ruled out by reproducing the identical
  failure via a plain `docker exec` into an already-running container — no
  BuildKit machinery involved at all, same `ECONNRESET`.
- **Assuming it was install-complexity specific** (dependency resolution,
  linking, postinstall scripts). Ruled out with `npm pack <package>`, which
  does nothing but download the tarball — it failed identically to the full
  `npm install -g`.
- **Passing `-e HTTPS_PROXY=...` explicitly to override any proxy
  Docker itself might be injecting.** This surfaced a real, separate bug
  (see the companion doc on Docker's own default proxy config below) but did
  not fix this failure — the uppercase override doesn't touch npm's own
  internal retry/timeout behavior, which turned out to be the actual fault.

## Solution

Isolated the fault to npm's own HTTP fetch client
(`make-fetch-happen`/`minipass-fetch`), not the network, the proxy, or
BuildKit — by holding every other variable constant and swapping only the
HTTP client:

| Client | Same URL, same container, same moment | Result |
|---|---|---|
| `curl` | ✅ | succeeds every time |
| `npm pack` (npm's fetch client) | ❌ | fails intermittently |
| `npm install -g` (npm's fetch client) | ❌ | fails intermittently |

Both npm-driven paths go through npm's own fetch stack; only the plain
`curl` path succeeds consistently for the same bytes over the same
connection. Worked around by never letting npm fetch the large tarball
itself — download it independently with `curl`, then hand it to npm as a
local, already-cached file:

```bash
# 1. Resolve the tarball URL(s) without downloading (main package + any
#    platform-specific optionalDependency variants the CLI ships):
npm view @anthropic-ai/claude-code dist.tarball
npm view @openai/codex dist.tarball
# (repeat for each optional per-platform variant actually needed on the
# target platform, e.g. via the registry JSON API for the optionalDependencies list)

# 2. curl each tarball to local disk — this is the leg that was proven reliable:
curl -fsSL -o claude-code.tgz "<tarball-url>"
curl -fsSL -o codex.tgz       "<tarball-url>"

# 3. Inject into npm's own content-addressable cache instead of letting npm fetch it:
npm cache add ./claude-code.tgz
npm cache add ./codex.tgz

# 4. Install normally, but force npm to prefer what's already cached:
npm install -g --prefer-offline @anthropic-ai/claude-code @openai/codex
```

`--prefer-offline` means npm still resolves the dependency graph and
manifest metadata over the network as normal (that traffic was never the
problem) — it only skips re-fetching the large tarball bytes that are
already sitting in the cache from step 3.

Verified end-to-end in this session: after building the image with this
prefetch pattern (via a `FROM scratch` helper image carrying the
pre-populated packages, `COPY --from`'d into the final stage — see the
companion multi-platform-helper-image doc), a direct container smoke-test
confirmed `claude --version` and `codex --version` both returned real
version strings.

**Note on scope:** the applied build also installed `opencode` through this
same npm-package + prefetch mechanism (the `opencode-ai` npm package, also
smoke-tested as `opencode --version`), rather than through
`install_opencode`'s curl-piped shell installer in
`clis/vnc-auth-capture/scripts/setup.sh`. That substitution was for
uniformity (one COPY-based mechanism for all three large CLIs) — opencode's
own curl-based installer was never observed to exhibit the `ECONNRESET`
failure this doc diagnoses, and there is no evidence either way on whether
it would. Reusing the npm-based approach for `opencode-ai` is a pragmatic
choice, not a finding that its default installer is flaky too.

**Open gap — this fix is not yet in the tracked Dockerfile.** The working
build in this session patched a temporary copy of
`clis/vnc-auth-capture/Dockerfile` (not the committed file) to replace the
plain `bash /opt/vnc-auth/setup.sh all || true` step at
`clis/vnc-auth-capture/Dockerfile:122-123` with the curl-prefetch +
helper-image `COPY --from` steps described above for `claude`, `codex`, and
`opencode`, keeping `setup.sh gemini` (still a plain `npm i -g
@google/gemini-cli`, never observed to fail) for the fourth tool. `setup.sh`'s
`install_claude`/`install_codex` functions still call plain
`npm i -g <package>` as of this writing, so a rebuild of this image from the
tracked Dockerfile as it stands today will hit this exact same flaky
failure again. Making the fix durable — baking the prefetch pattern into
the tracked Dockerfile and/or `setup.sh`, and deciding whether to keep
`install_opencode`'s curl-based installer or standardize on the npm-package
substitution used here — was not resolved before this was documented.

## Why This Works

npm's fetch client and `curl` request the identical bytes over the identical
connection, but only one of them survives reliably. That points at
something in npm's fetch client's own handling of large response bodies
(the default retry/timeout tuning of `make-fetch-happen`/`minipass-fetch`),
not at the transport itself — a transport-level fault (proxy, DNS,
connection reset by a middlebox) would have failed `curl` too, and it never
did. Pre-populating npm's cache via `npm cache add` from a file already on
disk removes npm's fetch client from the critical path for exactly the
payload it struggles with, while leaving it in charge of everything it
handles fine (manifest/metadata resolution, dependency graph, linking).

## Prevention

- **For any future Dockerfile in this repo that `npm install -g`s a package
  with a large (tens-of-MB+) binary payload**, use the curl-prefetch +
  `npm cache add` + `--prefer-offline` pattern above rather than a plain
  `npm install -g`, even though it works fine most of the time for smaller
  packages — the failure here was specific to large tarballs, not to `npm
  install -g` in general.
- **Don't diagnose npm network failures by suspecting the proxy first.**
  This repo already has multiple documented proxy/CA failure modes (see
  Related), which makes proxy trust the reflexive first suspect for any
  Docker-build network error. Confirm or rule it out directly — a `curl` to
  the identical URL from the identical container is a fast, decisive test —
  before assuming the fix is a CA/proxy change.
- **Land this fix in the tracked Dockerfile**, not only as a local
  build-time patch, so the next rebuild of this image (CI or another
  session) doesn't rediscover the same flaky failure from scratch.

## Related Issues

- `docs/solutions/build-errors/every-layer-that-needs-the-proxy-ca-separately.md`
  — a different set of Docker-build network failures in this same sandbox,
  all genuinely proxy/CA-trust related. Worth reading first when a Docker
  build fails with a network error, but this doc's failure was confirmed
  **not** to be one of those (curl succeeded where npm failed, over the
  identical connection).
- `docs/solutions/tooling-decisions/backend-docker-build-pnpm-fetch-cache-optimization.md`
  — a different, adjacent Docker-build caching optimization (pnpm workspace
  dependency fetch/cache for the backend apps), not the same package manager
  or mechanism as this doc's global `npm install -g` for standalone CLI
  binaries, but the same broader theme of avoiding redundant/unreliable
  network fetches inside a Docker build.
