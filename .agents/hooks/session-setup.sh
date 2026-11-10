#!/usr/bin/env bash
# SessionStart: run .agents/setup.sh quietly; its output would otherwise land in context.
root=${CLAUDE_PROJECT_DIR:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)}
log="$root/.agents/tmp/session-setup.log"
mkdir -p "$root/.agents/tmp"
bash "$root/.agents/setup.sh" >"$log" 2>&1 || { tail -n 20 "$log" >&2; exit 1; }
grep -h "WARN" "$log" >&2 || true
