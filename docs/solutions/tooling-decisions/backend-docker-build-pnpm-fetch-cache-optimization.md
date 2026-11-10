---
title: "Fast, cache-warm backend Docker builds in a pnpm-workspace monorepo"
date: 2026-07-30
category: tooling-decisions
module: backend-docker-builds
problem_type: tooling_decision
component: tooling
severity: medium
applies_when:
  - "A Docker build in this repo again installs workspace packages with pnpm (no backend Dockerfile has done so since the Rust rewrite)"
  - "A Docker build in this repo needs to resolve workspace:* deps via pnpm --filter"
  - "Building for arm64 (the production Oracle Cloud VM architecture) for the first time"
  - "Iterating on a sandbox-drivable build script with no persistent CI cache available"
  - "Choosing whether to export BuildKit cache to a registry for cold-sandbox reuse"
related_components:
  - backend/remote-chat-agent
  - backend/leetcode-backend
  - backend/leetcode-auth-cookie-capture
  - infra/oracle-cloud/vm-ops-api
tags: [docker, pnpm-fetch, buildx, arm64, node-gyp, cache-mount, monorepo]
---

# Fast, cache-warm backend Docker builds in a pnpm-workspace monorepo

Status note: this doc records the Node-era backend Dockerfiles as they stood on
`claude/oracle-cloud-agent-provisioning-scaffold` (2026-07-30), when none of these
Dockerfile/script changes had been merged to `main`. They reached `main` through
PR #175, merged on 2026-08-05 as `d152561c` (its `backend/leetcode-backend/Dockerfile`
runs `pnpm fetch --store-dir=/pnpm-store --ignore-scripts` and the offline install at
lines 52-56), before the Rust builders superseded them. The backends have since been
rewritten in Rust, and the `pnpm fetch` layering it describes is superseded: no
Dockerfile in the current tree runs `pnpm fetch` or uses the `pnpm-store` cache
mount. `leetcode-backend`, `leetcode-auth-cookie-capture` and `remote-chat-agent`
each compile one Rust binary in a `FROM docker.io/library/rust:1-bookworm` builder
stage with `cargo build --profile deploy`, behind a cargo registry cache mount and a
per-service target cache mount (`backend/leetcode-backend/Dockerfile:14-39`,
`backend/leetcode-auth-cookie-capture/Dockerfile:11-32`,
`backend/remote-chat-agent/Dockerfile:26-51`). Snippets and `file:line` references in
sections 1-4 describe the 2026-07-30 tree; where a fact has changed since, the
current state is given beside it.

## Context

On 2026-07-30 all four backend apps (`backend/remote-chat-agent`, the
since-deleted opencode Telegram bot, `backend/leetcode-backend`,
`backend/leetcode-auth-cookie-capture`) plus the small ops helper image at
`infra/oracle-cloud/vm-ops-api` built **from the repo root**, not the app's own
directory — every backend Dockerfile said so in its header comment, because pnpm had
to resolve each app's `workspace:*` dependencies (the since-deleted TypeScript
secrets CLI package, `@lib/logging`, `@app/*-contracts`, etc.) against the single
root `pnpm-lock.yaml`/`pnpm-workspace.yaml`.

The three backend Dockerfiles that remain from those four, plus the newer
`backend/reporting-service/Dockerfile`, build from the repo root and say so in their
header comments, but for a different reason: the Rust workspace manifest lives in
`backend/` (`backend/leetcode-backend/Dockerfile:3`,
`backend/remote-chat-agent/Dockerfile:3-4`, `backend/reporting-service/Dockerfile:3`;
`clis/deploy/src/build_image.rs:33-37` gives `reporting-service` the context `.`). Each builder copies `backend/Cargo.toml`
and `backend/Cargo.lock`, every workspace member, and the path crates under `clis/`
(`clis/telemetry`, `libraries/rca-core`), because Cargo loads every member's manifest even
for a `-p` build; no pnpm file is copied. `vm-ops-api` builds from the repo root like
the others (`backend/vm-ops-api/Dockerfile`, `clis/deploy/src/build_image.rs:38-42`). Until
the Node service was retired (Oct 2026) its image built from its own directory
(`build-and-push.sh` did `cd "$SCRIPT_DIR"` before building `.`) and its Dockerfile
compiled the TypeScript with a standalone `npm install` of `typescript` rather than
through the pnpm workspace.

The starting problem: each Dockerfile's build stage did one `COPY . .` — the
entire repo-root context — immediately before
`RUN pnpm install --frozen-lockfile --filter=@app/X...`. Since the build context is
the whole monorepo, any change anywhere (an unrelated frontend component, a doc, a
different backend app) busted that layer and forced a full `pnpm install`
reinstall of every dependency, even when the app's dependency closure hadn't
changed.

## Guidance

Sections 1-4 are the Node-era fix as it was applied on 2026-07-30. None of it is in a
current Dockerfile; it is the pattern to start from if a Docker build here installs
workspace packages with pnpm again.

**1. Two-step `COPY` via `pnpm fetch`, gated on the lockfile alone.**

Every backend Dockerfile of the time split the old single `COPY . .` + `pnpm install`
into a dependency-only fetch step followed by full source copy + offline install:

```dockerfile
COPY pnpm-lock.yaml pnpm-workspace.yaml ./
COPY patches ./patches
RUN --mount=type=cache,id=pnpm-store,target=/pnpm-store \
    pnpm fetch --store-dir=/pnpm-store --ignore-scripts
COPY . .
RUN --mount=type=cache,id=pnpm-store,target=/pnpm-store \
    pnpm install --frozen-lockfile --offline --store-dir=/pnpm-store --filter=@app/<name>...
```

`pnpm fetch` populates a content-addressable store purely from the lockfile —
`package.json` is deliberately ignored by `pnpm fetch`'s design — so this layer (and
the persistent cache mount backing it) invalidated only when a dependency actually
changed, not on every source edit. `--mount=type=cache,id=pnpm-store,...` is a
BuildKit **cache mount**, not a regular image layer, so the populated store survives
even a full image rebuild (`docker build --no-cache`), not merely repeat builds that
hit Docker's own layer cache.

The cache mount `id` was the literal string `pnpm-store`, identical across the four
backend Dockerfiles of the time. All four resolved against the same root
`pnpm-lock.yaml`, so a package fetched while building one app's image was already
warm in the shared store for the other three. The Rust builders keep the shared idea
for the cargo registry (a `/usr/local/cargo/registry` cache mount with no `id`, so the
same mount in every builder). Their per-service target-directory mounts (`leetcode-backend-target`
and the like) are gone: a mount is never exported to the registry cache, so each builder now compiles the
dependencies into an image layer (`cargo chef cook`), see `docs/plans/blob-cache/plan.md`, "Docker layers via Docker Hub".

**2. `pnpm fetch` needs `pnpm-workspace.yaml` and the real `patches/` directory —
not just the lockfile.**

On 2026-07-30, `pnpm-workspace.yaml` declared `patchedDependencies` (then around
lines 104-107):

```yaml
patchedDependencies:
  '@storybook/react-native-ui@10.4.4': patches/@storybook__react-native-ui@10.4.4.patch
  react-native-css-interop@0.2.5: patches/react-native-css-interop@0.2.5.patch
  skills@1.5.13: patches/skills@1.5.13.patch
```

and `patches/` at repo root holds the actual `.patch` files. `pnpm fetch` applies
patches while populating the store, so both `pnpm-workspace.yaml` (where
`patchedDependencies` is declared) and the real `patches/` tree are **required
inputs to the fetch step**, even though `package.json` is not. Omitting either
produces `[ERR_PNPM_PATCH_FILE_PATH_MISSING]` — pnpm resolves the patch mapping
but can't find the patch file on disk to apply it.

The block is now at `pnpm-workspace.yaml:121-137` and has eight entries: the three
above plus `lucide-react-native@1.17.0`, `sonner-native@0.26.4`,
`@quidone/react-native-wheel-picker@1.7.1`, `@gluestack-ui/core@5.0.15` and
`@gorhom/bottom-sheet@5.2.14`. No Docker build copies `pnpm-workspace.yaml` or
`patches/` any more, so this only matters again if a Dockerfile goes back to
`pnpm fetch`.

**3. Pass `--ignore-scripts` to `pnpm fetch` — it isn't a build step.**

`pnpm fetch`'s only job is to warm the store with package tarballs, but by default it
still executes install/postinstall scripts, including native compilation attempts.
All four Dockerfiles of the time called it as
`pnpm fetch --store-dir=/pnpm-store --ignore-scripts`. Script execution correctly
happened later, in the real `pnpm install --offline ...` step after the full
`COPY . .` — that step already has pnpm's normal tolerance for optional-dependency
build failures, whereas failing inside `fetch` would abort the whole build on a
dependency the app may not even need built from source.

**4. arm64 native-toolchain requirement for optional native deps — a real,
independent gap, not just an `--ignore-scripts` artifact.**

Three of the four backend apps of the time transitively depended on `ws`, which
declares `bufferutil` and `utf-8-validate` as optional peer dependencies
(`peerDependencies` plus `peerDependenciesMeta`, `pnpm-lock.yaml:10323-10345` in the current tree),
resolved here as `bufferutil@4.1.0` / `utf-8-validate@5.0.10`. These old pinned
native-accelerator versions ship no prebuilt binary for `linux-arm64`, so node-gyp
always falls back to compiling from source on that architecture — unconditional on
arm64, independent of `--ignore-scripts`, and it resurfaced at the real
`pnpm install`/`deploy` step if the build stage lacked a C/C++ toolchain and Python.
All four build stages then installed it up front (mirroring `remote-chat-agent`'s
pre-existing pattern for its own `whisper-node` dependency):

```dockerfile
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
      python3 make g++ ca-certificates
```

Confirmed fixed end-to-end: `leetcode-backend`'s image built successfully through
every step after this change and was confirmed pushed with an arm64 manifest
(`docker manifest inspect nuesslerm/leetcode-backend:sha-d3e4330c` showed a manifest
list entry with `"architecture": "arm64"`; `d3e4330c` is the commit this image was
tagged from, part of PR #175 — open and unmerged as of this writing; merged into
`main` on 2026-08-05 as `d152561c`).

No backend builder stage installs that toolchain now: the builders are
`rust:1-bookworm` cargo builds with no Node dependency install, so the `ws`
native-accelerator path is not exercised in any backend image. The toolchain itself
survives, for the same packages, in `infra/oracle-cloud/agent-shell/Dockerfile:143-171`:
that image runs on the arm64 VM and installs `python3`, `make` and `g++` as
"node-gyp's build deps for the native modules this monorepo's pnpm install pulls in
(bufferutil, utf-8-validate)", with plain `apt-get`, no apt cache mount, and a closing
`rm -rf /var/lib/apt/lists/*`. The only install of `make`/`g++` in a backend image is
`remote-chat-agent`'s `whisper` stage, which installs `make g++ ca-certificates` (no
`python3`) to compile whisper.cpp from the `whisper-node` package's vendored sources,
not `ws`'s accelerators (`backend/remote-chat-agent/Dockerfile:64-66`).

**5. apt-get installs and BuildKit cache mounts.**

On 2026-07-30 every `apt-get`/`apk` install step across the five Dockerfiles of the
time paired `--mount=type=cache,target=/var/cache/apt,sharing=locked` with
`--mount=type=cache,target=/var/lib/apt/lists,sharing=locked` (apt), or
`--mount=type=cache,target=/var/cache/apk` (apk, `vm-ops-api`), and the old manual
`rm -rf /var/lib/apt/lists/*` cleanup was dropped. Cache-mounted content never lands
in the final image layer, so that cleanup is unnecessary wherever the lists directory
is itself a cache mount. The current Dockerfiles are mixed:

- `leetcode-auth-cookie-capture`'s runtime stage installs
  `xvfb`/`x11vnc`/`novnc`/`websockify` behind both apt cache mounts
  (`backend/leetcode-auth-cookie-capture/Dockerfile:53-56`).
- `remote-chat-agent`'s runtime stage installs `util-linux`/`poppler-utils`/
  `tesseract-ocr`/`tesseract-ocr-eng`/`sqlite3` behind both mounts
  (`backend/remote-chat-agent/Dockerfile:108-112`), as does its `whisper` stage.
  `git`, `ca-certificates` and `curl` now come from the shared agent-base image,
  which installs them the same way (`infra/docker/agent-base/Dockerfile:61-64`).
- `leetcode-backend`'s runtime stage mounts `/var/cache/apt` and `/var/lib/apt` and
  still ends with `rm -rf /var/lib/apt/lists/*`
  (`backend/leetcode-backend/Dockerfile:68-73`).
- The optional proxy-CA steps in `leetcode-backend` and `leetcode-auth-cookie-capture`
  run plain `apt-get` with no cache mount and clean up with
  `rm -rf /var/lib/apt/lists/*`.
- vm-ops-api's Node-era Dockerfile (deleted with the Node service in Oct 2026) was
  `FROM node:22-bookworm-slim` (Debian, apt, no apk). It ran plain `apt-get` with no
  cache mount plus `rm -rf /var/lib/apt/lists/*`, and copied the docker CLI and
  compose plugin from `docker:26-cli` instead of installing them. The sandbox branch
  of its `build-and-push.sh` injected an `apt-get` line, not an `apk` one.

**6. Registry-backed BuildKit cache for cross-sandbox / cross-machine builds.**

`scripts/build-and-push-image.sh` and vm-ops-api's `build-and-push.sh` (deleted with the Node service)
switched from separate `docker build` + `docker push` calls to a single combined
`docker buildx build` invocation with registry cache flags. On 2026-07-30 both used
`mode=min`; the current `scripts/build-and-push-image.sh` (lines 82-93 and 268) reads:

```bash
TARGET_PLATFORM="${TARGET_PLATFORM:-linux/arm64}"
CACHE_REF="${IMAGE}:buildcache"
BUILD_ARGS=(--platform "${TARGET_PLATFORM}" --network host --allow network.host --cache-to "type=registry,ref=${CACHE_REF},mode=max,oci-mediatypes=true,compression=zstd,ignore-error=true" --cache-from "type=registry,ref=${CACHE_REF}")
...
docker buildx build "${BUILD_ARGS[@]}" -f "$DOCKERFILE_TO_BUILD" -t "${IMAGE}:${TAG}" -t "${IMAGE}:latest" --push .
```

`${IMAGE}:buildcache` is the ref and the attribute set the VM's builds use
(`clis/deploy/src/build_image.rs`, `Mode::ExportCache`), so a sandbox build and a VM build
warm each other and a registry that refuses the cache write no longer fails a push.
Until 23 Oct 2026 both scripts used a second repository, `${IMAGE}-cache` (the
sandbox `agent-shell` script with `mode=min`), which nothing reads any more.
On the VM the cache is written by its own job: after a redeploy, and since unit B9a
after a waited `wf ship --build-only` too (`--no-wait` cannot). The audit of every
build path and the Docker Hub limits are in `docs/plans/blob-cache/plan.md`
(section "Docker layers via Docker Hub").

vm-ops-api's `build-and-push.sh` built the same array with
`mode=min` and an extra `--build-arg "VM_OPS_API_IMAGE_TAG=${TAG}"`.

Requires an active buildx builder using the `docker-container` driver (the default
`docker` driver cannot export cache to a registry):

```bash
docker buildx create --name registry-cache-builder --driver docker-container --use
```

## Why This Matters

- With the 2026-07-30 layering, a source-only change to one backend app's code
  skipped the network-bound dependency-fetch step entirely — it hit the cheap,
  offline `pnpm install --offline --store-dir=/pnpm-store` link step instead of
  re-downloading the whole dependency graph, since the `pnpm fetch` layer keyed only
  off the lockfile/patches, not the full tree.
- A brand-new sandbox with zero local Docker state was meant to get a warm dependency
  cache on its first build: the pnpm store was a persistent BuildKit cache mount
  (survives full rebuilds) and the image cache round-trips through a registry
  (survives even a fresh sandbox with no prior local Docker history).
- A real, production-blocking gap — arm64 native compilation for `bufferutil`/
  `utf-8-validate` behind `ws` — was caught and fixed this session, before it could
  silently block every future deploy of three of the four production backend apps on
  the target Oracle Cloud arm64 (Ampere A1) VMs. None of these four apps had ever
  been built for arm64 before this session (none had ever been deployed to those VMs
  at all), so nothing had previously exercised this path.

## When to Apply

- Adding a pnpm install of workspace packages back into a Docker build in this
  monorepo (no backend Dockerfile has one since the Rust rewrite) — start from the
  `pnpm fetch` layering in section 1, with the section 2 inputs.
- Building any of these images for arm64 for the first time (or targeting a new
  architecture generally) — check whether any dependency's pinned optional (peer)
  native accelerator lacks a prebuilt binary for that arch,
  and add the `python3 make g++ ca-certificates` toolchain to the build stage if so.
- Iterating in a sandbox or CI runner with no persistent local Docker daemon state
  (fresh containers/VMs each run) — use the registry-backed buildx cache, not just
  layer caching.
- Adding a new entry to `pnpm-workspace.yaml`'s `patchedDependencies` while a Docker
  build runs `pnpm fetch` — the fetch step needs the same `pnpm-workspace.yaml` +
  `patches/` copy treatment as the lockfile; a patch declared but not copied into the
  fetch stage fails with `ERR_PNPM_PATCH_FILE_PATH_MISSING`.

## Examples

**Corrected two-step `COPY` + fetch + offline install** (the Node-era build stage of
2026-07-30, superseded by the Rust builders; the pattern to start from if a Docker
build here installs workspace packages with pnpm again):

```dockerfile
# ── build stage ──
FROM base AS build
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
      python3 make g++ ca-certificates

COPY pnpm-lock.yaml pnpm-workspace.yaml ./
COPY patches ./patches
RUN --mount=type=cache,id=pnpm-store,target=/pnpm-store \
    pnpm fetch --store-dir=/pnpm-store --ignore-scripts
COPY . .
RUN --mount=type=cache,id=pnpm-store,target=/pnpm-store \
    pnpm install --frozen-lockfile --offline --store-dir=/pnpm-store --filter=@app/<name>...
```

**Caveat — `mode=max` vs `mode=min`, a genuinely expensive-to-discover finding.**

BuildKit's registry cache export supports `mode=max` (export every intermediate
layer as its own cache blob, not just the final image's layers) and `mode=min`
(export only the final image's layers). `mode=max` was tried first and, for a single
moderate-sized image (`leetcode-backend`) under this sandbox's network conditions,
the cache-export step alone ran for **over 1800 seconds (30 minutes)** — the actual
image build finished in under 3 minutes; all of that time was purely uploading
intermediate cache blobs. This was genuine slow progress, not a hang: `docker stats`
on the buildx builder container showed low but non-zero, steadily-present CPU
(~0.1-0.6%) with slowly climbing network I/O across repeated checks, and BuildKit's
own daemon log showed `sending cache export 1802.1s done` right as the operation was
cancelled — by which point the actual image push had *already completed*
(separately confirmed via `docker manifest inspect` showing the image tag already
live on Docker Hub with a correct arm64 manifest, despite the cache-export step
still running). Both build scripts then switched to `mode=min` unconditionally — it
exported far less data and was the only practical choice given that sandbox's
bandwidth.

vm-ops-api's `build-and-push.sh` (since deleted) kept `mode=min`.
`scripts/build-and-push-image.sh` has since gone back to `mode=max` (line 93), and its
comment (lines 83-92) gives the reason: in a multi-stage Dockerfile the expensive
steps run in the build stage and none of their layers survive into the final image,
which is exactly the set `mode=min` exports, so a second build of an unchanged tree
restored 5 layers and still spent about 10 minutes in `pnpm install` under arm64
emulation. `mode=max` exports every stage, which is what makes those steps
restorable at all; the comment accepts a larger cache image and a slower export as
the price. The Rust builder stages are in the same position: only the binary copied
out of them reaches the final image. With `mode=max`, budget for the cache-export
step to potentially dominate total build wall-clock time on a bandwidth-constrained
sandbox, rather than assuming cache export is "free" next to the build.

**Note on CI vs. sandbox caching — don't try to unify them.**
The since-deleted `.github/workflows/build-images.yml` used `cache-from`/`cache-to
type=gha` for its own builds — a different mechanism appropriate for GitHub Actions
runners, which have persistent access to the GHA cache namespace. Images are now
built on the VM by `wf ship` in the `deploy.yml` CI pipeline instead. The
registry-backed `type=registry` cache in `scripts/build-and-push-image.sh` (and, until it was deleted, vm-ops-api's
`build-and-push.sh`) exists specifically for the
sandbox/local fallback path, which has no such persistent runner-scoped cache
available. The two were deliberately parallel, not competing, caching strategies for
two different execution environments. What is unified is the registry ref, not the
mechanism: the sandbox script and the VM's `wf ship` both read and write
`docker.io/nuesslerm/<image>:buildcache`.

**Caveat — never run these build scripts concurrently for multiple apps on a
resource-constrained sandbox.** That session's sandbox Docker VM had only 2 CPUs and
~6GB total memory (`docker info` — confirmed live). Rebuilding all five images that
session, one app's build failed (`ResourceExhausted: ... cannot allocate memory`)
after an orchestrating shell loop had no `set -e`: when one app's build errored, the
loop silently continued to the *next* app, so two `docker buildx build`
invocations ran simultaneously against the same buildx builder container, and the
heavier of the two (a Playwright/Chromium-based image) OOM'd. Retrying that same
build **alone**, with no other build running, succeeded cleanly on the first attempt
— the image and its dependency graph were never the problem; the concurrency was.
Two takeaways: (1) treat these builds as inherently serial on a sandbox this size —
don't fan them out in parallel or in a loop without `set -e` (or explicit exit-code
checks) that stops on the first failure; (2) if a build fails with
`cannot allocate memory` / `ResourceExhausted`, check `docker stats` on the buildx
builder container for any other build still holding memory before assuming the
Dockerfile itself is at fault, and consider `docker buildx stop <builder> && docker
buildx rm <builder> && docker buildx create ...` to clear any orphaned build jobs
left behind by a killed client process — a killed `docker buildx build` client does
not always cleanly cancel the corresponding job inside the long-lived builder
container.

## Related

- `docs/archive/solutions/workflow/root-lifecycle-scripts-break-docker-filtered-installs.md`
  — the same Node-era Dockerfiles, a different root cause (root postinstall/prepare
  scripts breaking a scoped `pnpm install --filter` inside Docker). Complementary,
  not overlapping.
- `docs/solutions/workflow/allowbuilds-deny-by-default.md` — covers why
  `bufferutil`/`utf-8-validate` (the same packages behind this doc's arm64
  toolchain finding) are allow-listed to run install scripts at all (a supply-chain
  gate). Different layer: security allow-list vs. native-toolchain availability for
  the arch actually compiling them.
- `docs/solutions/build-errors/docker-image-arch-mismatch-arm64-vms.md` — same
  session, adjacent topic (Oracle Cloud arm64 VMs + Docker), but a distinct root
  cause: that doc is about `vm-ops-api`'s `build-and-push.sh` never using buildx at
  all, so `:latest` had no arm64 manifest in the registry. This doc assumes
  correctly-arm64-targeted builds and covers layer-caching, native-toolchain, and
  registry-cache-cost concerns instead.
