#!/usr/bin/env python3
"""n=5 rounds of the capture listener's cold start, idle RSS and refusal latency, implementations interleaved per round.

usage: capture-listener-bench.py OUT.json label=cwd::command [label=cwd::command ...]
Per run: spawn the listener with a complete configuration (nothing is triggered), wait until the port accepts (start-up ms), wait 1 s,
read VmRSS from /proc (idle RSS), then send 200 `POST /trigger` with a wrong secret and take the median latency of the 401 (hot route).
A round's value is the median of its RUNS runs; the result is the median of the round medians.
"""
import json, os, socket, statistics, subprocess, sys, tempfile, time, urllib.request, urllib.error

RUNS, ROUNDS = 5, 5
out_path, specs = sys.argv[1], [a.split("=", 1) for a in sys.argv[2:]]


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def rss_kb(pid):
    for line in open(f"/proc/{pid}/status"):
        if line.startswith("VmRSS:"):
            return int(line.split()[1])
    return 0


def once(cwd, command):
    port = free_port()
    d = tempfile.mkdtemp(prefix="capture-bench-")
    env = {"PATH": os.environ["PATH"], "HOME": d, "WF_BIN": "/bin/true", "CAPTURE_LISTENER_PORT": str(port),
           "LEETCODE_CAPTURE_TRIGGER_SECRET": "trigger-secret", "LEETCODE_BACKEND_BASE_URL": "http://127.0.0.1:9",
           "LEETCODE_BACKEND_ADMIN_SECRET": "admin-secret", "TELEGRAM_NOTIFY_BASE_URL": "http://127.0.0.1:9",
           "TELEGRAM_NOTIFY_SECRET": "notify-secret", "CAPTURE_MODE": "local"}
    t0 = time.perf_counter()
    proc = subprocess.Popen(command.split(), cwd=cwd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    while True:
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.05).close()
            break
        except OSError:
            if proc.poll() is not None:
                raise SystemExit(f"listener exited early: {command}")
            time.sleep(0.002)
    start_ms = (time.perf_counter() - t0) * 1000
    time.sleep(1.0)
    rss = rss_kb(proc.pid)
    lat = []
    for _ in range(200):
        req = urllib.request.Request(f"http://127.0.0.1:{port}/trigger", method="POST", headers={"x-capture-trigger-secret": "nope"})
        t = time.perf_counter()
        try:
            urllib.request.urlopen(req, timeout=5)
        except urllib.error.HTTPError as e:
            if e.code not in (401, 429):
                raise SystemExit(f"unexpected status {e.code}")
        lat.append((time.perf_counter() - t) * 1000)
        if len(lat) % 25 == 0:
            time.sleep(0.5)  # stay under the 30 per minute limit's refill: only the 401s are timed
    proc.kill()
    proc.wait()
    return start_ms, rss, statistics.median(lat[:25])


results = {label: [] for label, _ in specs}
for r in range(ROUNDS):
    order = specs if r % 2 == 0 else list(reversed(specs))
    for label, spec in order:
        cwd, command = spec.split("::", 1)
        runs = [once(cwd, command) for _ in range(RUNS)]
        results[label].append({"start_ms": round(statistics.median(x[0] for x in runs), 1), "idle_rss_kb": int(statistics.median(x[1] for x in runs)),
                               "refusal_p50_ms": round(statistics.median(x[2] for x in runs), 3)})
summary = {}
for label, rounds in results.items():
    summary[label] = {k: statistics.median(x[k] for x in rounds) for k in ("start_ms", "idle_rss_kb", "refusal_p50_ms")} | {"rounds": rounds}
json.dump({"method": __doc__, "host": os.uname().machine, "implementations": summary}, open(out_path, "w"), indent=2)
print(json.dumps({k: {x: y for x, y in v.items() if x != "rounds"} for k, v in summary.items()}, indent=2))
