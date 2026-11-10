#!/usr/bin/env bash
# Add Tailscale's MagicDNS resolver (100.100.100.100) to DNS, keeping the
# original Docker-embedded resolver (127.0.0.11) as a fallback for everything
# else. Root-caused in backend/remote-chat-agent this session (see
# docs/known-gaps.md): `network_mode: service:tailscale` shares NETWORK
# namespace with the sidecar but not MOUNT namespace, so Docker still writes
# this container its own separate /etc/resolv.conf pointing only at
# 127.0.0.11, which has never heard of MagicDNS. This container actively
# depends on it: LEETCODE_BACKEND_BASE_URL is a `*.ts.net` hostname
# (docker-compose.capture.yml / .env.defaults), so every request to the real
# leetcode-backend fails with ENOTFOUND without this fix. Two nameservers,
# not a replacement: an earlier version of this same fix elsewhere replaced
# resolv.conf entirely and broke every non-tailnet lookup (confirmed live via
# a failed BetterStack log delivery) -- Tailscale's resolver returns
# SERVFAIL/timeout (not a clean NXDOMAIN) for anything outside its own suffix
# on a tailnet with no global nameservers configured, which is exactly the
# case glibc's resolver falls through to the second nameserver for.
set -euo pipefail

if [[ "$(id -u)" == "0" ]]; then
  {
    echo "nameserver 100.100.100.100"
    echo "nameserver 127.0.0.11"
    echo "search taila61461.ts.net"
    echo "options ndots:1"
  } > /etc/resolv.conf 2>/dev/null \
    || echo "[docker-entrypoint] WARNING: could not rewrite /etc/resolv.conf for MagicDNS -- *.ts.net lookups from this container will keep failing" >&2
fi

# The listener starts Xvfb on :99; a lock left by the previous run can name a PID reused after restart.
rm -f /tmp/.X99-lock /tmp/.X11-unix/X99

exec "$@"
