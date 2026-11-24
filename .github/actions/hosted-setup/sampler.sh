#!/usr/bin/env bash
# Samples this runner every 5 s into a JSONL file (usage: sampler.sh <file>). Always: {t, cpu (percent busy over all cores), memUsedMb, memTotalMb, load1}.
# Also: iowaitPct, swapUsedMb, procsRunning, readBytes/writeBytes (block I/O since the last sample), psiCpu/psiMem/psiIo (kernel pressure avg10, null when
# the kernel has none); the first sample alone carries cores and runner (RUNNER_NAME, the join key to CI_JOBS.RUNNER).
out="$1"
cores=$(nproc 2> /dev/null || echo 0)
runner="${RUNNER_NAME:-}"
read -r _ u n s i w q sq st _ < /proc/stat
prev_total=$((u + n + s + i + w + q + sq + st))
prev_idle=$((i + w))
prev_wait=$w
vm() { awk -v k="$1" '$1 == k { print $2 }' /proc/vmstat 2> /dev/null; }
prev_in=$(vm pgpgin)
prev_out=$(vm pgpgout)
psi() { awk '$1 == "some" { sub("avg10=", "", $2); print $2 }' "/proc/pressure/$1" 2> /dev/null; }
first=1
while sleep 5; do
  read -r _ u n s i w q sq st _ < /proc/stat
  total=$((u + n + s + i + w + q + sq + st))
  idle=$((i + w))
  dt=$((total - prev_total))
  di=$((idle - prev_idle))
  dw=$((w - prev_wait))
  prev_total=$total
  prev_idle=$idle
  prev_wait=$w
  cpu=0
  iowait=0
  if [ "$dt" -gt 0 ]; then
    cpu=$((100 * (dt - di) / dt))
    iowait=$((100 * dw / dt))
  fi
  mem_total=$(awk '/^MemTotal/ { print int($2 / 1024) }' /proc/meminfo)
  mem_available=$(awk '/^MemAvailable/ { print int($2 / 1024) }' /proc/meminfo)
  swap=$(awk '/^SwapTotal/ { t = $2 } /^SwapFree/ { f = $2 } END { print int((t - f) / 1024) }' /proc/meminfo)
  load1=$(cut -d' ' -f1 /proc/loadavg)
  running=$(awk '$1 == "procs_running" { print $2 }' /proc/stat)
  cur_in=$(vm pgpgin)
  cur_out=$(vm pgpgout)
  read_bytes=$(((${cur_in:-0} - ${prev_in:-0}) * 1024))
  write_bytes=$(((${cur_out:-0} - ${prev_out:-0}) * 1024))
  prev_in=$cur_in
  prev_out=$cur_out
  p_cpu=$(psi cpu)
  p_mem=$(psi memory)
  p_io=$(psi io)
  once=""
  if [ "$first" = 1 ]; then
    once=$(printf ',"cores":%s,"runner":"%s"' "$cores" "$runner")
    first=0
  fi
  printf '{"t":%s,"cpu":%s,"memUsedMb":%s,"memTotalMb":%s,"load1":%s,"iowaitPct":%s,"swapUsedMb":%s,"procsRunning":%s,"readBytes":%s,"writeBytes":%s,"psiCpu":%s,"psiMem":%s,"psiIo":%s%s}\n' \
    "$(date +%s)" "$cpu" "$((mem_total - mem_available))" "$mem_total" "$load1" "$iowait" "${swap:-0}" "${running:-0}" "$read_bytes" "$write_bytes" "${p_cpu:-null}" "${p_mem:-null}" "${p_io:-null}" "$once" >> "$out"
done
