#!/usr/bin/env bash
set -euo pipefail

# Builds agent-shell and pushes it to Docker Hub -- the same "incremental
# Docker Hub image" iteration path the old repo-root script gave the backend apps.
# No SSH involved. Follow with:
#   curl -X POST -H "Authorization: Bearer <token>" \
#     "https://<vm-ip>.sslip.io/services/agent-shell/redeploy?tag=<tag>"
#
# Written after this image had to be built ad hoc with no dedicated script
# at all -- unlike vm-ops-api and the 5 backend apps (the old repo-root script),
# agent-shell had neither its own script nor an entry in the shared one.
# This closes that gap.
#
# Usage: ./build-and-push.sh [tag]
#   tag defaults to the current git short SHA -- pass one explicitly to be sure.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
cd "$REPO_ROOT"

IMAGE_NAMESPACE="${IMAGE_NAMESPACE:-nuesslerm}"
IMAGE_REPO="agent-shell"
TAG="${1:-$(git rev-parse --short HEAD 2>/dev/null | sed 's/^/sha-/' || true)}"
if [[ -z "$TAG" ]]; then
  echo "[build-and-push] Could not derive a tag (not a git checkout?) -- pass one explicitly: ./build-and-push.sh <tag>" >&2
  exit 1
fi
IMAGE="${IMAGE_NAMESPACE}/${IMAGE_REPO}"
DOCKERFILE="infra/oracle-cloud/agent-shell/Dockerfile"

command -v docker >/dev/null 2>&1 || { echo "[build-and-push] docker not found on PATH" >&2; exit 1; }
if ! docker info >/dev/null 2>&1; then
  echo "[build-and-push] no Docker daemon reachable. In this sandbox: dockerd > /tmp/dockerd.log 2>&1 &  then retry." >&2
  exit 1
fi

# --platform pinned to linux/arm64 unconditionally -- see
# docs/solutions/build-errors/docker-image-arch-mismatch-arm64-vms.md for
# the incident a plain, unpinned `docker buildx build` caused elsewhere in
# this repo. Root build context: the Dockerfile's COPY paths are relative to the
# repository root.
TARGET_PLATFORM="${TARGET_PLATFORM:-linux/arm64}"
# The ref and attributes wf's VM build (clis/deploy/src/build_image.rs) uses: mode=max, because mode=min exports only the final-image layers.
CACHE_REF="${IMAGE}:buildcache"
BUILD_ARGS=(--platform "${TARGET_PLATFORM}" --network host --allow network.host --cache-to "type=registry,ref=${CACHE_REF},mode=max,oci-mediatypes=true,compression=zstd,ignore-error=true" --cache-from "type=registry,ref=${CACHE_REF}")

# Space-separated KEY=VALUE build args. The one that matters here is AGENT_BASE_TAG:
# this Dockerfile now builds FROM agent-base, which defaults to :latest, so
# pinning a specific base is otherwise unreachable from the command line.
#
#   EXTRA_BUILD_ARGS="AGENT_BASE_TAG=sha-706958f" ./build-and-push.sh <tag>
if [[ -n "${EXTRA_BUILD_ARGS:-}" ]]; then
  for _arg in ${EXTRA_BUILD_ARGS}; do
    BUILD_ARGS+=(--build-arg "${_arg}")
    echo "[build-and-push] extra build arg: ${_arg}"
  done
fi

CLEANUP_CA=""

# The Dockerfile-stage CA patch below only fixes what happens INSIDE a
# build stage. It can't fix the builder itself: with the docker-container
# driver, buildkit runs in its own container with its own CA store, and
# it's buildkit -- not a build stage -- that resolves every base-image
# manifest (agent-base, docker:26-cli). Behind this
# sandbox's TLS-intercepting proxy that store is missing the CA, so a
# fresh sandbox fails before the first stage runs -- confirmed hitting this
# exact failure with vm-ops-api's old build-and-push.sh before
# it got this same fix ported in:
#   ERROR: ... failed to resolve source metadata ...
#     tls: failed to verify certificate: x509: certificate signed by unknown authority
# Uses the identical builder name that script uses, so a sandbox that
# already ran either one reuses it instead of provisioning a second one.
SANDBOX_BUILDER="sandbox-proxy"
ensure_sandbox_builder() {
  if ! docker buildx inspect "$SANDBOX_BUILDER" >/dev/null 2>&1; then
    echo "[build-and-push] Creating buildx builder '$SANDBOX_BUILDER' (docker-container, host network)..."
    docker buildx create --name "$SANDBOX_BUILDER" --driver docker-container \
      --driver-opt network=host --bootstrap >/dev/null
  fi
  docker buildx use "$SANDBOX_BUILDER"
}

ensure_builder_trusts_proxy_ca() {
  local node container marker=/etc/ssl/certs/.sandbox-proxy-ca-installed
  node="$(docker buildx inspect --bootstrap 2>/dev/null | awk '/^Nodes:/{f=1} f && /^Name:/{print $2; exit}')"
  [[ -z "$node" ]] && { echo "[build-and-push] could not resolve the active buildx node; skipping CA injection" >&2; return 0; }
  container="buildx_buildkit_${node}"
  docker ps --format '{{.Names}}' | grep -qx "$container" || {
    echo "[build-and-push] active builder has no buildkit container ($container); assuming it uses the daemon's CA store." >&2
    return 0
  }
  docker exec "$container" test -f "$marker" 2>/dev/null && return 0
  echo "[build-and-push] Trusting the sandbox proxy CA inside $container..."
  docker exec -i "$container" sh -c "cat >> /etc/ssl/certs/ca-certificates.crt && touch $marker" < /root/.ccr/ca-bundle.crt
  # Go caches the system roots on first use, so buildkitd has to be restarted
  # to pick the appended CA up.
  docker restart "$container" >/dev/null
  docker buildx inspect --bootstrap >/dev/null
}

if [[ -n "${CCR_AGENT_PROXY_ENABLED:-}" && -f /root/.ccr/ca-bundle.crt ]]; then
  ensure_sandbox_builder
  ensure_builder_trusts_proxy_ca
  # This sandbox's egress proxy MITMs TLS with its own CA, and its plain-HTTP
  # listener 405s any non-TLS request outright -- so BOTH the apt sources'
  # http:// URLs need rewriting to https:// AND the resulting TLS connections
  # need the proxy's CA trusted, or apt-get fails with "Method Not Allowed"
  # and a confusing "not signed" error that looks unrelated to either cause.
  # Deliberately NOT baked into the committed Dockerfile -- irrelevant (and
  # an unexplained CA pin) anywhere but this sandbox.
  BUILD_ARGS+=(--build-arg "HTTPS_PROXY=${HTTPS_PROXY:-}" --build-arg "HTTP_PROXY=${HTTPS_PROXY:-}")
  echo "[build-and-push] Sandbox proxy detected — injecting its CA for this build only (not committed to the Dockerfile)."
  cp /root/.ccr/ca-bundle.crt "$SCRIPT_DIR/.sandbox-proxy-ca.crt"
  CLEANUP_CA="$SCRIPT_DIR/.sandbox-proxy-ca.crt"
  trap '[[ -n "$CLEANUP_CA" ]] && rm -f $CLEANUP_CA' EXIT
  PATCHED_DOCKERFILE="$(mktemp)"
  CLEANUP_CA="$CLEANUP_CA $PATCHED_DOCKERFILE"
  trap '[[ -n "$CLEANUP_CA" ]] && rm -f $CLEANUP_CA' EXIT
  # Matches `FROM node:` OR `FROM .../agent-base:`. This used to match only the
  # former, so the moment this Dockerfile moved onto the shared base the build
  # died with "no `FROM node:` line found" -- a script silently coupled to one
  # specific base image. The agent-base image performs its own CA-trust step via
  # a BuildKit secret, so this injection is belt-and-braces there rather than
  # load-bearing, but failing the build outright was the wrong response either
  # way.
  awk '
    !patched && (/^FROM node:/ || /^FROM .*agent-base:/) {
      print
      print "RUN for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources /etc/apt/sources.list.d/*.list; do [ -f \"$f\" ] && sed -i \"s#http://#https://#g\" \"$f\"; done; true"
      print "COPY infra/oracle-cloud/agent-shell/.sandbox-proxy-ca.crt /usr/local/share/ca-certificates/sandbox-proxy.crt"
      print "RUN apt-get update -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false && apt-get install -y --no-install-recommends -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false ca-certificates && update-ca-certificates && rm -rf /var/lib/apt/lists/*"
      print "ENV NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/sandbox-proxy.crt"
      patched=1
      next
    }
    { print }
    END { if (!patched) { print "[build-and-push] no `FROM node:` or `FROM .../agent-base:` line found -- cannot inject the sandbox CA" > "/dev/stderr"; exit 1 } }
  ' "$DOCKERFILE" > "$PATCHED_DOCKERFILE"
  DOCKERFILE_TO_BUILD="$PATCHED_DOCKERFILE"
  echo "[build-and-push] Building + pushing ${IMAGE}:${TAG} (sandbox CA-trust variant, cache: ${CACHE_REF})..."
  docker buildx build "${BUILD_ARGS[@]}" -f "$DOCKERFILE_TO_BUILD" -t "${IMAGE}:${TAG}" -t "${IMAGE}:latest" --push .
else
  echo "[build-and-push] Building + pushing ${IMAGE}:${TAG} (cache: ${CACHE_REF})..."
  docker buildx build "${BUILD_ARGS[@]}" -f "$DOCKERFILE" -t "${IMAGE}:${TAG}" -t "${IMAGE}:latest" --push .
fi

"$REPO_ROOT/scripts/docker-cache-cleanup.sh" || true

echo "[build-and-push] Done. Redeploy a VM with:"
echo "  curl -X POST -H \"Authorization: Bearer <token>\" \"https://<ip>.sslip.io/services/agent-shell/redeploy?tag=${TAG}\""
