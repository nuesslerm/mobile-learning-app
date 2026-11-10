#!/usr/bin/env bash
set -euo pipefail

VM_DIR="/home/ubuntu/leetcode-backend"
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
Missing required VM_HOST env var (e.g. VM_HOST=ubuntu@203.0.113.10). This
script no longer has a placeholder default — pass the real host explicitly,
either as an env var locally or as a GitHub Actions secret in CI.
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
ssh "$VM_HOST" bash -s "$SKIP_SECRETS_SYNC" "$SECRETS_BACKEND" "${IMAGE_TAG:-}" << 'REMOTE'
  set -euo pipefail
  SKIP_SECRETS_SYNC="$1"
  SECRETS_BACKEND="$2"
  # Pins the image build to pull; CI passes the sha- tag it just pushed. Empty
  # means "resolve IMAGE_TAG from .env" (default latest). Exported so it outranks
  # the .env file, as a deliberate CI pin should.
  if [[ -n "${3:-}" ]]; then export IMAGE_TAG="$3"; fi
  cd ~/leetcode-backend

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
    # Build .env: committed non-secret .env.defaults (base, synced with the repo)
    # + secrets from the vault via `sync-env --base`, then the VM-resident
    # .env.local last for environment-specific overrides (e.g. the real tailnet
    # URL that .env.defaults shows as a <tailnet> placeholder).
    echo "[deploy-remote] Building .env: .env.defaults base + secrets from ${SECRETS_BACKEND} (+ .env.local override)..."
    # --out writes the real .env directly from the wf process -- never via
    # this shell's stdout -- which is what lets sync-env redact-by-default
    # everywhere else without breaking this deploy path. See
    # docs/solutions/security/sync-env-stdout-redaction.md.
    wf secret sync-env --for leetcode-backend --backend "$SECRETS_BACKEND" --base .env.defaults --out .env
    if [[ -f .env.local ]]; then cat .env.local >> .env; fi
  else
    echo "[deploy-remote] SKIP_SECRETS_SYNC=true — keeping the existing .env as-is."
  fi

  # The image is a PUBLIC prebuilt pushed by CI to Docker Hub, so pull it rather
  # than having CI stream a tarball over SSH. Public means no registry credential
  # is needed on this host. There is still no build step here (the VM only has
  # this app dir, not the pnpm workspace a build needs). `pull` hard-fails
  # (set -e) on a missing tag, which beats silently starting a stale local image.
  echo "[deploy-remote] Pulling images (IMAGE_TAG=${IMAGE_TAG:-from .env})..."
  docker compose -f docker-compose.prod.yml pull
  echo "[deploy-remote] Stopping existing containers..."
  docker compose -f docker-compose.prod.yml down
  echo "[deploy-remote] Starting containers..."
  docker compose -f docker-compose.prod.yml up -d
  echo "[deploy-remote] Cleaning up old images..."
  docker image prune -f
REMOTE

echo "[deploy] Deploy complete."
echo "[deploy] Check status with: ssh ${VM_HOST} 'docker compose -f ~/leetcode-backend/docker-compose.prod.yml ps'"
