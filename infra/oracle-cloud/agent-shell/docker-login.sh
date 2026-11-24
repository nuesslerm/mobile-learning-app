#!/bin/bash
# Waits for this container's own nested dockerd to come up, creates a
# cache-export-capable buildx builder, then logs into Docker Hub -- all
# one-time setup whose result persists for the container's lifetime, not
# something every build/push needs to repeat. An unset or rejected
# DOCKERHUB_TOKEN fails the container start: no build runs without a login.
set -uo pipefail

for _ in $(seq 1 60); do
  if docker info > /dev/null 2>&1; then
    break
  fi
  sleep 1
done
if ! docker info > /dev/null 2>&1; then
  echo "[docker-login] dockerd never became ready -- giving up" >&2
  exit 0
fi

# The default `docker` buildx driver runs buildkit inside the daemon
# itself, which cannot export a registry-backed build cache ("Cache
# export is not supported for the docker driver") -- confirmed live,
# the build scripts used in this container always pass --cache-to. A
# docker-container-driver builder runs buildkit in its own container and
# supports it. Idempotent: `buildx create` on an existing builder name is
# a no-op check, not a repeat-create.
if ! docker buildx inspect agent-shell-builder >/dev/null 2>&1; then
  echo "[docker-login] Creating buildx builder 'agent-shell-builder' (docker-container driver, for registry cache export)..."
  docker buildx create --name agent-shell-builder --driver docker-container --bootstrap >/dev/null 2>&1
fi
docker buildx use agent-shell-builder

# Keep the nested daemon's store proportional to the disk it shares with the VM. Its persistent volume once grew
# without bound, and the daemon then saturated the disk reading it back. Limits derive from the real disk size.
disk_bytes=$(df -B1 --output=size / | tail -1 | tr -d ' ')
docker image prune -af > /dev/null 2>&1 || true
docker buildx inspect --bootstrap > /dev/null 2>&1 || true
docker buildx prune -f --max-used-space "$((disk_bytes * 40 / 100))" --min-free-space "$((disk_bytes * 25 / 100))" > /dev/null 2>&1 || true

: "${DOCKERHUB_TOKEN:?DOCKERHUB_TOKEN is not set}"

if echo "$DOCKERHUB_TOKEN" | docker login -u "${DOCKERHUB_USERNAME:-nuesslerm}" --password-stdin > /tmp/docker-login.log 2>&1; then
  echo "[docker-login] logged in to Docker Hub as ${DOCKERHUB_USERNAME:-nuesslerm}"
else
  echo "[docker-login] docker login failed: $(cat /tmp/docker-login.log)" >&2
  exit 1
fi
