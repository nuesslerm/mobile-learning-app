#!/usr/bin/env bash
#
# Per-package setup for @cli/vnc-auth-capture — installs, ON DEMAND, the tool
# CLIs whose interactive login this package captures: idempotent (fast no-op when present), best-effort
# (warns, never hard-fails on a missing network), and NOT run at plain workspace
# install so a fresh checkout stays cheap.
#
# Usage:
#   bash clis/vnc-auth-capture/scripts/setup.sh <tool>   # claude | codex (U3); opencode | gemini | gcloud (U8)
#   bash clis/vnc-auth-capture/scripts/setup.sh all
#   pnpm --filter @cli/vnc-auth-capture run setup <tool>
set -euo pipefail

log() { echo "[setup-vnc-auth-capture] $*"; }
warn() { echo "[setup-vnc-auth-capture] WARNING: $*" >&2; }
have() { command -v "$1" >/dev/null 2>&1; }

# Retries once (a transient registry timeout during a multi-package install
# run has been observed live to fail one package while its neighbors on
# either side succeed) and, on a second failure, surfaces the command's own
# real stderr instead of discarding it -- a swallowed `>/dev/null 2>&1` here
# previously meant an install failure left literally no diagnostic trace
# anywhere, not even in the build log.
run_with_retry() {
  local label="$1"; shift
  local err
  if err="$("$@" 2>&1 >/dev/null)"; then return 0; fi
  log "first attempt to install ${label} failed, retrying once..."
  if err="$("$@" 2>&1 >/dev/null)"; then log "${label} installed (on retry)."; return 0; fi
  warn "could not install ${label} after a retry; install manually if you need this adapter. Last error: $(echo "$err" | tail -c 500)"
  return 1
}

install_codex() {
  if have codex; then log "codex already present."; return 0; fi
  if ! have npm; then warn "npm not found; install Node first (needed for @openai/codex)."; return 0; fi
  log "Installing @openai/codex..."
  run_with_retry "@openai/codex" npm i -g @openai/codex && log "codex installed." || true
}

install_claude() {
  if have claude; then log "claude already present."; return 0; fi
  if ! have npm; then warn "npm not found; install Node first (needed for @anthropic-ai/claude-code)."; return 0; fi
  log "Installing @anthropic-ai/claude-code..."
  run_with_retry "@anthropic-ai/claude-code" npm i -g @anthropic-ai/claude-code && log "claude installed." || true
}

install_opencode() {
  if have opencode; then log "opencode already present."; return 0; fi
  log "Installing opencode..."
  run_with_retry "opencode" bash -c 'curl -fsSL https://opencode.ai/install | bash' && log "opencode installed." || true
}

install_gemini() {
  if have gemini; then log "gemini already present."; return 0; fi
  if ! have npm; then warn "npm not found; install Node first (needed for @google/gemini-cli)."; return 0; fi
  log "Installing @google/gemini-cli..."
  run_with_retry "@google/gemini-cli" npm i -g @google/gemini-cli && log "gemini installed." || true
}

install_gcloud() {
  if have gcloud; then log "gcloud already present."; return 0; fi
  log "Installing google-cloud-cli..."
  run_with_retry "google-cloud-cli" bash -c 'curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg | gpg --dearmor -o /usr/share/keyrings/cloud.google.gpg && echo "deb [signed-by=/usr/share/keyrings/cloud.google.gpg] https://packages.cloud.google.com/apt cloud-sdk main" > /etc/apt/sources.list.d/google-cloud-sdk.list && apt-get update && apt-get install -y google-cloud-cli' && log "gcloud installed." || true
}

target="${1:-}"
case "$target" in
  codex) install_codex ;;
  claude) install_claude ;;
  opencode) install_opencode ;;
  gemini) install_gemini ;;
  gcloud) install_gcloud ;;
  all) install_claude; install_codex; install_opencode; install_gemini; install_gcloud ;;
  leetcode) warn "leetcode uses a site login (no CLI to install); its adapter uses persist/verify hooks (U7)." ;;
  "")
    log "No tool selected. Pass a tool: claude | codex | opencode | gemini | gcloud | all."
    ;;
  *)
    warn "unknown tool '${target}'. Expected: claude | codex | all | opencode | gemini | gcloud | leetcode."
    exit 1
    ;;
esac

log "Done."
