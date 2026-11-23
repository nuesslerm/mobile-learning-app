#!/usr/bin/env bash
# Keeps a test step from freezing the runner (usage: memguard.sh <floor_mb> <log>). Every 2 s it reads MemAvailable; under the floor it logs the 15 largest processes and
# kills the largest one that runs from the checkout or the runner's temp directory, so the step fails fast with the culprit named in a log that uploads, instead of the
# whole machine thrashing until the job's time is out with none. Every minute it logs one heartbeat line, which shows a log that ends because the machine stopped.
# With GH_TOKEN, GITHUB_REPOSITORY, GITHUB_SHA and HEARTBEAT_NAME set it also keeps one check run of the commit up to date every 30 s with the machine's state and the
# tail of each stage log: a job whose machine froze never uploads its log, and the check run is what is left of it (the API shows it; the log is a 404).
floor="${1:?floor in MB}"
log="${2:?log file}"
stages="${3:-}"
check=""
fails=0
lastfail=""
errfile=$(mktemp)
post() {
  [ -n "${GH_TOKEN:-}" ] && [ -n "${GITHUB_REPOSITORY:-}" ] && [ -n "${GITHUB_SHA:-}" ] && [ -n "${HEARTBEAT_NAME:-}" ] || return 0
  local state="$1" text="$2" conclusion=()
  [ "$state" != completed ] || conclusion=(-f conclusion=neutral)
  if [ -z "$check" ]; then
    check=$(timeout 20 gh api -X POST "repos/$GITHUB_REPOSITORY/check-runs" -f "name=heartbeat $HEARTBEAT_NAME" -f "head_sha=$GITHUB_SHA" -f status=in_progress -f "output[title]=$HEARTBEAT_NAME" -f "output[summary]=$text" --jq .id 2> "$errfile") || { check=""; failed; }
    return 0
  fi
  timeout 20 gh api -X PATCH "repos/$GITHUB_REPOSITORY/check-runs/$check" -f "status=$state" "${conclusion[@]}" -f "output[title]=$HEARTBEAT_NAME" -f "output[summary]=$text" > /dev/null 2> "$errfile" || failed
}
# a post that did not go through is kept for the next one, which says how many failed and the last reason: a runner whose network was cut posts nothing, but the first post after it returns tells when
failed() {
  fails=$((fails + 1))
  lastfail="$(date -u +%H:%M:%S) $(head -c 160 "$errfile" 2> /dev/null | tr '\n' ' ')"
}
state_text() {
  {
    echo "$(date -u +%H:%M:%S) MemAvailable $(awk '/^MemAvailable/ { print int($2 / 1024) }' /proc/meminfo) MB, load $(cut -d' ' -f1 /proc/loadavg), $(ps -e --no-headers | wc -l) processes"
    echo "posts that failed: $fails$([ -z "$lastfail" ] || echo ", the last at $lastfail")"
    echo "network: $(ip -br addr 2> /dev/null | grep -v '^lo ' | head -n 3 | tr '\n' ';') default $(ip route 2> /dev/null | head -n 1), dns $(grep -m1 '^nameserver' /etc/resolv.conf 2> /dev/null), api.github.com $(timeout 3 getent hosts api.github.com 2> /dev/null | head -n 1 | cut -d' ' -f1)"
    echo "disk free (MB): / $(df -m --output=avail / 2> /dev/null | tail -n 1 | tr -d ' ')$([ -d /mnt ] && echo ", /mnt $(df -m --output=avail /mnt 2> /dev/null | tail -n 1 | tr -d ' ')"), used by the checkout $(du -sm "${GITHUB_WORKSPACE:-.}" 2> /dev/null | cut -f1) MB"
    echo
    echo "busiest by cpu (pid ppid %cpu etime stat cmd):"
    ps -eo pid,ppid,pcpu,etimes,stat,comm --sort=-pcpu | sed -n '2,11p'
    echo
    echo "largest by memory (pid ppid rss_kb %cpu etime stat cmd):"
    ps -eo pid,ppid,rss,pcpu,etimes,stat,comm --sort=-rss | sed -n '2,9p'
    if [ -n "$stages" ]; then
      for f in "$stages"/*.log; do
        [ -f "$f" ] || continue
        echo
        echo "$(basename "$f"), the last test lines of it:"
        # only lines about tests and tasks, never a masked value (`::add-mask::` lines carry them) or a long token-like string: this text is public on a public repository
        grep -E 'test .* \.\.\. |has been running for over|Running target|^wf: nx cost' "$f" 2> /dev/null | grep -v 'add-mask' | sed -E 's/[A-Za-z0-9+\/=-]{24,}/***/g' | tail -n 8 | cut -c1-200
      done
    fi
  } 2> /dev/null
}
# an earlier version of this text quoted stage log lines, among them masked values: blank every heartbeat of the last commits that still holds one
redact_old() {
  [ -n "${GH_TOKEN:-}" ] && [ -n "${GITHUB_REPOSITORY:-}" ] || return 0
  local sha id
  for sha in $(timeout 20 gh api "repos/$GITHUB_REPOSITORY/commits?sha=dev&per_page=4" --jq '.[].sha' 2> /dev/null); do
    for id in $(timeout 30 gh api "repos/$GITHUB_REPOSITORY/commits/$sha/check-runs?per_page=100" --jq '.check_runs[] | select((.name | startswith("heartbeat")) and ((.output.summary // "") | test("add-mask|ords/health"))) | .id' 2> /dev/null); do
      timeout 20 gh api -X PATCH "repos/$GITHUB_REPOSITORY/check-runs/$id" -f "output[title]=redacted" -f "output[summary]=redacted: an earlier version of this heartbeat quoted stage log lines" > /dev/null 2>&1 || true
    done
  done
}
finish() {
  post completed "ended with its job: $(state_text)"
  exit 0
}
trap finish TERM
# a sleep the trap can interrupt: bash runs a trap only when the foreground command ends
nap() {
  sleep "$1" &
  wait $!
}
roots=()
[ -z "${GITHUB_WORKSPACE:-}" ] || roots+=("$GITHUB_WORKSPACE")
[ -z "${RUNNER_TEMP:-}" ] || roots+=("$RUNNER_TEMP")
beat=0
cloud=0
sweeps=0
post in_progress "started: $(state_text)"
redact_old &
while nap 2; do
  avail=$(awk '/^MemAvailable/ { print int($2 / 1024) }' /proc/meminfo)
  beat=$((beat + 2))
  cloud=$((cloud + 2))
  if [ "$cloud" -ge 30 ]; then
    cloud=0
    post in_progress "$(state_text)"
    sweeps=$((sweeps + 1))
    [ $((sweeps % 4)) -ne 0 ] || redact_old &
  fi
  if [ "$beat" -ge 60 ]; then
    beat=0
    echo "memguard: $(date -u +%H:%M:%S) MemAvailable ${avail} MB, load $(cut -d' ' -f1 /proc/loadavg), $(ps -e --no-headers | wc -l) processes" >> "$log"
  fi
  disk=$(df -m --output=avail / 2> /dev/null | tail -n 1 | tr -d ' ')
  [ "${avail:-0}" -lt "$floor" ] || [ "${disk:-99999}" -lt "${DISK_FLOOR_MB:-700}" ] || continue
  {
    echo "::error title=machine exhausted::MemAvailable ${avail} MB (floor ${floor}), disk free ${disk} MB (floor ${DISK_FLOOR_MB:-700})"
    echo "--- the largest processes (pid ppid rss_kb etime cmd)"
    ps -eo pid,ppid,rss,etimes,args --sort=-rss | head -n 16 | cut -c1-220
  } >> "$log"
  victim=""
  while read -r pid rss args; do
    for r in "${roots[@]}"; do
      case "$args" in *"$r"*) victim="$pid"; break 2;; esac
    done
  done < <(ps -eo pid=,rss=,args= --sort=-rss)
  if [ -n "$victim" ]; then
    echo "memguard: killed pid $victim, the largest process of this job: $(tr '\0' ' ' < "/proc/$victim/cmdline" 2>/dev/null | cut -c1-300)" >> "$log"
    kill -KILL "$victim" 2>/dev/null || true
  else
    echo "memguard: no process of this job to kill" >> "$log"
  fi
  nap 10
done
