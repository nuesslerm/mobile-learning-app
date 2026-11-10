#!/bin/bash
# redeploy-app <app> [tag] -- the scoped way this container redeploys one of
# the VM's OTHER backend apps, without ever touching a docker socket. Calls
# vm-ops-api's own POST /services/:app/redeploy over the tailnet (this
# container is tailscale-joined via its own sidecar, same as every other
# app), using the same VM_OPS_API_AUTH_TOKEN bearer token every other caller
# of that endpoint already uses. See infra/oracle-cloud/README.md section 10
# for the endpoint's own docs.
set -euo pipefail

if [ -z "${1:-}" ]; then
  echo "usage: redeploy-app <app> [tag]" >&2
  exit 1
fi
APP="$1"
TAG="${2:-latest}"

if [ -z "${VM_OPS_API_AUTH_TOKEN:-}" ]; then
  echo "redeploy-app: VM_OPS_API_AUTH_TOKEN is not set in this container's env" >&2
  exit 1
fi
if [ -z "${HOST_LABEL:-}" ]; then
  echo "redeploy-app: HOST_LABEL is not set in this container's env" >&2
  exit 1
fi

# The bare hostname (vm-ops-api-${HOST_LABEL}) can be stuck unresolvable
# for a real, root-caused reason (docs/known-gaps.md, "tailnet sidecars
# stuck on collision-suffixed names"): each sidecar's tailscale-state is a
# named Docker volume that survives a plain compose down/up, so a node that
# once collided with a stale peer keeps re-claiming its "-1" (etc.) suffix
# forever -- hostname negotiation only happens once, at first `tailscale up`
# on a genuinely new identity. Until that's fixed at the host level (needs
# `docker compose down -v`, i.e. SSH), fall back to trying a few suffixes
# rather than hard-failing on a stale assumption.
try_redeploy() {
  local host="$1"
  # vm-ops-api's own /services/:app/redeploy blocks synchronously until the
  # sibling container's pull+down+up finishes -- confirmed live to take
  # 30-90s even with a warm image cache, well past a "is this host even
  # reachable" timeout. --max-time must cover the slowest real redeploy, not
  # just the connection -- a short timeout here reads identically to a dead
  # host (curl exit 28, http_code 000) and made every suffix fall through as
  # if unreachable when the first reachable one was just still working.
  curl -sS --max-time 180 -w '\n%{http_code}' -X POST \
    -H "Authorization: Bearer ${VM_OPS_API_AUTH_TOKEN}" \
    "http://${host}:8080/services/${APP}/redeploy?tag=${TAG}"
}

for SUFFIX in "" "-1" "-2" "-3"; do
  HOST="vm-ops-api-${HOST_LABEL}${SUFFIX}"
  RESULT="$(try_redeploy "$HOST" 2>&1)" && {
    STATUS="${RESULT##*$'\n'}"
    if [ "$STATUS" != "000" ]; then
      echo "$RESULT"
      exit 0
    fi
  }
done

echo "redeploy-app: could not reach vm-ops-api under ${HOST_LABEL} or any -1.._3 suffix -- see docs/known-gaps.md's tailnet-hostname-suffix entry" >&2
exit 1
