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
post() {
  [ -n "${GH_TOKEN:-}" ] && [ -n "${GITHUB_REPOSITORY:-}" ] && [ -n "${GITHUB_SHA:-}" ] && [ -n "${HEARTBEAT_NAME:-}" ] || return 0
  local state="$1" text="$2" conclusion=()
  [ "$state" != completed ] || conclusion=(-f conclusion=neutral)
  if [ -z "$check" ]; then
    check=$(timeout 20 gh api -X POST "repos/$GITHUB_REPOSITORY/check-runs" -f "name=heartbeat $HEARTBEAT_NAME" -f "head_sha=$GITHUB_SHA" -f status=in_progress -f "output[title]=$HEARTBEAT_NAME" -f "output[summary]=$text" --jq .id 2> /dev/null) || check=""
    return 0
  fi
  timeout 20 gh api -X PATCH "repos/$GITHUB_REPOSITORY/check-runs/$check" -f "status=$state" "${conclusion[@]}" -f "output[title]=$HEARTBEAT_NAME" -f "output[summary]=$text" > /dev/null 2>&1 || true
}
state_text() {
  {
    echo "$(date -u +%H:%M:%S) MemAvailable $(awk '/^MemAvailable/ { print int($2 / 1024) }' /proc/meminfo) MB, load $(cut -d' ' -f1 /proc/loadavg), $(ps -e --no-headers | wc -l) processes"
    echo
    echo "largest by memory (pid ppid rss_kb %cpu etime stat cmd):"
    ps -eo pid,ppid,rss,pcpu,etimes,stat,args --sort=-rss | sed -n '2,9p' | cut -c1-200
    if [ -n "$stages" ]; then
      for f in "$stages"/*.log; do
        [ -f "$f" ] || continue
        echo
        echo "$(basename "$f"), its last 4 lines:"
        tail -n 4 "$f" | cut -c1-240
      done
    fi
  } 2> /dev/null
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
post in_progress "started: $(state_text)"
while nap 2; do
  avail=$(awk '/^MemAvailable/ { print int($2 / 1024) }' /proc/meminfo)
  beat=$((beat + 2))
  cloud=$((cloud + 2))
  if [ "$cloud" -ge 30 ]; then
    cloud=0
    post in_progress "$(state_text)"
  fi
  if [ "$beat" -ge 60 ]; then
    beat=0
    echo "memguard: $(date -u +%H:%M:%S) MemAvailable ${avail} MB, load $(cut -d' ' -f1 /proc/loadavg), $(ps -e --no-headers | wc -l) processes" >> "$log"
  fi
  [ "${avail:-0}" -lt "$floor" ] || continue
  {
    echo "::error title=memory exhausted::MemAvailable ${avail} MB is under ${floor} MB"
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
