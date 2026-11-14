#!/usr/bin/env bash
set -euo pipefail

# Caps local Docker/BuildKit build-cache growth after a build. Discovered
# 2026-07-30: a buildx docker-container builder's cache is a *volume*, not a
# layer the daemon garbage-collects on its own -- `--cache-to type=registry`
# (infra/oracle-cloud/agent-shell/build-and-push.sh)
# writes into it on every build with no eviction, so it grew to 16GB+ across
# one session's worth of rebuilds. Called automatically at the end of both
# those scripts so no one has to remember to run this by hand.
#
# Deliberately scoped to build-cache + dangling images only -- never touches
# running containers, tagged images, or volumes outside the buildx builder's
# own cache (e.g. this repo's sandbox may also be running an unrelated local
# Kubernetes cluster; this script has no business pruning that).
#
# Usage: ./scripts/docker-cache-cleanup.sh [keep-storage]
#   keep-storage: max size to retain in the active buildx builder's cache,
#                 e.g. "5GB" (default). Passed straight to `docker buildx
#                 prune --keep-storage`.

KEEP_STORAGE="${1:-5GB}"

command -v docker >/dev/null 2>&1 || { echo "[docker-cache-cleanup] docker not found on PATH" >&2; exit 0; }
docker info >/dev/null 2>&1 || { echo "[docker-cache-cleanup] no Docker daemon reachable, skipping" >&2; exit 0; }

echo "[docker-cache-cleanup] Before:"
docker system df

echo "[docker-cache-cleanup] Capping buildx cache at ${KEEP_STORAGE}..."
docker buildx prune -f --keep-storage "$KEEP_STORAGE" >/dev/null

echo "[docker-cache-cleanup] Dropping dangling (untagged) images..."
docker image prune -f >/dev/null

echo "[docker-cache-cleanup] After:"
docker system df
