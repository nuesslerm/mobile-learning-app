#!/usr/bin/env bash
# Detects if always-free-quota.tf's compartment quota has drifted from what
# Terraform declares -- e.g. someone (or a compromised credential) widened
# the cap via `oci limits quota update` directly, outside of Terraform,
# defeating the preventive block without ever touching this repo.
#
# Limits/Quotas isn't an Events-producing service (verified -- see
# security-alerts.tf's "Known gap #1"), so there's no way to catch this via
# the Events pipeline the way network/compute changes are caught. This
# script is the substitute: periodic drift detection instead of real-time
# events, alerting directly to the security-alerts ONS topic (the same
# direct-publish mechanism proven reliable in manual testing) since the
# Events rule itself can't see this resource type at all.
#
# Meant to run on a schedule (a cron / scheduled Routine — see README
# section 8). Exits 0 (no drift) or 1 (drift found, alert sent either way)
# — safe to run repeatedly.
#
# Usage: scripts/check-quota-drift.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."
# shellcheck source=lib/oci-cli.sh
source "$SCRIPT_DIR/lib/oci-cli.sh"

log() { printf '[check-quota-drift] %s\n' "$1"; }

[[ -f terraform.tfvars ]] || { log "terraform.tfvars not found — run this from infra/oracle-cloud"; exit 1; }
command -v terraform >/dev/null 2>&1 || { log "terraform not on PATH — run scripts/retry-apply.sh once first, or install it yourself"; exit 1; }

ensure_oci_cli
ensure_oci_config

terraform init -input=false >/dev/null

set +e
PLAN_OUTPUT="$(terraform plan -input=false -no-color -detailed-exitcode -target=oci_limits_quota.always_free_only 2>&1)"
PLAN_EXIT=$?
set -e

if [[ $PLAN_EXIT -eq 0 ]]; then
  log "no drift — the live quota matches always-free-quota.tf exactly."
  exit 0
fi

if [[ $PLAN_EXIT -ne 2 ]]; then
  log "terraform plan itself failed (exit $PLAN_EXIT) — treating as inconclusive, not confirmed drift:"
  echo "$PLAN_OUTPUT" | tail -20
  exit 1
fi

log "DRIFT DETECTED — the live compartment quota no longer matches always-free-quota.tf:"
echo "$PLAN_OUTPUT" | grep -A 20 'oci_limits_quota.always_free_only' | head -30

COMPARTMENT_OCID="$(grep -E '^\s*compartment_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
TENANCY_OCID="$(grep -E '^\s*tenancy_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"

QUOTA_ID="$(terraform show -json | python3 -c "
import json, sys
d = json.load(sys.stdin)
for r in d['values']['root_module']['resources']:
    if r['address'] == 'oci_limits_quota.always_free_only':
        print(r['values']['id'])
        break
" 2>/dev/null || true)"

TOPIC_ID="$(terraform show -json | python3 -c "
import json, sys
d = json.load(sys.stdin)
for r in d['values']['root_module']['resources']:
    if r['address'] == 'oci_ons_notification_topic.security_alerts[0]':
        print(r['values']['id'])
        break
" 2>/dev/null || true)"

if [[ -z "$TOPIC_ID" ]]; then
  log "security-alerts.tf isn't enabled (no ONS topic) — can't alert, but drift was still detected above. Fix the quota (terraform apply) and/or enable security-alerts.tf."
  exit 1
fi

# Limits/Quotas doesn't produce Events, but UpdateQuota/DeleteQuota calls are
# still logged in the general Audit trail (confirmed live) with the real
# caller's identity — look that up so the alert names who actually made the
# change, instead of just saying "this detection script found drift."
log "looking up who actually made this change via Audit..."
PRINCIPAL="unknown (no matching UpdateQuota/DeleteQuota Audit event found yet — Audit can lag; re-check manually if this matters)"
if [[ -n "$QUOTA_ID" && -n "$TENANCY_OCID" ]]; then
  AUDIT_START="$(python3 -c "import datetime; print((datetime.datetime.utcnow()-datetime.timedelta(hours=6)).strftime('%Y-%m-%dT%H:%M:%SZ'))")"
  AUDIT_END="$(python3 -c "import datetime; print((datetime.datetime.utcnow()+datetime.timedelta(minutes=1)).strftime('%Y-%m-%dT%H:%M:%SZ'))")"
  PRINCIPAL_LOOKUP="$(oci audit event list --compartment-id "$TENANCY_OCID" --start-time "$AUDIT_START" --end-time "$AUDIT_END" --all 2>/dev/null | QUOTA_ID="$QUOTA_ID" python3 -c "
import json, os, sys
d = json.load(sys.stdin)
quota_id = os.environ['QUOTA_ID']
matches = [
    e for e in d.get('data', [])
    if e.get('event-type') in ('com.oraclecloud.limits.UpdateQuota', 'com.oraclecloud.limits.DeleteQuota')
    and quota_id in (e.get('data', {}).get('request', {}).get('path') or '')
]
if matches:
    latest = sorted(matches, key=lambda e: e.get('event-time', ''))[-1]
    ident = latest['data']['identity']
    print(f\"{ident.get('principal-name')} (ip {ident.get('ip-address')}, via {ident.get('user-agent', '').split(' ')[0]}) at {latest.get('event-time')}\")
" 2>/dev/null || true)"
  [[ -n "$PRINCIPAL_LOOKUP" ]] && PRINCIPAL="$PRINCIPAL_LOOKUP"
fi
log "attributed to: $PRINCIPAL"

EVENT_TIME="$(python3 -c "import datetime; print(datetime.datetime.utcnow().strftime('%Y-%m-%dT%H:%M:%SZ'))")"
EVENT_ID="quota-drift-$(date +%s 2>/dev/null || echo manual)"
ALERT_BODY=$(PRINCIPAL="$PRINCIPAL" EVENT_TIME="$EVENT_TIME" EVENT_ID="$EVENT_ID" python3 -c "
import json, os
print(json.dumps({
    'eventType': 'oracle-cloud.quota-drift.detected',
    'eventTime': os.environ['EVENT_TIME'],
    'data': {
        'identity': {'principalName': os.environ['PRINCIPAL']},
        'compartmentName': 'always-free-quota.tf drift (see terraform plan)',
        'resourceName': 'oci_limits_quota.always_free_only',
    },
    'eventID': os.environ['EVENT_ID'],
}))
")
oci ons message publish --topic-id "$TOPIC_ID" --title "quota-drift-detected" --body "$ALERT_BODY" >/dev/null
log "alert published directly to the security-alerts topic (bypasses the Events pipeline entirely, since Limits/Quotas doesn't produce Events) — check PagerDuty/email."
log "To fix: run 'terraform apply' from infra/oracle-cloud to restore the declared quota, or update always-free-quota.tf if the change was intentional."
exit 1
