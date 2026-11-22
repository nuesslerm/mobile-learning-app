#!/usr/bin/env bash
# Keeps a test step from freezing the runner (usage: memguard.sh <floor_mb> <log>). Every 2 s it reads MemAvailable; under the floor it logs the 15 largest processes and
# kills the largest one that runs from the checkout or the runner's temp directory, so the step fails fast with the culprit named in a log that uploads, instead of the
# whole machine thrashing until the job's time is out with none. Every minute it logs one heartbeat line, which shows a log that ends because the machine stopped.
floor="${1:?floor in MB}"
log="${2:?log file}"
roots=()
[ -z "${GITHUB_WORKSPACE:-}" ] || roots+=("$GITHUB_WORKSPACE")
[ -z "${RUNNER_TEMP:-}" ] || roots+=("$RUNNER_TEMP")
beat=0
while sleep 2; do
  avail=$(awk '/^MemAvailable/ { print int($2 / 1024) }' /proc/meminfo)
  beat=$((beat + 2))
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
  sleep 10
done
