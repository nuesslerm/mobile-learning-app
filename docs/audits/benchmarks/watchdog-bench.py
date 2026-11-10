#!/usr/bin/env python3
"""n=5 rounds of the watchdog's cold start and idle RSS, implementations interleaved per round.

usage: watchdog-bench.py OUT.json label=command [label=command ...]
Per run: spawn the daemon beside a stub `docker` that reports no containers (HOST_LABEL set, no ingest), wait until the stub has been
called (start-up ms: process start to the first `docker ps`), wait 1 s, read VmRSS from /proc (idle RSS). A round's value is the
median of its RUNS runs; the result is the median of the round medians.
"""
import json, os, statistics, subprocess, sys, tempfile, time

RUNS, ROUNDS = 5, 5
out_path, specs = sys.argv[1], [a.split("=", 1) for a in sys.argv[2:]]


def rss_kb(pid):
    for line in open(f"/proc/{pid}/status"):
        if line.startswith("VmRSS:"):
            return int(line.split()[1])
    return 0


def once(command):
    d = tempfile.mkdtemp(prefix="watchdog-bench-")
    os.mkdir(f"{d}/bin")
    open(f"{d}/bin/docker", "w").write(f'#!/bin/sh\necho x >> "{d}/calls"\nexit 0\n')
    os.chmod(f"{d}/bin/docker", 0o755)
    env = {"PATH": f"{d}/bin:{os.environ['PATH']}", "HOME": d, "HOST_LABEL": "bench", "WATCHDOG_INTERVAL_MS": "30000"}
    t0 = time.perf_counter()
    proc = subprocess.Popen(command.split(), env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    while not os.path.exists(f"{d}/calls"):
        time.sleep(0.002)
    start_ms = (time.perf_counter() - t0) * 1000
    time.sleep(1)
    rss = rss_kb(proc.pid)
    proc.terminate()
    proc.wait()
    return start_ms, rss


results = {label: [] for label, _ in specs}
for r in range(ROUNDS):
    order = specs if r % 2 == 0 else list(reversed(specs))
    for label, command in order:
        runs = [once(command) for _ in range(RUNS)]
        results[label].append({"start_ms": round(statistics.median(x[0] for x in runs), 3), "idle_rss_kb": int(statistics.median(x[1] for x in runs))})

summary = {}
for label, rounds in results.items():
    summary[label] = {k: round(statistics.median(x[k] for x in rounds), 3) for k in rounds[0]}
    summary[label]["rounds"] = rounds
json.dump({"method": __doc__.strip().split("\n")[0] + " See the script header.", "host": os.uname().machine, "implementations": summary}, open(out_path, "w"), indent=2)
print(json.dumps({k: {x: y for x, y in v.items() if x != "rounds"} for k, v in summary.items()}, indent=2))
