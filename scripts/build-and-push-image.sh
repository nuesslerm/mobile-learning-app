#!/usr/bin/env bash
set -euo pipefail

# Local build+push for a deployable image, mirroring
# .github/workflows/build-images.yml's per-app build step exactly (same
# Dockerfile, same repo-root context, same tag scheme) -- the sandbox-drivable
# fallback for when that CI workflow can't run (e.g. a GitHub Actions billing
# outage). No SSH involved. Follow with:
#   curl -X POST -H "Authorization: Bearer <token>" \
#     "https://<vm-ip>.sslip.io/services/<app>/redeploy?tag=<tag>"
# (infra/oracle-cloud/vm-ops-api) to pull + run the pushed tag on a VM -- but
# only once that app already has a docker-compose file + .env on the VM's
# filesystem (see that endpoint's own docs for the current provisioning gap).
#
# Usage: ./scripts/build-and-push-image.sh <app> [tag]
#   app: remote-chat-agent | opencode-server | leetcode-backend | leetcode-auth-cookie-capture | reporting-service | vnc-auth-capture
#   tag: defaults to sha-<short-sha>

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

APP="${1:-}"
case "$APP" in
  agent-base|remote-chat-agent|opencode-server|leetcode-backend|leetcode-auth-cookie-capture|reporting-service|vnc-auth-capture) ;;
  *)
    echo "Usage: $0 <app> [tag]" >&2
    echo "  app must be one of: agent-base, remote-chat-agent, opencode-server, leetcode-backend, leetcode-auth-cookie-capture, reporting-service, vnc-auth-capture" >&2
    exit 1
    ;;
esac

# opencode-server is the one app whose Dockerfile does not sit at
# backend/<app>/Dockerfile -- it is remote-chat-agent's own dependency (the
# `opencode serve` process its gateway dials) and lives under that package.
# It still builds from the repo ROOT context like every other app, so only the
# Dockerfile path differs. Same shape as vm-ops-api's APP_REPO_PATH_OVERRIDE.
if [[ "$APP" == "opencode-server" ]]; then
  DOCKERFILE="backend/remote-chat-agent/opencode-server/Dockerfile"
elif [[ "$APP" == "agent-base" ]]; then
  # Not an app -- the shared foundation the Node images build FROM. It ships no
  # service of its own; building it is how a Node/pnpm/apt-bootstrap change
  # reaches every consumer at once instead of being copy-pasted into each.
  DOCKERFILE="infra/docker/agent-base/Dockerfile"
elif [[ "$APP" == "vnc-auth-capture" ]]; then
  # A clis/* package, not a backend/* app -- its Dockerfile lives alongside
  # the package it builds, same shape as opencode-server's override above.
  DOCKERFILE="clis/vnc-auth-capture/Dockerfile"
else
  DOCKERFILE="backend/${APP}/Dockerfile"
fi
IMAGE_NAMESPACE="${IMAGE_NAMESPACE:-nuesslerm}"
IMAGE="${IMAGE_NAMESPACE}/${APP}"
TAG="${2:-$(git rev-parse --short HEAD 2>/dev/null | sed 's/^/sha-/' || true)}"
if [[ -z "$TAG" ]]; then
  echo "[build-and-push-image] Could not derive a tag (not a git checkout?) -- pass one explicitly: $0 $APP <tag>" >&2
  exit 1
fi

command -v docker >/dev/null 2>&1 || { echo "[build-and-push-image] docker not found on PATH" >&2; exit 1; }
if ! docker info >/dev/null 2>&1; then
  echo "[build-and-push-image] no Docker daemon reachable. In this sandbox: dockerd > /tmp/dockerd.log 2>&1 &  then retry." >&2
  exit 1
fi

# Registry-backed BuildKit cache (not just this daemon's local layer cache), on the same `<image>:buildcache` ref that wf's VM builds
# and the vm-ops-api self-build read and write (clis/deploy/src/build_image.rs), so a sandbox build warms the VM's and the reverse, and a
# recreated VM or a fresh sandbox starts warm. The repositories are public, so the cache is public.
# Requires a buildx builder using the docker-container driver (the default
# "docker" driver can't export cache to a registry) to be active, e.g.:
#   docker buildx create --name registry-cache-builder --driver docker-container --use
#
# --platform is pinned to linux/arm64 unconditionally -- both production VMs
# (light, heavy) are Oracle Cloud Ampere A1 (arm64) instances, and a plain
# `docker buildx build` with no --platform targets whatever architecture the
# *building* host happens to be, silently overwriting :latest with an
# incompatible manifest the next time this runs from a different-arch
# machine. See docs/solutions/build-errors/docker-image-arch-mismatch-arm64-vms.md
# for the incident this caused. If the build host isn't natively arm64,
# QEMU emulation must be registered first: `docker run --rm --privileged
# tonistiigi/binfmt --install arm64`.
TARGET_PLATFORM="${TARGET_PLATFORM:-linux/arm64}"
CACHE_REF="${IMAGE}:buildcache"
# mode=max, NOT mode=min. These are multi-stage Dockerfiles: apt, `pnpm fetch`
# and `pnpm install` all run in the `build` stage, and none of their layers
# survive into the final image -- which is exactly the set `mode=min` exports.
# So with mode=min the registry cache could only ever restore the handful of
# final-image layers, and every expensive step re-ran on a clean builder.
# Measured: a second build of an unchanged tree restored 5 layers and still
# spent ~10min in `pnpm install` alone under arm64 emulation. mode=max exports
# every stage, which is what makes those steps restorable at all. It costs a
# larger cache image and a slower export -- a trivial trade against 10 minutes
# of emulated install.
BUILD_ARGS=(--platform "${TARGET_PLATFORM}" --network host --allow network.host --cache-to "type=registry,ref=${CACHE_REF},mode=max,oci-mediatypes=true,compression=zstd,ignore-error=true" --cache-from "type=registry,ref=${CACHE_REF}")

# Per-image build args, e.g. remote-chat-agent's WHISPER_MODELS (which whisper.cpp
# models the image bakes in). The Dockerfile
# documents that knob, but until now nothing here could set it, so the only way
# to build anything other than the default was to bypass this script -- and a
# path only some callers take is exactly the wrapper smell CLAUDE.md warns about,
# since every other caller silently misses whatever setup it does (CA trust,
# cache refs, platform).
#
#   EXTRA_BUILD_ARGS="WHISPER_MODELS=base.en" ./scripts/build-and-push-image.sh remote-chat-agent <tag>
#
# Space-separated KEY=VALUE pairs. Values containing spaces are not supported;
# nothing needs them today and quoting them through word-splitting is a trap.
if [[ -n "${EXTRA_BUILD_ARGS:-}" ]]; then
  for _arg in ${EXTRA_BUILD_ARGS}; do
    BUILD_ARGS+=(--build-arg "${_arg}")
    echo "[build-and-push-image] extra build arg: ${_arg}"
  done
fi
CLEANUP_CA=""

# Runs on EVERY exit, not just success. The cache cap used to be a plain
# command after `docker buildx build`, which `set -e` skips entirely when the
# build fails — so a run of failed builds grew the builder cache unbounded
# until the next one died with ERR_PNPM_ENOSPC, a failure that looks nothing
# like "the cache is full". A failed build is exactly when the cap matters
# most, since its partial layers are still written.
cleanup() {
  local status=$?
  [[ -n "$CLEANUP_CA" ]] && rm -f $CLEANUP_CA
  "$REPO_ROOT/scripts/docker-cache-cleanup.sh" || true
  return $status
}
trap cleanup EXIT
# The CA-patched Dockerfile below fixes what happens INSIDE a build stage. It
# cannot fix the builder itself: with the docker-container driver, buildkit runs
# in its own container with its own CA store, and it is buildkit — not the
# daemon, not a build stage — that resolves `# syntax=docker/dockerfile:1` and
# every base-image manifest. Behind a TLS-intercepting proxy that store is
# missing the CA, so a fresh sandbox fails before the first stage runs:
#
#   ERROR: failed to resolve source metadata for docker.io/docker/dockerfile:1:
#     tls: failed to verify certificate: x509: certificate signed by unknown authority
#
# Creating the builder is a documented manual step (see the comment above and
# docs/solutions/tooling-decisions/backend-docker-build-pnpm-fetch-cache-optimization.md)
# that never mentioned the CA, so every fresh sandbox hit this. Appending to the
# running builder's store and restarting it is idempotent — the marker makes a
# re-run a no-op — and it patches whichever builder is active rather than
# imposing a new one.
# The build steps reach the outside world only through the sandbox's proxy,
# which listens on HOST loopback (127.0.0.1:<port>). With the docker-container
# driver, `--network host` on a BUILD means the buildkit container's namespace,
# not the host's — so a builder on the default bridge network cannot reach the
# proxy at all, and every apt/curl step fails with
#   Could not connect to 127.0.0.1:<port> (111: Connection refused)
# which reads like the proxy is down rather than unreachable. A buildkit
# container created with `network=host` shares the host namespace, so loopback
# means the same thing on both sides. Provisioned here rather than left to
# whoever created the ambient builder, because the ambient one is wrong by
# default and the failure it produces points nowhere near the cause.
SANDBOX_BUILDER="sandbox-proxy"
ensure_sandbox_builder() {
  if ! docker buildx inspect "$SANDBOX_BUILDER" >/dev/null 2>&1; then
    echo "[build-and-push-image] Creating buildx builder '$SANDBOX_BUILDER' (docker-container, host network)..."
    docker buildx create --name "$SANDBOX_BUILDER" --driver docker-container \
      --driver-opt network=host --bootstrap >/dev/null
  fi
  docker buildx use "$SANDBOX_BUILDER"
}

ensure_builder_trusts_proxy_ca() {
  local node container marker=/etc/ssl/certs/.sandbox-proxy-ca-installed
  node="$(docker buildx inspect --bootstrap 2>/dev/null | awk '/^Nodes:/{f=1} f && /^Name:/{print $2; exit}')"
  [[ -z "$node" ]] && { echo "[build-and-push-image] could not resolve the active buildx node; skipping CA injection" >&2; return 0; }
  container="buildx_buildkit_${node}"
  # The `docker` driver runs buildkit inside the daemon, which already trusts
  # the host CA store — there is no separate container to patch.
  docker ps --format '{{.Names}}' | grep -qx "$container" || {
    echo "[build-and-push-image] active builder has no buildkit container ($container); assuming it uses the daemon's CA store." >&2
    return 0
  }
  docker exec "$container" test -f "$marker" 2>/dev/null && return 0
  echo "[build-and-push-image] Trusting the sandbox proxy CA inside $container..."
  docker exec -i "$container" sh -c "cat >> /etc/ssl/certs/ca-certificates.crt && touch $marker" < /root/.ccr/ca-bundle.crt
  # Go caches the system roots on first use, so buildkitd has to be restarted
  # to pick the appended CA up.
  docker restart "$container" >/dev/null
  docker buildx inspect --bootstrap >/dev/null
}

DOCKERFILE_TO_BUILD="$DOCKERFILE"
if [[ -n "${CCR_AGENT_PROXY_ENABLED:-}" && -f /root/.ccr/ca-bundle.crt ]]; then
  ensure_sandbox_builder
  ensure_builder_trusts_proxy_ca
  # Same sandbox-proxy MITM issue as vm-ops-api's old build-and-push.sh,
  # confirmed here too (npm: SELF_SIGNED_CERT_IN_CHAIN fetching registry.npmjs.org):
  # these Dockerfiles' base images don't trust the sandbox's TLS-intercepting CA, so
  # any HTTPS fetch during the build (npm/corepack, apt, node/gh binary downloads)
  # fails without both routing through the proxy AND trusting its CA. Unlike
  # vm-ops-api's trivial image, these are real multi-stage production Dockerfiles --
  # rather than hand-copy each one, patch a temp copy by inserting a CA-trust step
  # right after the FIRST `FROM` line (every later stage in all 4 Dockerfiles derives
  # from that first stage, directly or transitively, so the trust carries through to
  # every subsequent RUN/curl that needs it). Never touches the committed file.
  # Only HTTPS_PROXY -- per /root/.ccr/README.md, the proxy only speaks CONNECT
  # (HTTPS tunneling); a plain-HTTP request routed through it via HTTP_PROXY gets
  # a 405, which is exactly what apt's default http:// Debian mirrors hit (see
  # the sed step below, which switches them to https:// so they go through
  # HTTPS_PROXY correctly instead).
  BUILD_ARGS+=(--build-arg "HTTPS_PROXY=${HTTPS_PROXY:-}")
  # `vnc-auth-capture`'s Dockerfile additionally opts into CA trust per-RUN-step
  # via `--mount=type=secret,id=extra_ca_cert,required=false` (rather than the
  # patched-copy approach below, which only reaches the base image's own apt/OS
  # trust store) -- confirmed live that omitting this secret left
  # NODE_EXTRA_CA_CERTS pointing at nothing, so pnpm's registry fetch still hit
  # `SELF_SIGNED_CERT_IN_CHAIN` even with the patched Dockerfile and a correct
  # HTTPS_PROXY. `required=false` on the Dockerfile side means this is a no-op
  # for the other 4 apps' Dockerfiles, which don't declare that mount at all.
  BUILD_ARGS+=(--secret "id=extra_ca_cert,src=/root/.ccr/ca-bundle.crt")
  echo "[build-and-push-image] Sandbox proxy detected -- building from a CA-trust-patched copy of ${DOCKERFILE} (not committed)."
  cp /root/.ccr/ca-bundle.crt ./.sandbox-proxy-ca.crt
  CLEANUP_CA="./.sandbox-proxy-ca.crt"
  PATCHED_DOCKERFILE="$(mktemp)"
  CLEANUP_CA="$CLEANUP_CA $PATCHED_DOCKERFILE"
  # Patched after EVERY stage built from an EXTERNAL image, not just the first
  # FROM in the file. It used to be the first one only, on the reasoning that
  # every later stage derives from it — which stopped being true the moment a
  # Dockerfile gained a leading helper stage nothing inherits from (this repo's
  # `FROM node:22.14.0 AS aptcerts`). The trust then landed on a dead end and
  # the real base image kept none of it, so the build died much later with a
  # bare `curl ... exit code 60` fetching the gh CLI. Tracking declared stage
  # names and patching only the FROMs that name something else makes it
  # structural rather than dependent on stage ordering.
  awk '
    function trust_ca() {
      print "RUN for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources /etc/apt/sources.list.d/*.list; do [ -f \"$f\" ] && sed -i \"s#http://#https://#g\" \"$f\"; done; true"
      print "COPY .sandbox-proxy-ca.crt /usr/local/share/ca-certificates/sandbox-proxy.crt"
      print "RUN apt-get update -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false && apt-get install -y --no-install-recommends -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false ca-certificates && update-ca-certificates && rm -rf /var/lib/apt/lists/*"
      print "ENV NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/sandbox-proxy.crt"
    }
    /^FROM / {
      print
      image = ""
      for (i = 2; i <= NF; i++) { if ($i ~ /^--/) continue; image = $i; break }
      for (i = 2; i < NF; i++) if (tolower($i) == "as") stages[$(i + 1)] = 1
      if (!(image in stages)) trust_ca()
      next
    }
    { print }
  ' "$DOCKERFILE" > "$PATCHED_DOCKERFILE"
  DOCKERFILE_TO_BUILD="$PATCHED_DOCKERFILE"
fi

# An app that ships as a single pre-bundled file builds that bundle HERE, on
# the host, before docker is invoked. Doing it inside the image meant compiling
# under QEMU emulation AND re-resolving the dependency graph with `pnpm deploy`
# -- ~17 minutes for vnc-auth-capture, re-run on every source edit. A bundle is
# plain arch-independent JS, so it builds at native speed.
#
# Keyed on the package having a `bundle:build` script, not on the app's name.
# This lives here rather than in a caller because every path to a built image
# goes through here, so none can skip it (the missing-bundle failure would be
# a COPY error deep in the build).
PROJECT_DIR="$(dirname "$DOCKERFILE")"
if [[ -f "$PROJECT_DIR/package.json" ]] \
  && node -e "process.exit(require('./$PROJECT_DIR/package.json').scripts?.['bundle:build'] ? 0 : 1)" 2>/dev/null; then
  PROJECT_NAME="$(node -p "require('./$PROJECT_DIR/package.json').name")"
  echo "[build-and-push-image] ${PROJECT_NAME} ships as a bundle -- building it on the host first..."
  pnpm --filter "${PROJECT_NAME}^..." run build
  pnpm --filter "${PROJECT_NAME}" run bundle:build
fi

echo "[build-and-push-image] Building + pushing ${IMAGE}:${TAG} from ${DOCKERFILE} (repo-root context, cache: ${CACHE_REF})..."
docker buildx build "${BUILD_ARGS[@]}" -f "$DOCKERFILE_TO_BUILD" -t "${IMAGE}:${TAG}" -t "${IMAGE}:latest" --push .

echo "[build-and-push-image] Done. Run it on a VM (once provisioned there) with:"
echo "  curl -X POST -H \"Authorization: Bearer <token>\" \"https://<vm-ip>.sslip.io/services/${APP}/redeploy?tag=${TAG}\""
