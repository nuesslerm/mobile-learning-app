#!/usr/bin/env bash
# Retries `terraform apply` for the Oracle Cloud VM instances until Oracle's
# per-AD "Out of host capacity" error (ARM A1.Flex Always-Free pool) clears,
# cycling availability_domain_index (and optionally region, and as a last
# resort the x86 E2.1.Micro shape) across attempts. Safe to run repeatedly
# (e.g. from an hourly cron) — it's a no-op if the instances already exist,
# and always exits 0/1/3 within its time budget rather than hanging.
#
# Usage: scripts/retry-apply.sh [--max-attempts N] [--timeout-seconds N]
#                                [--regions r1,r2,...] [--allow-e2-micro-fallback]
#                                [--auto-subscribe-regions]
#
# --regions defaults to just the region already configured in terraform.tfvars
# (no rotation) — your tenancy must be SUBSCRIBED to any extra region you pass
# here (Console: Governance & Administration -> Region Management), otherwise
# every attempt in that region fails fast and it's skipped, which is fine but
# wastes an attempt. --allow-e2-micro-fallback only kicks in once every region
# x AD combination is exhausted for the A1.Flex shape; E2.1.Micro is fixed at
# 1/8 OCPU + 1 GB RAM — likely too small to run this repo's actual services,
# so treat a fallback-created VM as "something is reachable", not "done."
#
# --auto-subscribe-regions: on a region-not-subscribed error, try subscribing
# the tenancy to that region via the OCI CLI (`oci iam region-subscription
# create`) before giving up on it — off by default since region subscription
# is itself a real, not-easily-reversible tenancy-level action. Free Tier
# tenancies are capped at 1 subscribed region regardless, so this is a no-op
# until you're on Pay-As-You-Go; safe to enable ahead of that.
set -euo pipefail

MAX_ATTEMPTS=8
TIMEOUT_SECONDS=480 # 8 min — safely under an hourly cron's next fire
AD_COUNT=3
REGIONS=""
ALLOW_E2_MICRO_FALLBACK=false
AUTO_SUBSCRIBE_REGIONS=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --max-attempts) MAX_ATTEMPTS="$2"; shift 2 ;;
    --timeout-seconds) TIMEOUT_SECONDS="$2"; shift 2 ;;
    --regions) REGIONS="$2"; shift 2 ;;
    --allow-e2-micro-fallback) ALLOW_E2_MICRO_FALLBACK=true; shift ;;
    --auto-subscribe-regions) AUTO_SUBSCRIBE_REGIONS=true; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."
# shellcheck source=lib/oci-cli.sh
source "$SCRIPT_DIR/lib/oci-cli.sh"

log() { printf '[retry-apply] %s\n' "$1"; }

ensure_terraform() {
  command -v terraform >/dev/null 2>&1 && return
  log "terraform not on PATH, installing via mise"
  mise install terraform
  local bin
  bin="$(find "$HOME/.local/share/mise/installs/terraform" -maxdepth 2 -name terraform -type f | sort -V | tail -1)"
  [[ -n "$bin" ]] || { log "could not locate terraform binary after mise install"; exit 1; }
  mkdir -p "$HOME/.local/bin"
  ln -sf "$bin" "$HOME/.local/bin/terraform"
  export PATH="$HOME/.local/bin:$PATH"
}

ensure_bws() {
  command -v bws >/dev/null 2>&1 && return
  log "bws not on PATH, installing via mise"
  mise install bitwarden-secrets-manager
  local bin
  bin="$(find "$HOME/.local/share/mise/installs/bitwarden-secrets-manager" -maxdepth 2 -name bws -type f | sort -V | tail -1)"
  [[ -n "$bin" ]] || { log "could not locate bws binary after mise install"; exit 1; }
  mkdir -p "$HOME/.local/bin"
  ln -sf "$bin" "$HOME/.local/bin/bws"
  export PATH="$HOME/.local/bin:$PATH"
}

ensure_ssh_key() {
  [[ -f "$HOME/.ssh/oracle_deploy.pub" ]] && return
  command -v ssh-keygen >/dev/null 2>&1 || { log "installing openssh-client"; apt-get update -qq && apt-get install -y -qq openssh-client; }
  log "generating deploy SSH keypair"
  ssh-keygen -t ed25519 -f "$HOME/.ssh/oracle_deploy" -N "" -C oracle-deploy
}

ensure_tfvars() {
  [[ -f terraform.tfvars ]] && [[ -f "$HOME/.oci/oci_api_key.pem" ]] && return
  log "terraform.tfvars or OCI API key missing, bootstrapping"
  ensure_bws
  ensure_ssh_key
  : "${BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN:?BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN must be set to bootstrap terraform.tfvars}"
  SECRETS_BACKEND=bitwarden wf vm creds sync
  {
    echo "ssh_public_key_path = \"$HOME/.ssh/oracle_deploy.pub\""
    echo "budget_alert_email  = \"owner@example.com\""
    echo "bws_read_access_token = \"$BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN\""
  } >> terraform.tfvars
}

# Maps a region identifier (e.g. eu-amsterdam-1) to its 3-letter region key
# (e.g. AMS) via a live API lookup — avoids hardcoding OCI's full region
# table, which changes as Oracle adds regions.
region_key_for() {
  local region_name="$1"
  oci iam region list --output json 2>/dev/null \
    | python3 -c "import json,sys; data=json.load(sys.stdin); m={r['name']: r['key'] for r in data}; print(m.get('$region_name',''))"
}

# Attempts to subscribe the tenancy to a region via the OCI CLI. Returns 1
# (caller falls through to the existing skip-to-next-region behavior) if the
# CLI/config isn't available, the region key can't be resolved, or the
# subscribe call itself fails (e.g. still capped at 1 region on Free Tier).
auto_subscribe_region() {
  local region_name="$1"
  ensure_oci_cli || return 1
  ensure_oci_config || return 1
  local key tenancy_ocid
  key="$(region_key_for "$region_name")"
  if [[ -z "$key" ]]; then
    log "could not resolve a region key for $region_name — skipping auto-subscribe"
    return 1
  fi
  tenancy_ocid="$(grep -E '^\s*tenancy_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
  log "attempting to auto-subscribe tenancy to $region_name (key=$key)"
  oci iam region-subscription create --region-key "$key" --tenancy-id "$tenancy_ocid" 2>&1 | tee -a "$APPLY_LOG" >&2
}

instances_exist() {
  terraform state list 2>/dev/null | grep -q '^oci_core_instance.main$'
}

# If budget-guard.tf's function terminated the VMs for overspending, this
# script must NOT just recreate them next hour — that would fight the kill
# switch and defeat its entire purpose. Best-effort and fails OPEN (proceeds
# with a warning) on anything except a clear "yes, over threshold" answer —
# a missing oci CLI, IAM permission gap, or API error shouldn't silently
# break capacity retries for unrelated reasons; only a confirmed overspend
# blocks. Not verified against a live tenancy from here (same usage-api call
# budget-guard's function makes) — if this misbehaves, the real fix is
# checking Console -> Billing before assuming this check is trustworthy.
spend_over_stop_threshold() {
  ensure_oci_cli || return 1
  ensure_oci_config || return 1
  local tenancy_ocid compartment_ocid budget_usd stop_pct
  tenancy_ocid="$(grep -E '^\s*tenancy_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
  compartment_ocid="$(grep -E '^\s*compartment_ocid\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
  budget_usd="$(grep -E '^\s*budget_amount_usd\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*([0-9.]+).*/\1/')"
  stop_pct="$(grep -E '^\s*budget_guard_stop_threshold_pct\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*([0-9.]+).*/\1/')"
  budget_usd="${budget_usd:-20}"
  stop_pct="${stop_pct:-50}"
  [[ -n "$tenancy_ocid" && -n "$compartment_ocid" ]] || return 1

  local start_of_month now
  start_of_month="$(date -u +%Y-%m-01T00:00:00.000Z)"
  now="$(date -u +%Y-%m-%dT%H:%M:%S.000Z)"

  local spend
  spend="$(oci usage-api usage-summary request-summarized-usages \
      --tenant-id "$tenancy_ocid" \
      --time-usage-started "$start_of_month" \
      --time-usage-ended "$now" \
      --granularity MONTHLY \
      --query-type COST \
      --compartment-depth 6 \
      --output json 2>/dev/null \
    | python3 -c "
import json,sys
try:
  d=json.load(sys.stdin)
  items=d.get('data',{}).get('items',[])
  print(sum(i.get('computed-amount') or 0 for i in items))
except Exception:
  print('')
")"
  [[ -n "$spend" ]] || return 1

  python3 -c "import sys; sys.exit(0 if float('$spend') / float('$budget_usd') * 100 >= float('$stop_pct') else 1)"
}

report_success() {
  log "SUCCESS — VM instance created"
  terraform output -json | python3 -c 'import json,sys; d=json.load(sys.stdin); print("vm_host:", d["vm_host"]["value"])'
}

# If the winning attempt used a non-default region or shape (via -var
# override), that override only applied to THIS terraform invocation — it's
# never written back to terraform.tfvars. Left as-is, any future plain
# `terraform apply` (run by hand, not through this script) would read the
# stale default, point the provider at the wrong region/shape for existing
# resource IDs, and could produce a destroy-and-recreate plan. Persist the
# winning values so tfvars matches what's actually running.
persist_winning_config() {
  local region="$1" shape="$2"
  if [[ -n "$region" ]]; then
    local current
    current="$(grep -E '^\s*region\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
    if [[ "$region" != "$current" ]]; then
      log "persisting winning region '$region' into terraform.tfvars (was '$current') so a future plain terraform apply targets the right place"
      sed -i.bak -E "s|^([[:space:]]*region[[:space:]]*=[[:space:]]*).*|\\1\"$region\"|" terraform.tfvars
      rm -f terraform.tfvars.bak
    fi
  fi
  if [[ "$shape" != "VM.Standard.A1.Flex" ]]; then
    log "persisting fallback shape '$shape' into terraform.tfvars so a future plain terraform apply doesn't try to move it back to A1.Flex"
    if grep -qE '^[[:space:]]*instance_shape[[:space:]]*=' terraform.tfvars; then
      sed -i.bak -E "s|^([[:space:]]*instance_shape[[:space:]]*=[[:space:]]*).*|\\1\"$shape\"|" terraform.tfvars
      rm -f terraform.tfvars.bak
    else
      echo "instance_shape = \"$shape\"" >> terraform.tfvars
    fi
  fi
}

ensure_terraform
ensure_tfvars
terraform init -input=false >/dev/null

# Sync local state with reality before checking it. If budget-guard.tf's
# function terminated the VMs out-of-band (via the Compute API directly, not
# Terraform), plain `terraform state list` would still show them as present
# — this is what actually catches that drift.
terraform apply -refresh-only -auto-approve -input=false >/dev/null 2>&1 || true

if instances_exist; then
  log "both VM instances already exist — nothing to do"
  report_success
  exit 0
fi

if spend_over_stop_threshold; then
  log "REFUSING to create/recreate instances — month-to-date spend is already at or above the budget-guard stop threshold. If the VMs were just terminated by budget-guard for overspending, recreating them now would fight the kill switch. Check Console -> Billing and either wait for the monthly reset or re-run manually once you've confirmed it's safe."
  exit 1
fi

# Default to just the region already in terraform.tfvars (no rotation) unless
# --regions was passed explicitly.
if [[ -z "$REGIONS" ]]; then
  REGIONS="$(grep -E '^\s*region\s*=' terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/')"
fi
IFS=',' read -ra REGION_LIST <<< "$REGIONS"

# A bare retry loop can't tell a transient capacity shortage from a real
# config/auth bug (or a region the tenancy isn't subscribed to) and would
# blindly retry all three (see research on community oci-arm-host-capacity-
# style tools). So each attempt's output is classified into one of three
# outcomes:
#   - "Out of host capacity"           -> keep cycling ADs in this region
#   - not subscribed / invalid region  -> stop trying this region, move to next
#   - anything else                    -> real bug, abort the whole script now
# rather than burning the whole attempt/time budget on something retrying
# can't fix.
APPLY_LOG="$(mktemp)"
trap 'rm -f "$APPLY_LOG"' EXIT

deadline=$(( $(date +%s) + TIMEOUT_SECONDS ))
attempt=0

# Runs one shape (A1.Flex, or the E2.1.Micro fallback) across every
# configured region x AD combination within the shared attempt/time budget.
# Exits the whole script on success or on a non-capacity/non-subscription
# error; returns 1 if this shape is exhausted everywhere so the caller can
# fall through to the next shape (or give up).
try_shape() {
  local shape="$1"
  local region ad
  local subscribe_attempted=""
  for region in "${REGION_LIST[@]}"; do
    for (( ad = 0; ad < AD_COUNT; ad++ )); do
      if (( attempt >= MAX_ATTEMPTS )) || (( $(date +%s) >= deadline )); then
        return 1
      fi
      attempt=$(( attempt + 1 ))
      local region_args=()
      [[ -n "$region" ]] && region_args=(-var "region=$region")
      log "attempt $attempt/$MAX_ATTEMPTS shape=$shape region=${region:-<tfvars default>} ad=$ad"
      if terraform apply -auto-approve -input=false \
          -var "instance_shape=$shape" -var "availability_domain_index=$ad" \
          "${region_args[@]}" 2>&1 | tee "$APPLY_LOG"; then
        if instances_exist; then
          persist_winning_config "$region" "$shape"
          report_success
          exit 0
        fi
      fi
      if grep -qi 'out of host capacity' "$APPLY_LOG"; then
        log "attempt $attempt: out of host capacity (shape=$shape region=${region:-default} ad=$ad), trying next AD"
        continue
      # "NotAuthorizedOrNotFound" alone is OCI's generic code for ANY
      # inaccessible resource (a bad image/subnet OCID triggers it too) — it
      # must NOT be treated as "skip this region" on its own, or a real config
      # bug gets silently swallowed as region-unavailable (caught in testing:
      # a bad image OCID error matched and was wrongly skipped). Only treat it
      # as a region problem when the word "region" co-occurs with it.
      elif grep -qi 'region' "$APPLY_LOG" \
          && grep -qiE 'not (authorized|subscribed)|not a (valid|subscribed) region|is not subscribed' "$APPLY_LOG"; then
        if $AUTO_SUBSCRIBE_REGIONS && [[ -n "$region" ]] && [[ "$subscribe_attempted" != *" $region "* ]]; then
          subscribe_attempted+=" $region "
          if auto_subscribe_region "$region"; then
            log "auto-subscribed to $region — retrying this AD"
            continue
          fi
        fi
        log "region ${region:-default} not usable (not subscribed, or invalid) for shape=$shape — skipping to next region"
        break
      else
        log "FATAL — apply failed for a reason other than capacity/region-subscription; stopping instead of retrying blindly. Last 30 lines:"
        tail -30 "$APPLY_LOG"
        exit 3
      fi
    done
  done
  return 1
}

# try_shape exits the script directly on success (or on a fatal error); a
# plain return here just means this shape is exhausted everywhere it tried.
try_shape "VM.Standard.A1.Flex" || true

if $ALLOW_E2_MICRO_FALLBACK; then
  log "A1.Flex exhausted across all configured region/AD combinations — falling back to VM.Standard.E2.1.Micro (1/8 OCPU + 1GB RAM each, last resort)"
  try_shape "VM.Standard.E2.1.Micro" || true
fi

log "exhausted $attempt attempt(s) / ${TIMEOUT_SECONDS}s budget without capacity — no VMs created this run"
exit 1
