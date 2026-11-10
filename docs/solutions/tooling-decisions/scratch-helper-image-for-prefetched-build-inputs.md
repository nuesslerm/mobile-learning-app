---
title: "A FROM scratch helper image is a reliable way to COPY large prefetched files into a Dockerfile — but it must match the main build's --platform"
date: 2026-08-17
category: tooling-decisions
module: docker-build
problem_type: tooling_decision
component: tooling
severity: medium
applies_when:
  - "A Dockerfile needs large files that were downloaded or generated outside the normal build steps (e.g. curl-prefetched package tarballs) copied into an image"
  - "A local-context COPY of those files behaves unreliably or doesn't invalidate/cache the way a normal RUN step would"
  - "The main image is built for a specific --platform (e.g. linux/arm64) in a cross-platform or emulated build"
related_components:
  - clis/vnc-auth-capture
tags: [docker, buildkit, multi-platform, copy-from, helper-image, buildx]
---

# A FROM scratch helper image is a reliable way to COPY large prefetched files into a Dockerfile — but it must match the main build's --platform

## Context

Working around a flaky `npm install -g` (see the companion ECONNRESET doc)
required getting several curl-prefetched, npm-cache-injected npm global
package directories (the `@anthropic-ai`, `@openai`, and `opencode-ai`
scopes, under npm's own global module prefix on the build host — a
container filesystem location populated outside the normal Dockerfile
steps, not a path tracked in this repo) into the final image. A plain
local-context `COPY` of these directories from the build host had
unreliable caching behavior in this workflow. The fix was to
package the prefetched files as their own tiny Docker image and pull them in
via `COPY --from=<image>`, the same mechanism Dockerfiles already use to copy
artifacts out of a named build stage or another image entirely.

## Guidance

Build a minimal helper image from nothing but the prefetched files:

```dockerfile
# Dockerfile.prefetched
FROM scratch
COPY . /
```

```bash
docker buildx build --platform linux/arm64 -f Dockerfile.prefetched \
  -t local-prefetched-cli:latest --load /path/to/prefetched-files
```

Then reference it from the main Dockerfile exactly like any other
multi-stage `COPY --from`:

```dockerfile
COPY --from=local-prefetched-cli:latest lib/node_modules/@anthropic-ai /usr/local/lib/node_modules/@anthropic-ai
COPY --from=local-prefetched-cli:latest lib/node_modules/@openai       /usr/local/lib/node_modules/@openai
COPY --from=local-prefetched-cli:latest lib/node_modules/opencode-ai   /usr/local/lib/node_modules/opencode-ai
```

**The helper image's `--platform` must match the main build's `--platform`
exactly.** `docker buildx build --load` with no explicit `--platform` flag
defaults to the *host's native platform*, not whatever it will later be
`COPY --from`'d into. Building the helper image without
`--platform linux/arm64` on an x86_64 sandbox host produced an amd64 helper
image; referencing it from a main build running `--platform linux/arm64`
then failed with:

```
ERROR: no match for platform in manifest: not found
```

Passing the same `--platform linux/arm64` flag to both the helper-image
build and the main build resolved it.

## Why This Matters

`FROM scratch` produces the smallest possible image — literally just the
files copied in, no base OS layer — so it's a cheap, disposable vehicle for
getting arbitrary local files into Docker's build graph in a form that
`COPY --from` can address reliably, independent of whatever caching quirks
apply to a plain local-context `COPY` in a given workflow. But a
`docker buildx build --load`'d image is still a real, single-platform image
under the hood: exactly one platform in its manifest (whatever
`--platform` was passed, or the host's native platform if omitted), and
`COPY --from` in a multi-platform build resolves against the *target*
platform being built, not whatever happens to be loaded locally. A
mismatch produces a manifest lookup failure that names the symptom
("no match for platform") without naming which of the two images
(helper or main) has the wrong platform.

## When to Apply

- Getting large or awkward-to-`COPY`-reliably local build inputs (prefetched
  downloads, generated artifacts, binary caches) into a Dockerfile via a
  disposable `FROM scratch` helper image and `COPY --from`.
- Any time a main build passes an explicit `--platform` flag (cross-building
  for a different architecture, or building in an emulated/QEMU
  environment) and also references a locally-built helper image via
  `COPY --from` — pass the identical `--platform` to the helper image's own
  build command.

## Examples

**Before** (helper image built without `--platform`, defaults to host's
native platform — here, `amd64` on an x86_64 sandbox):

```bash
docker buildx build --load -f Dockerfile.prefetched \
  -t local-prefetched-cli:latest /path/to/prefetched-files
# main build:
docker buildx build --platform linux/arm64 -t final-image:latest .
# → ERROR: no match for platform in manifest: not found
```

**After** (both builds pass the same `--platform`):

```bash
docker buildx build --platform linux/arm64 --load -f Dockerfile.prefetched \
  -t local-prefetched-cli:latest /path/to/prefetched-files
docker buildx build --platform linux/arm64 -t final-image:latest .
# → succeeds
```

## Related

- `docs/solutions/build-errors/npm-global-install-econnreset-large-tarballs-buildkit.md`
  — the flaky npm install this helper-image pattern was built to work around
  (getting the curl-prefetched, cache-injected package directories into the
  final image).
- `docs/solutions/build-errors/docker-image-arch-mismatch-arm64-vms.md` — a
  different platform-mismatch failure in this repo (a *registry* manifest
  missing an arm64 entry after `docker push`, discovered at `docker compose
  pull` time on a production VM). Same broad failure family — Docker
  refusing to resolve a manifest for the wrong platform — but a distinct
  mechanism: that doc concerns a pushed multi-arch manifest list on Docker
  Hub; this doc concerns a local `buildx --load`'d helper image's single
  baked-in platform not matching a sibling local build's `--platform` flag.
