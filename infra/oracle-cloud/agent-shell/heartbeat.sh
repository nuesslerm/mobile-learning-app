#!/bin/bash
# Runs continuously in the background (launched detached by entrypoint.sh,
# before it execs into sshd as PID 1) and emits one BetterStack log every
# 15 minutes confirming this container is alive.
#
# Built because this container previously had NO telemetry at all: nothing
# distinguished "quietly idle" from "silently dead" short of someone trying
# to SSH in and finding it unreachable.
#
# Uses `wf log emit` (the VM's wf, clis/telemetry's OTLP path): the line is
# printed and sent to BetterStack, and a line that could not be sent exits 1,
# which the call below reports and moves past.
#
# --once: emit a single heartbeat and exit, instead of looping -- lets a
# diagnostic route (vm-ops-api's /services/agent-shell/heartbeat-test) get
# a real, immediate, on-demand reading of current recentLogins/
# recentCommands without waiting up to 15 minutes for the background
# loop's own next tick.
set -uo pipefail

INTERVAL_SECONDS="${AGENT_SHELL_HEARTBEAT_INTERVAL_SECONDS:-900}"
LOGIN_LOG=/var/log/agent-shell/logins.log
HISTORY_FILE=/home/agent/.bash_history

# Recent logins/commands are a summary for operator visibility (an audit
# trail of who used this shell and what they ran), not a security control --
# this container's whole purpose is running trusted interactive agent
# sessions, and its only login account is a single shared `agent` user.
# Values are flattened to a single `|`-joined line each so they survive as
# one log attribute; a `sudo`/`security`-relevant secret typed directly at
# the shell would still be visible here, same as it would be in
# ~/.bash_history itself -- this does not add a new place secrets can leak
# from, it just surfaces what was already sitting on disk.
emit_once() {
  RECENT_LOGINS=$(tail -n 5 "$LOGIN_LOG" 2>/dev/null | tr '\n' '|')
  RECENT_COMMANDS=$(tail -n 20 "$HISTORY_FILE" 2>/dev/null | tr '\n' '|')
  # Total counts alongside the tails above -- "5 recent logins" reads very
  # differently against a lifetime total of 5 (this is the only session
  # this container has ever seen) vs. 5000 (recent tail is a rounding
  # error). Neither tail alone can distinguish those.
  LOGIN_COUNT=$(wc -l < "$LOGIN_LOG" 2>/dev/null || echo 0)
  COMMAND_COUNT=$(wc -l < "$HISTORY_FILE" 2>/dev/null || echo 0)
  # Container uptime/mem/disk -- the same liveness signal every other
  # service's health.heartbeat carries (see frontend/libraries/logging/src/node/
  # heartbeat.ts's uptimeSec/rssMb) so this one isn't the odd one out with
  # nothing but a message name and a login audit trail. /proc/uptime's
  # first field is seconds since boot -- this is PID 1's container uptime,
  # not this script's own process uptime, since the container has no
  # single long-lived Node process to measure against.
  UPTIME_SEC=$(awk '{print int($1)}' /proc/uptime 2>/dev/null || echo -1)
  MEM_USED_MB=$(free -m 2>/dev/null | awk '/^Mem:/{print $3}')
  MEM_TOTAL_MB=$(free -m 2>/dev/null | awk '/^Mem:/{print $2}')
  DISK_USED_PERCENT=$(df -P / 2>/dev/null | awk 'NR==2{gsub("%","",$5); print $5}')
  wf log emit \
    --service agent-shell \
    --level info \
    --message agent_shell.heartbeat \
    --host-label "${HOST_LABEL:-unknown}" \
    --attr "uptimeSec=${UPTIME_SEC:--1}" \
    --attr "memUsedMb=${MEM_USED_MB:--1}" \
    --attr "memTotalMb=${MEM_TOTAL_MB:--1}" \
    --attr "diskUsedPercent=${DISK_USED_PERCENT:--1}" \
    --attr "loginCount=${LOGIN_COUNT:-0}" \
    --attr "commandCount=${COMMAND_COUNT:-0}" \
    --attr "recentLogins=${RECENT_LOGINS:-none}" \
    --attr "recentCommands=${RECENT_COMMANDS:-none}" \
    --attr "ok=true" \
    || echo "[agent-shell-heartbeat] wf log emit failed, continuing" >&2
}

if [ "${1:-}" = "--once" ]; then
  emit_once
  exit 0
fi

while true; do
  emit_once
  sleep "$INTERVAL_SECONDS"
done
