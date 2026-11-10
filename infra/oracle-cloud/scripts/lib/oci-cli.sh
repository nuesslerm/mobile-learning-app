#!/usr/bin/env bash
# Shared helpers for scripts that need the `oci` CLI (separate from
# `terraform` — retry-apply.sh's --auto-subscribe-regions and
# deploy-budget-guard-function.sh's OCIR namespace lookup both need it).
# Source this file; don't execute it directly.

# Installs the oci CLI via pip if it's not already on PATH. Prefers pip over
# curling Oracle's install script blind — oci-cli is a normal, well-known
# PyPI package.
ensure_oci_cli() {
  command -v oci >/dev/null 2>&1 && return
  echo "[oci-cli] oci CLI not on PATH, installing via pip" >&2
  (pip3 install --user --quiet oci-cli || pip install --user --quiet oci-cli)
  export PATH="$HOME/.local/bin:$PATH"
  command -v oci >/dev/null 2>&1 || { echo "[oci-cli] could not install oci CLI" >&2; return 1; }
}

# Generates ~/.oci/config from the same values already in terraform.tfvars
# (tenancy_ocid, user_ocid, api_key_fingerprint, api_private_key_path, region,
# api_private_key_passphrase) so the oci CLI can authenticate without a
# separate manual `oci setup config` walkthrough. Idempotent — no-ops if the
# config already exists. Expects to be called with terraform.tfvars in the
# current directory (both callers cd into infra/oracle-cloud first).
ensure_oci_config() {
  [[ -f "$HOME/.oci/config" ]] && return
  [[ -f terraform.tfvars ]] || { echo "[oci-cli] terraform.tfvars not found — run this from infra/oracle-cloud, or run the retry-apply.sh bootstrap first" >&2; return 1; }

  tfvar() {
    grep -E "^\s*$1\s*=" terraform.tfvars | tail -1 | sed -E 's/.*=\s*"([^"]*)".*/\1/'
  }

  local tenancy user fingerprint key_path region passphrase
  tenancy="$(tfvar tenancy_ocid)"
  user="$(tfvar user_ocid)"
  fingerprint="$(tfvar api_key_fingerprint)"
  key_path="$(tfvar api_private_key_path)"
  region="$(tfvar region)"
  passphrase="$(tfvar api_private_key_passphrase)"

  mkdir -p "$HOME/.oci"
  {
    echo "[DEFAULT]"
    echo "tenancy=$tenancy"
    echo "user=$user"
    echo "fingerprint=$fingerprint"
    echo "key_file=$key_path"
    echo "region=$region"
    [[ -n "$passphrase" ]] && echo "pass_phrase=$passphrase"
  } > "$HOME/.oci/config"
  chmod 600 "$HOME/.oci/config"
  echo "[oci-cli] wrote $HOME/.oci/config from terraform.tfvars" >&2
}
