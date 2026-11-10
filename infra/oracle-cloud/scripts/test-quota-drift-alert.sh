#!/usr/bin/env bash
# Simulates exactly the attack this check-quota-drift.sh guards against: a
# credential calling `oci limits quota update` directly to loosen the
# Always-Free cap, bypassing Terraform entirely (Limits/Quotas doesn't
# produce Events, so this can't be caught in real time — this is why
# check-quota-drift.sh exists as a periodic substitute).
#
# Temporarily bumps the A1.Flex OCPU cap by 1, runs check-quota-drift.sh to
# confirm it detects the change and alerts, then reverts via `terraform
# apply` regardless of outcome (trap-guarded) so no elevated quota is ever
# left in place, even if this script is interrupted partway through.
#
# Usage: scripts/test-quota-drift-alert.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."
# shellcheck source=lib/oci-cli.sh
source "$SCRIPT_DIR/lib/oci-cli.sh"

log() { printf '[test-quota-drift-alert] %s\n' "$1"; }

[[ -f terraform.tfvars ]] || { log "terraform.tfvars not found — run this from infra/oracle-cloud"; exit 1; }
command -v terraform >/dev/null 2>&1 || { log "terraform not on PATH — run scripts/retry-apply.sh once first, or install it yourself"; exit 1; }

ensure_oci_cli
ensure_oci_config
terraform init -input=false >/dev/null

QUOTA_ID="$(terraform show -json | python3 -c "
import json, sys
d = json.load(sys.stdin)
for r in d['values']['root_module']['resources']:
    if r['address'] == 'oci_limits_quota.always_free_only':
        print(r['values']['id'])
        break
")"
[[ -n "$QUOTA_ID" ]] || { log "could not read the quota id from terraform state — is always-free-quota.tf applied?"; exit 1; }

revert() {
  log "reverting quota to the declared state via terraform apply (always runs, even on failure/interrupt)..."
  terraform apply -input=false -auto-approve -no-color -target=oci_limits_quota.always_free_only >/dev/null 2>&1 || {
    log "WARNING: revert failed — check the quota manually: oci limits quota get --quota-id $QUOTA_ID"
    return 1
  }
  log "reverted."
}
trap revert EXIT

log "reading current live statements..."
CURRENT_STATEMENTS="$(oci limits quota get --quota-id "$QUOTA_ID" --query 'data.statements' --raw-output 2>&1)"

log "drifting the quota: bumping standard-a1-core-regional-count by 1 directly via the OCI API (bypassing Terraform)..."
NEW_STATEMENTS="$(echo "$CURRENT_STATEMENTS" | python3 -c "
import json, sys, re
stmts = json.load(sys.stdin)
out = []
for s in stmts:
    if 'standard-a1-core-regional-count' in s:
        m = re.search(r'to (\d+)', s)
        n = int(m.group(1)) + 1
        s = re.sub(r'to \d+', f'to {n}', s)
    out.append(s)
print(json.dumps(out))
")"
oci limits quota update --quota-id "$QUOTA_ID" --statements "$NEW_STATEMENTS" --force >/dev/null
log "drifted. Running check-quota-drift.sh — it should detect this and alert..."

set +e
./scripts/check-quota-drift.sh
DRIFT_EXIT=$?
set -e

if [[ $DRIFT_EXIT -eq 1 ]]; then
  log "PASS — check-quota-drift.sh correctly detected the drift and published an alert. Check PagerDuty/email now."
else
  log "FAIL — check-quota-drift.sh reported no drift (exit 0) despite the quota being deliberately changed. The detection script has a bug."
fi

exit $((DRIFT_EXIT == 1 ? 0 : 1))
