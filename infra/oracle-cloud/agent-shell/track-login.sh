# Sourced by /etc/profile for every interactive LOGIN shell -- the only way
# in is `AllowUsers agent` over SSH (sshd_config), so this fires once per
# SSH session. This container has no wtmp/lastlog set up (a minimal base
# image, and PAM's own lastlog wiring is not something to depend on
# working by default in a container), so this file IS the login-tracking
# mechanism, not a supplement to one. Read by heartbeat.sh's 15-min
# summary.
mkdir -p /var/log/agent-shell 2>/dev/null || true
printf '%s login user=%s tty=%s ssh_client=%s\n' \
  "$(date -u +%FT%TZ)" "$(whoami)" "$(tty 2>/dev/null || echo -)" "${SSH_CLIENT:-unknown}" \
  >> /var/log/agent-shell/logins.log 2>/dev/null || true
