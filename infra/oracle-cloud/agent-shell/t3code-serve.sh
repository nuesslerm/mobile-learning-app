#!/bin/bash
# Supervises T3 Code's server (github.com/pingdotgg/t3code) as a persistent
# background process. Runs as `agent` (never root -- it's the process that
# ends up spawning `opencode`/`claude`/`codex` and making real git commits
# on the user's behalf), launched detached by entrypoint.sh the same way
# heartbeat.sh is, guarded by AGENT_SHELL_T3CODE_ENABLED so it stays off
# until verified end to end.
#
# Pinned Node + pinned t3 version, not `node@latest`/`t3@latest`: this is an
# unattended container restart target, and letting either self-upgrade on
# every reboot is the exact "floating tag" failure class this Dockerfile's
# own comment already warns about (node:22-bookworm-slim drifting 9 minors
# from the pinned agent-base). Bump AGENT_SHELL_T3CODE_VERSION /
# AGENT_SHELL_T3CODE_NODE_VERSION deliberately in .env.defaults.
#
# The repo-wide `mise.toml` pins node=22.14.0 for the whole toolchain (wf,
# remote-chat-agent parity, etc.) -- T3 Code's own server package requires
# `^22.16 || ^23.11 || >=24.10`, which 22.14.0 fails. `mise exec` overrides
# the active version for exactly this one invocation without touching that
# shared pin (no `mise use -g`, no second mise.toml under /workspace).
set -uo pipefail

NODE_VERSION="${AGENT_SHELL_T3CODE_NODE_VERSION:-24.13.1}"
T3_VERSION="${AGENT_SHELL_T3CODE_VERSION:?AGENT_SHELL_T3CODE_VERSION must be set}"
PROJECT_DIR="${T3CODE_PROJECT_DIR:-/workspace/repos/t3code-project}"
PIDFILE=/var/log/agent-shell/t3code.pid

# `app` shares the tailscale sidecar's NETWORK namespace (network_mode:
# service:tailscale) but not its MOUNT namespace, and has no `tailscale`
# CLI of its own (confirmed: nothing installs one in this Dockerfile) --
# so there is no way to shell out to `tailscale ip -4` or drive
# `tailscaled`'s control socket from here (that's also why
# T3CODE_TAILSCALE_SERVE is not used: it would try to do exactly that and
# fail). Reading the tailscale0 interface's own address directly works
# because the network namespace itself IS shared -- this container can see
# and bind to that interface, it just can't drive the sidecar's control
# plane. This is what makes the tailnet-only exposure mode (Option A) the
# one that actually works unmodified in this container topology, not just
# the lowest-exposure choice.
#
# Via node's os.networkInterfaces(), not `ip addr`/`ifconfig` -- confirmed
# live that neither iproute2 nor net-tools is installed in this image
# (`ip: not found`, `ifconfig: not found`), while node is guaranteed
# present (it's what mise/npx run under anyway).
resolve_bind_host() {
  node -e '
    const nets = require("os").networkInterfaces()["tailscale0"] || [];
    const v4 = nets.find((n) => n.family === "IPv4");
    if (v4) console.log(v4.address);
  '
}

mise which "node@${NODE_VERSION}" >/dev/null 2>&1 || mise install "node@${NODE_VERSION}"

while true; do
  BIND_HOST="$(resolve_bind_host)"
  if [ -z "$BIND_HOST" ]; then
    echo "[t3code-serve] tailscale0 has no address yet, retrying in 5s" >&2
    sleep 5
    continue
  fi

  mise exec "node@${NODE_VERSION}" -- \
    npx --yes "t3@${T3_VERSION}" serve \
      --host "$BIND_HOST" \
      --port "${T3CODE_PORT:-3773}" \
      "$PROJECT_DIR" &
  echo $! > "$PIDFILE"
  wait "$(cat "$PIDFILE")"
  echo "[t3code-serve] exited (host=${BIND_HOST}), restarting in 5s" >&2
  sleep 5
done
