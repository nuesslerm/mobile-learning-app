#!/usr/bin/env bash
# Tool-agnostic bootstrap: fresh checkout -> working state (mise toolchain, wf/rtk/opencode, deps, agent
# config, tailnet). Claude: SessionStart hook; Codex cloud: environment setup script `bash .agents/setup.sh`.
set -euo pipefail

REPO_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$REPO_ROOT"
BIN="$HOME/.local/bin"
RTK_VERSION=0.47.0
mkdir -p "$BIN"
case ":$PATH:" in *":$BIN:"*) ;; *) export PATH="$BIN:$PATH" ;; esac

log() { echo "[setup] $*"; }
warn() { echo "[setup] WARN: $*" >&2; }
os() { uname -s | tr '[:upper:]' '[:lower:]'; }
arch() { case "$(uname -m)" in aarch64 | arm64) echo arm64 ;; *) echo x64 ;; esac; }
pin() { sed -n "s/^$1 = \"\(.*\)\"/\1/p" mise.toml; }

# Plain release-asset URLs only: sandbox proxies 403 the GitHub REST API that
# mise and upstream installers resolve versions through.
fetch_tgz() {
  local url=$1 member=$2 dest=$3 tmp
  tmp=$(mktemp -d)
  curl -fsSL "$url" | tar -xz -C "$tmp" 2>/dev/null || { rm -rf "$tmp"; return 1; }
  find "$tmp" -type f -name "$member" -exec install -m 0755 {} "$dest" \; -quit
  rm -rf "$tmp"
  [ -x "$dest" ]
}

TOOLS="node pnpm gh actionlint bws terraform tailscale"
step_mise() {
  local missing=0 t want
  want=$(pin node)
  for t in $TOOLS; do command -v "$t" >/dev/null 2>&1 || missing=1; done
  [ "$(node -v 2>/dev/null)" = "v$want" ] || missing=1
  [ "$missing" = 0 ] && return
  if ! command -v mise >/dev/null 2>&1 && [ ! -x "$BIN/mise" ]; then
    curl -fsSL https://mise.run | MISE_INSTALL_PATH="$BIN/mise" sh >/dev/null 2>&1 || warn "could not install mise"
  fi
  local mise
  mise=$(command -v mise || echo "$BIN/mise")
  [ -x "$mise" ] && { "$mise" trust -q mise.toml >/dev/null 2>&1 || true; "$mise" install >/dev/null 2>&1 || warn "mise install failed"; }
  local tool path
  if [ -x "$mise" ] && [ "$(node -v 2>/dev/null)" != "v$want" ]; then
    path=$("$mise" which node 2>/dev/null || true)
    [ -x "$path" ] && ln -sf "$path" "$BIN/node"
    hash -r
  fi
  for tool in $TOOLS; do
    command -v "$tool" >/dev/null 2>&1 && continue
    path=$([ -x "$mise" ] && "$mise" which "$tool" 2>/dev/null || true)
    if [ -n "$path" ] && [ -x "$path" ]; then ln -sf "$path" "$BIN/$tool"; continue; fi
    if [ "$tool" = tailscale ] && [ "$(os)" = linux ]; then
      local v a
      v=$(pin tailscale) a=$([ "$(arch)" = arm64 ] && echo arm64 || echo amd64)
      fetch_tgz "https://pkgs.tailscale.com/stable/tailscale_${v}_${a}.tgz" tailscale "$BIN/tailscale" &&
        fetch_tgz "https://pkgs.tailscale.com/stable/tailscale_${v}_${a}.tgz" tailscaled "$BIN/tailscaled" && continue
    fi
    warn "$tool not provisioned"
  done
}

# The agent-shell runbooks need ssh; rsync is kept for ad-hoc copies to the VM.
step_apt() {
  local pkgs=()
  command -v ssh >/dev/null 2>&1 || pkgs+=(openssh-client)
  command -v rsync >/dev/null 2>&1 || pkgs+=(rsync)
  [ ${#pkgs[@]} -eq 0 ] && return
  [ "$(id -u)" = 0 ] && command -v apt-get >/dev/null 2>&1 || { warn "missing: ${pkgs[*]}"; return; }
  apt-get install -y -qq "${pkgs[@]}" >/dev/null 2>&1 || warn "apt-get install ${pkgs[*]} failed"
}

# The Rust wf (cargo: a no-op when unchanged) linked as `wf`, with t3-serve, rca-exec and wf-mimic beside it for `wf t3`, `wf exec` and `wf mimic`.
# Never steal the link from another live checkout: a deleted worktree would leave every `wf` call on the machine dangling.
step_wf() {
  command -v cargo >/dev/null 2>&1 || { warn "cargo missing: wf not built"; return; }
  cargo build --release --quiet --manifest-path clis/Cargo.toml -p wf -p t3-serve -p rca-exec -p wf-mimic || { warn "building wf failed"; return; }
  local target="$REPO_ROOT/clis/target/release/wf" current
  current=$(readlink "$BIN/wf" 2>/dev/null || true)
  if [ -n "$current" ] && [ "$current" != "$target" ] && [ -e "$current" ]; then return; fi
  ln -sf "$target" "$BIN/wf"
}

# Never `cargo install rtk` (an unrelated crate) or `rtk init -g` (writes ~/.claude).
step_rtk() {
  "$BIN/rtk" --version 2>/dev/null | grep -q "^rtk $RTK_VERSION" && return
  local asset base sum tmp
  case "$(os)-$(arch)" in
    linux-x64) asset=rtk-x86_64-unknown-linux-musl.tar.gz ;;
    linux-arm64) asset=rtk-aarch64-unknown-linux-gnu.tar.gz ;;
    darwin-x64) asset=rtk-x86_64-apple-darwin.tar.gz ;;
    darwin-arm64) asset=rtk-aarch64-apple-darwin.tar.gz ;;
    *) warn "rtk: no asset for $(os)/$(arch)"; return ;;
  esac
  base="https://github.com/rtk-ai/rtk/releases/download/v$RTK_VERSION"
  sum=$(curl -fsSL "$base/checksums.txt" | awk -v a="$asset" '$2 == a { print $1 }' || true)
  tmp=$(mktemp)
  if [ -z "$sum" ] || ! curl -fsSL -o "$tmp" "$base/$asset"; then warn "rtk download failed"; rm -f "$tmp"; return; fi
  if [ "$( (sha256sum "$tmp" 2>/dev/null || shasum -a 256 "$tmp") | cut -d' ' -f1)" != "$sum" ]; then
    warn "rtk checksum mismatch"; rm -f "$tmp"; return
  fi
  tar -xzf "$tmp" -C "$BIN" rtk 2>/dev/null || tar -xzf "$tmp" -C "$BIN" || warn "rtk extract failed"
  rm -f "$tmp"
}

step_opencode() {
  command -v opencode >/dev/null 2>&1 && return
  local dir="$REPO_ROOT/.opencode/bin"
  mkdir -p "$dir"
  fetch_tgz "https://github.com/anomalyco/opencode/releases/latest/download/opencode-$(os)-$(arch).tar.gz" opencode "$dir/opencode" &&
    ln -sf "$dir/opencode" "$BIN/opencode" || warn "opencode download failed"
}

# Marks this checkout trusted so `claude` honours .claude/settings.json's allowlist.
step_claude_trust() {
  node -e '
    const fs = require("fs"), p = require("os").homedir() + "/.claude.json";
    let c = {}; try { c = JSON.parse(fs.readFileSync(p, "utf8")); } catch (e) { if (e.code !== "ENOENT") process.exit(0); }
    const r = process.argv[1]; c.projects ??= {};
    if (c.projects[r]?.hasTrustDialogAccepted) process.exit(0);
    c.projects[r] = { ...c.projects[r], hasTrustDialogAccepted: true };
    fs.writeFileSync(p, JSON.stringify(c, null, 2) + "\n");
  ' "$REPO_ROOT" || warn "could not mark the repo trusted"
}

step_tailnet() {
  command -v tailscale >/dev/null 2>&1 || return
  tailscale status --json 2>/dev/null | grep -q '"Online": *true' && return
  timeout 180 wf vm net up >/dev/null 2>&1 || warn "wf vm net up failed (VM services unreachable)"
}

step_pnpm() {
  command -v pnpm >/dev/null 2>&1 || corepack enable pnpm
  cmp -s pnpm-lock.yaml node_modules/.pnpm/lock.yaml && return
  log "pnpm install"
  pnpm install
}

step_lefthook() {
  grep -qs lefthook "$(git rev-parse --git-path hooks/pre-commit)" && return
  pnpm exec lefthook install --force >/dev/null || warn "lefthook install failed"
}

step_agent_config() {
  if [ -x clis/target/release/wf ]; then
    # A release binary built before the verb and noun surface only knows `agents sync`.
    clis/target/release/wf agent-config sync --check >/dev/null 2>&1 && return
    clis/target/release/wf agent-config sync >/dev/null 2>&1 || clis/target/release/wf agent-config sync >/dev/null || warn "agent-config-sync failed"
    return
  fi
  warn "clis/target/release/wf missing; build it (cargo build --release in clis/) to sync agent config"
}

if [ "${DOCKER_BUILD:-}" != true ]; then
  step_apt; step_mise; step_wf; step_rtk; step_opencode
fi
step_pnpm
step_lefthook
step_agent_config
step_claude_trust
if [ "${DOCKER_BUILD:-}" != true ]; then step_tailnet; fi
log "done"
