#!/usr/bin/env bash
# Mirrors backend/remote-chat-agent/docker-entrypoint.sh. Both this container
# and the agent may be the first to start on a fresh VM, and docker creates a
# new named volume's root as `root:root 0755` -- which a non-root process
# cannot create inside. Whichever boots first initialises the layout; the other
# finds it done. Without this, ordering decides whether opencode comes up.
#
# Why not simply `user: "1001:1001"` in compose: that would leave this process
# unable to create /workspace/home on a fresh volume, and with HOME pointing
# there, opencode's own session store would fail to initialise.
#
# One uid owns this volume rather than a shared gid + `umask 002` -- measured,
# with the rejected alternative, in docs/solutions/infra/
# sharing-a-docker-volume-across-containers-needs-one-uid.md
set -euo pipefail

if [[ -d /workspace && "$(id -u)" == "0" ]]; then
  chown agent:agent /workspace 2>/dev/null || true
  chmod 775 /workspace 2>/dev/null || true
  runuser -u agent -- mkdir -p /workspace/home /workspace/repos /workspace/worktrees 2>/dev/null \
    || echo "[opencode-entrypoint] WARNING: could not initialise /workspace layout" >&2
# `runuser` resets HOME from /etc/passwd and DISCARDS the value compose
# sets -- verified directly: with `-e HOME=/workspace/home`, the dropped
# process reported HOME=/home/agent. Left unfixed, opencode's session store
# and ~/.claude would land on the container's ephemeral layer while looking
# entirely correct. Making passwd itself the source of truth fixes it for
# every launch path, not just this one.
usermod -d /workspace/home agent 2>/dev/null || true
fi

# Gated on /workspace EXISTING, not merely on being root -- this image is
# published to the same `latest` tag the VM pulls by default, so it must behave
# identically under the OLD compose file (which mounts no /workspace and points
# WORKTREE_BASE_DIR at a root-owned volume). Dropping to 1001 there would leave
# the service unable to write its own worktrees: a silent, delayed breakage
# triggered by nothing more than a routine redeploy picking up `latest`.
# With this gate, image and compose can land in either order.
if [[ -d /workspace ]] && [[ "$(id -u)" == "0" ]] && id -u agent >/dev/null 2>&1; then
  exec runuser -u agent -- "$@"
fi

exec "$@"
