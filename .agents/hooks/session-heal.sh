#!/usr/bin/env bash
# UserPromptSubmit: repair what a skipped SessionStart or a wedged tailscaled leaves behind; silent when healthy.
root=${CLAUDE_PROJECT_DIR:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)}
[ -d "$root/node_modules" ] || (cd "$root" && pnpm install >&2) || exit 1
command -v tailscale >/dev/null 2>&1 || exit 0
tailscale status --json 2>/dev/null | grep -q '"Online": *true' && exit 0
timeout 180 wf vm net up >/dev/null 2>&1 || echo "[session-heal] wf vm net up failed" >&2
exit 0
