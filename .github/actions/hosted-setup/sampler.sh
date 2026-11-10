#!/usr/bin/env bash
# Samples this runner every 5 s into a JSONL file (usage: sampler.sh <file>): {t, cpu (percent busy over all cores), memUsedMb, memTotalMb, load1}.
out="$1"
read -r _ u n s i w q sq st _ < /proc/stat
prev_total=$((u + n + s + i + w + q + sq + st))
prev_idle=$((i + w))
while sleep 5; do
  read -r _ u n s i w q sq st _ < /proc/stat
  total=$((u + n + s + i + w + q + sq + st))
  idle=$((i + w))
  dt=$((total - prev_total))
  di=$((idle - prev_idle))
  prev_total=$total
  prev_idle=$idle
  cpu=0
  [ "$dt" -gt 0 ] && cpu=$((100 * (dt - di) / dt))
  mem_total=$(awk '/^MemTotal/ { print int($2 / 1024) }' /proc/meminfo)
  mem_available=$(awk '/^MemAvailable/ { print int($2 / 1024) }' /proc/meminfo)
  load1=$(cut -d' ' -f1 /proc/loadavg)
  printf '{"t":%s,"cpu":%s,"memUsedMb":%s,"memTotalMb":%s,"load1":%s}\n' "$(date +%s)" "$cpu" "$((mem_total - mem_available))" "$mem_total" "$load1" >> "$out"
done
