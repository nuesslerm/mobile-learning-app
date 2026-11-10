#!/usr/bin/env bash
set -euo pipefail

# One-time VM setup for vm-ops-api: syncs the compose/Caddy config, seeds
# the vault bootstrap credential, and pulls+starts the published image.
# Needs real SSH access, so run this from your own machine.
#
# For ONGOING code changes after this initial setup, you no longer need
# this script or SSH at all: CI's last deploy leg calls POST /redeploy?ref=<sha>
# on the running vm-ops-api, which builds its own image at that commit and
# recreates itself from it over plain HTTPS. Re-run this script only if the
# compose/Caddy config itself changes, or for a fresh VM.
VM_DIR="/home/ubuntu/vm-ops-api"
LOCAL_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$LOCAL_DIR/../../.." && pwd)"
WF_BIN="${WF_BIN:-wf}"
SECRETS_BACKEND="${SECRETS_BACKEND:-bitwarden}"

for var in VM_HOST HOST_LABEL; do
  if [[ -z "${!var:-}" ]]; then
    cat >&2 <<MSG
Missing required $var env var. Example:
  VM_HOST=ubuntu@203.0.113.15 HOST_LABEL=light ./deploy.sh
MSG
    exit 1
  fi
done

if [[ ! -x "$WF_BIN" ]]; then
  echo "[deploy] $WF_BIN is not executable — it builds the Rust wf with cargo on first use, so cargo must be installed." >&2
  exit 1
fi

# vm-ops-api didn't exist when cloud-init ran, so unlike the other four
# app dirs, ~/vm-ops-api/.env.local was never seeded with the read-only
# deploy bootstrap credential — without it, sync-env below has no way to
# reach the vault at all. Bootstrap it here from THIS machine's own
# already-configured credential, so nothing is ever hardcoded.
#
# Can't use --backend bitwarden for this: BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN
# is the credential bitwarden itself needs to authenticate, so fetching it
# via bitwarden would be circular (see bootstrap-config.ts). Mirrors that
# same env-first-then-keychain resolution instead — env var wins if set
# (how it's provisioned in most shells/sandboxes), macOS keychain only as
# a local-dev fallback (no-ops on non-darwin). Safe to re-run: same
# idempotent content cloud-init would have written for a freshly created
# VM (also added to cloud-init.sh.tftpl's per-app loop for future rebuilds).
LOCAL_READ_TOKEN="${BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN:-}"
LOCAL_SERVER_URL="${BWS_SERVER_URL:-}"
if [[ -z "$LOCAL_READ_TOKEN" ]]; then
  LOCAL_READ_TOKEN="$("$WF_BIN" secrets get BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN --backend macos 2>/dev/null || true)"
fi
if [[ -z "$LOCAL_SERVER_URL" ]]; then
  LOCAL_SERVER_URL="$("$WF_BIN" secrets get BWS_SERVER_URL --backend macos 2>/dev/null || true)"
fi
if [[ -z "$LOCAL_READ_TOKEN" ]]; then
  cat >&2 <<MSG
[deploy] Could not resolve BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN from either
your shell environment or the local macOS keychain. That's the credential
vm-ops-api needs on the VM to reach the vault at all — export it in this
shell, or run: wf secret set BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN
MSG
  exit 1
fi
echo "[deploy] Seeding ~/vm-ops-api/.env.local on ${VM_HOST} with the read-only deploy bootstrap credential..."
ssh "$VM_HOST" "mkdir -p $VM_DIR && umask 077 && cat > $VM_DIR/.env.local" <<EOF
BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN=${LOCAL_READ_TOKEN}
$( [[ -n "$LOCAL_SERVER_URL" ]] && echo "BWS_SERVER_URL=${LOCAL_SERVER_URL}" )
EOF

echo "[deploy] Syncing to ${VM_HOST}..."
rsync -avz --exclude=node_modules --exclude=.git --exclude=.env --exclude=.env.local "$LOCAL_DIR/" "$VM_HOST:$VM_DIR/"


echo "[deploy] Building .env and starting containers..."
ssh "$VM_HOST" bash -s "$SECRETS_BACKEND" "$HOST_LABEL" << 'REMOTE'
  set -euo pipefail
  SECRETS_BACKEND="$1"
  HOST_LABEL="$2"
  cd ~/vm-ops-api

  if ! command -v wf >/dev/null 2>&1; then
    echo "[deploy-remote] wf is required on this host to resolve secrets from the vault; cloud-init installs it at /usr/local/bin/wf." >&2
    exit 1
  fi
  if [[ -f .env.local ]]; then set -a; . ./.env.local; set +a; fi

  echo "[deploy-remote] Building .env: .env.defaults base + secrets from ${SECRETS_BACKEND} (+ HOST_LABEL)..."
  # --out writes the real .env directly from the wf process -- never via this
  # shell's stdout -- which is what lets sync-env redact-by-default everywhere
  # else without breaking this deploy path. See
  # docs/solutions/security/sync-env-stdout-redaction.md.
  wf secret sync-env --for vm-ops-api --backend "$SECRETS_BACKEND" --base .env.defaults --out .env
  echo "HOST_LABEL=$HOST_LABEL" >> .env
  if [[ -f .env.local ]]; then cat .env.local >> .env; fi

  echo "[deploy-remote] Pulling the published image (IMAGE_TAG=${IMAGE_TAG:-latest})..."
  docker compose pull
  echo "[deploy-remote] Starting containers..."
  docker compose up -d
  docker compose ps
REMOTE

echo "[deploy] Done. vm-ops-api joined the tailnet as vm-ops-api-${HOST_LABEL} -- reach it via vm-gateway's Funnel, e.g.:"
echo "  curl -H \"Authorization: Bearer <the VM_GATEWAY_AUTH_SEED value>\" https://<vm-gateway-funnel-host>/vm-ops-api/health"
