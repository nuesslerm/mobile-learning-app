#!/usr/bin/env bash
set -euo pipefail

# leetcode-auth-cookie-capture is a resource-hungry service (headed Chromium via
# Xvfb+VNC, shm_size 2gb). Host-agnostic: reads whatever VM_HOST it's given.
# That used to be the dedicated `heavy` host via VM_HOST_HEAVY; there is now one
# VM, so the deploy workflow passes VM_HOST.
# Uses docker-compose.capture.yml (not .prod.yml).
VM_DIR="/home/ubuntu/leetcode-auth-cookie-capture"
COMPOSE_FILE="docker-compose.capture.yml"
LOCAL_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="$(cd "$LOCAL_DIR/../.." && pwd)"
WF_BIN="${WF_BIN:-wf}"
SKIP_SECRETS_SYNC="${SKIP_SECRETS_SYNC:-false}"
# Which `wf secret` backend resolves prod secrets on the VM. Defaults to
# bitwarden (Bitwarden Secrets Manager, via the `bws` CLI + BWS_ACCESS_TOKEN /
# BWS_PROJECT_ID in the VM env). bitwarden is currently the only production backend.
SECRETS_BACKEND="${SECRETS_BACKEND:-bitwarden}"

if [[ -z "${VM_HOST:-}" ]]; then
  cat >&2 <<'MSG'
Missing required VM_HOST env var (e.g. VM_HOST=ubuntu@203.0.113.20). No
placeholder default — pass the real host explicitly, either as an env var
locally or (in CI) as the VM_HOST GitHub Actions secret.
MSG
  exit 1
fi

echo "[deploy] Syncing to ${VM_HOST}..."

rsync -avz \
  --exclude=node_modules \
  --exclude=dist \
  --exclude=.git \
  --exclude=.env \
  "$LOCAL_DIR/" "$VM_HOST:$VM_DIR/"

if [[ "$SKIP_SECRETS_SYNC" != "true" ]]; then
  if [[ ! -x "$WF_BIN" ]]; then
    echo "[deploy] $WF_BIN is not executable — it builds the Rust wf with cargo on first use, so cargo must be installed, or pass SKIP_SECRETS_SYNC=true to keep the VM's hand-maintained .env." >&2
    exit 1
  fi
fi

echo "[deploy] Building and restarting containers..."
ssh "$VM_HOST" bash -s "$SKIP_SECRETS_SYNC" "$COMPOSE_FILE" "$SECRETS_BACKEND" "${IMAGE_TAG:-}" << 'REMOTE'
  set -euo pipefail
  SKIP_SECRETS_SYNC="$1"
  COMPOSE_FILE="$2"
  SECRETS_BACKEND="$3"
  # Pins the image build to pull; CI passes the sha- tag it just pushed. Empty
  # means "resolve IMAGE_TAG from .env" (default latest). Exported so it outranks
  # the .env file, as a deliberate CI pin should.
  if [[ -n "${4:-}" ]]; then export IMAGE_TAG="$4"; fi
  cd ~/leetcode-auth-cookie-capture

  if [[ "$SKIP_SECRETS_SYNC" != "true" ]]; then
    if ! command -v wf >/dev/null 2>&1; then
      echo "[deploy-remote] wf is required on this host to resolve secrets from the vault (see docs/solutions/security/keychain-secrets-not-env.md); cloud-init installs it at /usr/local/bin/wf. Install it, or re-run with SKIP_SECRETS_SYNC=true to keep the existing .env." >&2
      exit 1
    fi
    # The secrets backend needs its bootstrap creds in this shell's env to reach
    # the vault (Bitwarden: BWS_ACCESS_TOKEN + BWS_PROJECT_ID). Source the
    # VM-resident .env.local first so they're present — it's also the non-secret
    # override file appended below.
    if [[ -f .env.local ]]; then set -a; . ./.env.local; set +a; fi
    # Build .env: committed non-secret .env.defaults (base) + secrets from the
    # vault via `sync-env --base`, then the VM-resident .env.local last for
    # environment-specific overrides (the real tailnet URLs .env.defaults shows as
    # <tailnet> placeholders). Secrets never get hand-typed into a file.
    echo "[deploy-remote] Building .env: .env.defaults base + secrets from ${SECRETS_BACKEND} (+ .env.local override)..."
    # --out writes the real .env directly from the wf process -- never via
    # this shell's stdout -- which is what lets sync-env redact-by-default
    # everywhere else without breaking this deploy path. See
    # docs/solutions/security/sync-env-stdout-redaction.md.
    wf secret sync-env --for leetcode-auth-cookie-capture --backend "$SECRETS_BACKEND" --base .env.defaults --out .env
    if [[ -f .env.local ]]; then cat .env.local >> .env; fi
  else
    echo "[deploy-remote] SKIP_SECRETS_SYNC=true — keeping the existing .env as-is."
  fi

  # The image is a PUBLIC prebuilt pushed by CI to Docker Hub, so pull it rather
  # than having CI stream a tarball over SSH — public means no registry credential
  # on this host. `pull` hard-fails (set -e) on a missing tag, which beats
  # silently starting a stale local image.
  echo "[deploy-remote] Pulling images (IMAGE_TAG=${IMAGE_TAG:-from .env})..."
  docker compose -f "$COMPOSE_FILE" pull
  echo "[deploy-remote] Stopping existing containers..."
  docker compose -f "$COMPOSE_FILE" down
  echo "[deploy-remote] Starting containers..."
  docker compose -f "$COMPOSE_FILE" up -d
  echo "[deploy-remote] Cleaning up old images..."
  docker image prune -f
REMOTE

echo "[deploy] Deploy complete."
echo "[deploy] Check status with: ssh ${VM_HOST} 'docker compose -f ~/leetcode-auth-cookie-capture/${COMPOSE_FILE} ps'"
