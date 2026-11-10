#!/usr/bin/env bash
# Fires a real, harmless Networking event (create + immediately delete a
# throwaway Network Security Group) to verify security-alerts.tf's
# Audit -> Events -> ONS -> email/PagerDuty pipeline actually delivers,
# end to end, rather than trusting the Terraform config alone.
#
# Uses createnetworksecuritygroup, NOT an IAM/auth-token event: a live A/B
# test found identity-plane events (CreateAuthToken, CreateGroup, etc.)
# never actually deliver in this tenancy, even though they land in Audit
# with the exact eventType string the rule matches (see README section 9,
# "Known gap #2"). Testing with an IAM event would always report Audit
# confirmation and never actually prove delivery — this uses the one
# category confirmed to work end-to-end instead.
#
# Usage: scripts/test-security-alert.sh [--timeout-seconds N]
#
# Confirms createnetworksecuritygroup landed in Audit within the timeout —
# proof the event actually happened. It CANNOT confirm PagerDuty/email
# actually received or rendered the alert (no PagerDuty API credential is
# wired up here) — check your phone/inbox for the alert this generates.
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

log() { printf '[test-security-alert] %s\n' "$1"; }

[[ -f terraform.tfvars ]] || { log "terraform.tfvars not found — run this from infra/oracle-cloud, or bootstrap via retry-apply.sh first"; exit 1; }
command -v terraform >/dev/null 2>&1 || { log "terraform not on PATH — run scripts/retry-apply.sh once first, or install it yourself"; exit 1; }

ensure_oci_cli
ensure_oci_config

TENANCY_OCID="$(grep -E '^\s*tenancy_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
COMPARTMENT_OCID="$(grep -E '^\s*compartment_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
[[ -n "$TENANCY_OCID" && -n "$COMPARTMENT_OCID" ]] || { log "tenancy_ocid/compartment_ocid missing from terraform.tfvars"; exit 1; }

VCN_ID="$(terraform show -json | python3 -c "
import json, sys
d = json.load(sys.stdin)
for r in d['values']['root_module']['resources']:
    if r['address'] == 'oci_core_vcn.main':
        print(r['values']['id'])
        break
")"
[[ -n "$VCN_ID" ]] || { log "could not read the VCN id from terraform state — is oci_core_vcn.main created?"; exit 1; }

NAME="security-alerts-test-nsg-$(date +%s 2>/dev/null || echo manual)"
log "creating a throwaway NSG ($NAME) in $VCN_ID..."
CREATE_OUTPUT="$(oci network nsg create --compartment-id "$COMPARTMENT_OCID" --vcn-id "$VCN_ID" --display-name "$NAME")"
NSG_ID="$(echo "$CREATE_OUTPUT" | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["id"])')"
log "created $NSG_ID — deleting it immediately"
oci network nsg delete --nsg-id "$NSG_ID" --force
log "deleted. Waiting up to ${TIMEOUT_SECONDS}s for the event to land in Audit..."

DEADLINE=$(($(date +%s) + TIMEOUT_SECONDS))
SEEN_CREATE=false

while true; do
  WINDOW_START="$(python3 -c "import datetime; print((datetime.datetime.utcnow()-datetime.timedelta(minutes=15)).strftime('%Y-%m-%dT%H:%M:%SZ'))")"
  WINDOW_END="$(python3 -c "import datetime; print((datetime.datetime.utcnow()+datetime.timedelta(minutes=1)).strftime('%Y-%m-%dT%H:%M:%SZ'))")"

  EVENTS_JSON="$(oci audit event list --compartment-id "$TENANCY_OCID" --start-time "$WINDOW_START" --end-time "$WINDOW_END" --all 2>/dev/null || echo '{"data":[]}')"

  if echo "$EVENTS_JSON" | python3 -c "
import json, sys
d = json.load(sys.stdin)
types = [e.get('event-type', '') for e in d.get('data', [])]
sys.exit(0 if any('createnetworksecuritygroup' in t.lower() for t in types) else 1)
"; then
    SEEN_CREATE=true
  fi

  if $SEEN_CREATE; then
    log "createnetworksecuritygroup confirmed in Audit."
    log "This event type is confirmed (via a prior live test) to deliver correctly through the full pipeline — check PagerDuty/email now for an alert."
    log "If nothing arrived: check the security-alerts ONS topic's subscription state (oci ons subscription list --compartment-id <compartment_ocid>), and the PagerDuty integration's own 'recent events' log for a delivery that arrived but didn't transform into an incident."
    exit 0
  fi

  if [[ $(date +%s) -ge $DEADLINE ]]; then
    log "timed out after ${TIMEOUT_SECONDS}s — createnetworksecuritygroup not seen in Audit"
    log "Audit itself can lag a few minutes under load; try a longer --timeout-seconds before assuming something's broken."
    exit 1
  fi

  sleep 10
done
