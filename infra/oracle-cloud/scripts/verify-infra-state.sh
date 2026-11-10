#!/usr/bin/env bash
# Read-only sanity check: confirms the cost/security-safety layers are
# actually live in the real tenancy, not just present in Terraform's
# config. No live-fire, no resources created/destroyed — just state reads.
# Complements test-security-alert.sh and test-quota-block.sh, which prove
# the layers actually DO something rather than just existing.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."
# shellcheck source=lib/oci-cli.sh
source "$SCRIPT_DIR/lib/oci-cli.sh"

log() { printf '[verify-infra-state] %s\n' "$1"; }
FAILED=0
check() {
  local desc="$1" ok="$2"
  if [[ "$ok" == "true" ]]; then
    log "OK   - $desc"
  else
    log "FAIL - $desc"
    FAILED=1
  fi
}

[[ -f terraform.tfvars ]] || { log "terraform.tfvars not found — run this from infra/oracle-cloud"; exit 1; }
command -v terraform >/dev/null 2>&1 || { log "terraform not on PATH — run scripts/retry-apply.sh once first, or install it yourself"; exit 1; }
ensure_oci_cli
ensure_oci_config

COMPARTMENT_OCID="$(grep -E '^\s*compartment_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"

STATE_JSON="$(terraform show -json)"

# --- always-free-quota.tf: the compartment quota itself is ACTIVE ---
QUOTA_ID="$(echo "$STATE_JSON" | python3 -c "
import json, sys
d = json.load(sys.stdin)
for r in d['values']['root_module']['resources']:
    if r['address'] == 'oci_limits_quota.always_free_only':
        print(r['values']['id'])
        break
" 2>/dev/null || true)"
if [[ -n "$QUOTA_ID" ]]; then
  QUOTA_STATE="$(oci limits quota get --quota-id "$QUOTA_ID" --query 'data."lifecycle-state"' --raw-output 2>/dev/null || echo "")"
  check "always-free-quota.tf: compartment quota is ACTIVE" "$([[ "$QUOTA_STATE" == "ACTIVE" ]] && echo true || echo false)"
else
  check "always-free-quota.tf: compartment quota resource exists in state" "false"
fi

# --- security-alerts.tf: topic + at least one subscription ACTIVE ---
TOPIC_ID="$(echo "$STATE_JSON" | python3 -c "
import json, sys
d = json.load(sys.stdin)
for r in d['values']['root_module']['resources']:
    if r['address'] == 'oci_ons_notification_topic.security_alerts[0]':
        print(r['values']['id'])
        break
" 2>/dev/null || true)"
if [[ -n "$TOPIC_ID" ]]; then
  check "security-alerts.tf: ONS topic exists" "true"
  SUB_STATES="$(oci ons subscription list --compartment-id "$COMPARTMENT_OCID" --topic-id "$TOPIC_ID" --query 'data[].{p:protocol,s:"lifecycle-state"}' 2>/dev/null || echo "[]")"
  echo "$SUB_STATES" | python3 -c "
import json, sys
subs = json.load(sys.stdin)
for s in subs:
    print(f\"       {s.get('p')}: {s.get('s')}\")
"
  ACTIVE_COUNT="$(echo "$SUB_STATES" | python3 -c "
import json, sys
subs = json.load(sys.stdin)
print(len([s for s in subs if s.get('s') == 'ACTIVE']))
")"
  check "security-alerts.tf: at least one subscription is ACTIVE" "$([[ "$ACTIVE_COUNT" -gt 0 ]] && echo true || echo false)"
else
  log "SKIP - security-alerts.tf: not enabled (security_alert_email / pagerduty_integration_key both unset) — nothing to check"
fi

# --- budget.tf: budget exists and is nonzero ---
BUDGET_AMOUNT="$(echo "$STATE_JSON" | python3 -c "
import json, sys
d = json.load(sys.stdin)
for r in d['values']['root_module']['resources']:
    if r['address'] == 'oci_budget_budget.agent_cost_cap':
        print(r['values']['amount'])
        break
" 2>/dev/null || echo "")"
check "budget.tf: budget exists with a nonzero cap (\$${BUDGET_AMOUNT:-0})" "$([[ -n "$BUDGET_AMOUNT" && "$BUDGET_AMOUNT" != "0" ]] && echo true || echo false)"

# --- budget-guard.tf: report whether the kill-switch pipeline is live (not required) ---
GUARD_ENABLED="$(echo "$STATE_JSON" | python3 -c "
import json, sys
d = json.load(sys.stdin)
found = any(r['address'] == 'oci_functions_function.budget_guard[0]' for r in d['values']['root_module']['resources'])
print('true' if found else 'false')
" 2>/dev/null || echo false)"
if [[ "$GUARD_ENABLED" == "true" ]]; then
  log "INFO - budget-guard.tf: automated kill-switch Function is deployed"
else
  log "INFO - budget-guard.tf: not deployed yet (function_image unset) — not a failure, see README section 7"
fi

if [[ $FAILED -eq 0 ]]; then
  log "all checks passed"
  exit 0
else
  log "one or more checks FAILED — see above"
  exit 1
fi
