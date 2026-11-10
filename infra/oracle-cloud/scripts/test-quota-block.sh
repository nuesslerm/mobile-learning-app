#!/usr/bin/env bash
# Verifies always-free-quota.tf actually blocks non-free provisioning, live
# against the real tenancy — not just "the Terraform config looks right."
#
# How: the Always-Free A1.Flex pool (2 OCPU / 12 GB regional cap) is
# already fully reserved by this repo's single VM, which holds the whole
# allowance — a STOPPED instance still holds its allocated OCPU/memory quota
# in OCI, it isn't released until terminated — so ANY additional A1.Flex OCPU
# request should be rejected by the compartment quota before anything is
# created. Reuses that VM's own AD/subnet/image (guaranteed-valid parameters)
# so a rejection can only be the quota, not some unrelated bad-parameter
# error.
#
# Also checks whether the launchinstance Events actually fire on a
# rejected attempt — security-alerts.tf assumed they do; this is the live
# check of that assumption, not a repeat of the same guess.
#
# Safe to re-run: if the launch unexpectedly succeeds (quota didn't
# block), the script immediately terminates what it created.
set -euo pipefail

TIMEOUT_SECONDS=180

while [[ $# -gt 0 ]]; do
  case "$1" in
    --timeout-seconds) TIMEOUT_SECONDS="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."
# shellcheck source=lib/oci-cli.sh
source "$SCRIPT_DIR/lib/oci-cli.sh"

log() { printf '[test-quota-block] %s\n' "$1"; }

[[ -f terraform.tfvars ]] || { log "terraform.tfvars not found — run this from infra/oracle-cloud"; exit 1; }
command -v terraform >/dev/null 2>&1 || { log "terraform not on PATH — run scripts/retry-apply.sh once first (it bootstraps terraform), or install it yourself"; exit 1; }

ensure_oci_cli
ensure_oci_config

TENANCY_OCID="$(grep -E '^\s*tenancy_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
COMPARTMENT_OCID="$(grep -E '^\s*compartment_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
SSH_KEY_PATH="$(grep -E '^\s*ssh_public_key_path\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
SSH_KEY_PATH="${SSH_KEY_PATH/#\~/$HOME}"

STATE_JSON="$(terraform show -json)"
read -r AD SUBNET_ID IMAGE_ID <<< "$(echo "$STATE_JSON" | python3 -c "
import json, sys
d = json.load(sys.stdin)
for r in d['values']['root_module']['resources']:
    if r['address'] == 'oci_core_instance.main':
        v = r['values']
        ad = v['availability_domain']
        subnet = v['create_vnic_details'][0]['subnet_id']
        image = v['source_details'][0]['source_id']
        print(ad, subnet, image)
        break
")"
[[ -n "$AD" && -n "$SUBNET_ID" && -n "$IMAGE_ID" ]] || { log "could not read AD/subnet/image from terraform state — is oci_core_instance.main created?"; exit 1; }
[[ -f "$SSH_KEY_PATH" ]] || { log "ssh key not found at $SSH_KEY_PATH"; exit 1; }

DISPLAY_NAME="quota-block-test-$(date +%s 2>/dev/null || echo manual)"
log "attempting a 2nd A1.Flex instance (1 OCPU/6GB) — the free pool's 2 OCPU cap is already fully held by the single VM, so this should be rejected by always-free-quota.tf..."

SSH_KEY_CONTENT="$(cat "$SSH_KEY_PATH")"
set +e
LAUNCH_OUTPUT="$(oci compute instance launch \
  --availability-domain "$AD" \
  --compartment-id "$COMPARTMENT_OCID" \
  --shape "VM.Standard.A1.Flex" \
  --shape-config '{"ocpus":1,"memoryInGBs":6}' \
  --subnet-id "$SUBNET_ID" \
  --image-id "$IMAGE_ID" \
  --display-name "$DISPLAY_NAME" \
  --metadata "{\"ssh_authorized_keys\":\"$SSH_KEY_CONTENT\"}" 2>&1)"
LAUNCH_EXIT=$?
set -e

if [[ $LAUNCH_EXIT -eq 0 ]]; then
  INSTANCE_ID="$(echo "$LAUNCH_OUTPUT" | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["id"])' 2>/dev/null || echo "")"
  log "FAIL — the launch SUCCEEDED (quota did not block it). Terminating what was created to avoid a stray billable resource."
  [[ -n "$INSTANCE_ID" ]] && oci compute instance terminate --instance-id "$INSTANCE_ID" --force
  log "Check always-free-quota.tf — either the quota isn't applied, or its cap doesn't match reality anymore (e.g. the VM was resized down, freeing headroom)."
  exit 1
fi

if echo "$LAUNCH_OUTPUT" | grep -qiE 'limitexceeded|limit.*exceed|quota'; then
  log "PASS — launch rejected, and the error is quota-shaped:"
  echo "$LAUNCH_OUTPUT" | grep -iE 'limitexceeded|limit.*exceed|quota' | head -3
else
  log "launch was rejected, but NOT with a quota-shaped error — inconclusive, this may be a different problem entirely:"
  echo "$LAUNCH_OUTPUT" | head -10
  exit 1
fi

log "checking whether the rejected attempt still shows up in Audit as a launchinstance event (security-alerts.tf's compute category depends on this)..."
DEADLINE=$(($(date +%s) + TIMEOUT_SECONDS))
SEEN_BEGIN=false
SEEN_END=false
while true; do
  WINDOW_START="$(python3 -c "import datetime; print((datetime.datetime.utcnow()-datetime.timedelta(minutes=15)).strftime('%Y-%m-%dT%H:%M:%SZ'))")"
  WINDOW_END="$(python3 -c "import datetime; print((datetime.datetime.utcnow()+datetime.timedelta(minutes=1)).strftime('%Y-%m-%dT%H:%M:%SZ'))")"
  EVENTS_JSON="$(oci audit event list --compartment-id "$TENANCY_OCID" --start-time "$WINDOW_START" --end-time "$WINDOW_END" --all 2>/dev/null || echo '{"data":[]}')"

  if echo "$EVENTS_JSON" | python3 -c "
import json, sys
d = json.load(sys.stdin)
types = [e.get('event-type', '') for e in d.get('data', [])]
sys.exit(0 if any('launchinstance.begin' in t.lower() for t in types) else 1)
"; then SEEN_BEGIN=true; fi
  if echo "$EVENTS_JSON" | python3 -c "
import json, sys
d = json.load(sys.stdin)
types = [e.get('event-type', '') for e in d.get('data', [])]
sys.exit(0 if any('launchinstance.end' in t.lower() for t in types) else 1)
"; then SEEN_END=true; fi

  if $SEEN_BEGIN || $SEEN_END || [[ $(date +%s) -ge $DEADLINE ]]; then
    break
  fi
  sleep 10
done

log "result: launchinstance.begin=$SEEN_BEGIN launchinstance.end=$SEEN_END on a quota-rejected attempt"
if ! $SEEN_BEGIN && ! $SEEN_END; then
  log "NEITHER fired — a request OCI rejects synchronously (before any work request/async provisioning starts) apparently does NOT emit these Events at all. security-alerts.tf's comment claiming '.begin fires even on a blocked attempt' is WRONG and should be corrected — a quota-rejected launch is currently invisible to security-alerts.tf, only always-free-quota.tf's own rejection (visible in the API response, not an alert) stops it."
fi
