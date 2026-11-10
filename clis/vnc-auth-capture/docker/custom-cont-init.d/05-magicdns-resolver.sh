#!/usr/bin/with-contenv bash
# Add Tailscale's MagicDNS resolver (100.100.100.100) to DNS, keeping the
# original Docker-embedded resolver (127.0.0.11) as a fallback for everything
# else. Root-caused in backend/remote-chat-agent (see docs/known-gaps.md):
# `network_mode: service:tailscale` shares NETWORK namespace with the
# sidecar but not MOUNT namespace, so Docker still writes this container its
# own separate /etc/resolv.conf pointing only at 127.0.0.11, which has never
# heard of MagicDNS. This container calls out to `vm-ops-api-main:8080`
# (agent-shell-relay.ts's DEFAULT_VM_OPS_API_BASE_URL) -- an unqualified
# tailnet short name that only resolves via MagicDNS's search-domain
# expansion, so it is actively exposed to this bug today, not just
# structurally similar. Numbered low (05-) so it runs before any other
# init script that might depend on DNS. Two nameservers, not a replacement:
# an earlier version of this same fix elsewhere replaced resolv.conf
# entirely and broke every non-tailnet lookup (confirmed live via a failed
# BetterStack log delivery) -- Tailscale's resolver returns SERVFAIL/timeout
# (not a clean NXDOMAIN) for anything outside its own suffix on a tailnet
# with no global nameservers configured, which is exactly the case glibc's
# resolver falls through to the second nameserver for.
set -e

if [ "$(id -u)" = "0" ]; then
  {
    echo "nameserver 100.100.100.100"
    echo "nameserver 127.0.0.11"
    echo "search taila61461.ts.net"
    echo "options ndots:1"
  } > /etc/resolv.conf 2>/dev/null \
    && echo "[vnc-auth-init] /etc/resolv.conf rewritten for MagicDNS" \
    || echo "[vnc-auth-init] WARNING: could not rewrite /etc/resolv.conf for MagicDNS -- *.ts.net / tailnet short-name lookups from this container will keep failing" >&2
fi
