#!/usr/bin/env bash
# Builds + pushes the budget-guard function's container image to OCIR, then
# applies budget-guard.tf with the resulting image URI.
#
# Needs a running Docker daemon — this repo's own sandbox environment often
# doesn't have one (dockerd not started). Run this from anywhere that does:
# OCI Cloud Shell (free, browser-based, has docker/oci CLI preinstalled and
# already authenticated to your tenancy) is the easiest option if your own
# machine doesn't have Docker.
#
# Requires OCIR_AUTH_TOKEN to be set: an OCI Auth Token (Console -> Profile
# icon -> My profile -> Auth Tokens -> Generate Token) — NOT your account
# password, NOT the API signing key used elsewhere in this repo. This is the
# one piece of setup that has no CLI/Terraform equivalent; must be generated
# once by hand.
#
# Usage: OCIR_AUTH_TOKEN=... scripts/deploy-budget-guard-function.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."
# shellcheck source=lib/oci-cli.sh
source "$SCRIPT_DIR/lib/oci-cli.sh"

log() { printf '[deploy-budget-guard] %s\n' "$1"; }

: "${OCIR_AUTH_TOKEN:?Set OCIR_AUTH_TOKEN to an OCI Auth Token (Console -> Profile -> Auth Tokens -> Generate Token)}"

command -v docker >/dev/null 2>&1 || { log "docker not found — run this from a machine/shell with Docker (e.g. OCI Cloud Shell)"; exit 1; }
docker info >/dev/null 2>&1 || { log "docker daemon not reachable — is it running? (this sandbox usually doesn't have one)"; exit 1; }

ensure_oci_cli
ensure_oci_config

tfvar() {
  grep -E "^\s*$1\s*=" terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/'
}

REGION="$(tfvar region)"
USER_OCID="$(tfvar user_ocid)"

REGION_KEY="$(oci iam region list --output json | python3 -c "
import json,sys
data=json.load(sys.stdin)
m={r['name']: r['key'] for r in data}
print(m.get('$REGION','').lower())
")"
[[ -n "$REGION_KEY" ]] || { log "could not resolve OCIR region key for $REGION"; exit 1; }

NAMESPACE="$(oci os ns get --query data --raw-output)"
[[ -n "$NAMESPACE" ]] || { log "could not resolve the tenancy's Object Storage namespace"; exit 1; }

# OCI CLI's `oci iam user get` returns the plain username; for a federated
# user this is usually already in `oracleidentitycloudservice/<name>` form,
# which is exactly what OCIR expects as the docker login username.
OCIR_USERNAME_SUFFIX="$(oci iam user get --user-id "$USER_OCID" --query 'data.name' --raw-output)"
OCIR_HOST="${REGION_KEY}.ocir.io"
OCIR_USERNAME="${NAMESPACE}/${OCIR_USERNAME_SUFFIX}"
IMAGE="${OCIR_HOST}/${NAMESPACE}/budget-guard-functions/budget-guard:0.0.1"

log "logging into $OCIR_HOST as $OCIR_USERNAME"
echo "$OCIR_AUTH_TOKEN" | docker login "$OCIR_HOST" -u "$OCIR_USERNAME" --password-stdin

log "building $IMAGE"
docker build -t "$IMAGE" functions/budget-guard/

log "pushing $IMAGE"
docker push "$IMAGE"

log "applying budget-guard.tf with function_image=$IMAGE"
terraform init -input=false >/dev/null
terraform apply -auto-approve -input=false -var "function_image=$IMAGE"

log "done — the automated kill switch is now live. Confirm the budget_alert_email and budget-guard ONS topic confirmation emails if you haven't already (each needs its link clicked before it delivers anything)."
