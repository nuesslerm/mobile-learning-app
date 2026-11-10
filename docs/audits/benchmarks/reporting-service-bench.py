#!/usr/bin/env python3
"""n=5 rounds of the reporting service's cold start, idle RSS and two hot routes, implementations interleaved per round.

usage: reporting-service-bench.py OUT.json label=cwd::command [label=cwd::command ...]
Per run: spawn the service on a database seeded with 20,000 raw samples (nothing is sampled: every interval is an hour), wait until
/health answers (start-up ms), wait 1 s, read VmRSS from /proc (idle RSS), then take the median latency of 200 GETs of /health and of
50 GETs of /api/metrics/series?window=24h (reads the seeded rows). A round's value is the median of its RUNS runs; the result is the
median of the round medians.
"""
import json, os, socket, sqlite3, statistics, subprocess, sys, tempfile, time, urllib.request

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


def seed(path):
    db = sqlite3.connect(path)
    db.execute("CREATE TABLE samples (ts INTEGER NOT NULL, scope TEXT NOT NULL, name TEXT NOT NULL, metric TEXT NOT NULL, value REAL NOT NULL, PRIMARY KEY (ts, scope, name, metric))")
    db.execute("CREATE TABLE rollups (granularity TEXT NOT NULL, bucket_ts INTEGER NOT NULL, scope TEXT NOT NULL, name TEXT NOT NULL, metric TEXT NOT NULL, value REAL NOT NULL, PRIMARY KEY (granularity, bucket_ts, scope, name, metric))")
    now = int(time.time() * 1000)
    db.executemany("INSERT INTO samples VALUES (?, 'container', ?, 'cpu', ?)", [(now - i * 4000, f"c{i % 10}", float(i % 97)) for i in range(20000)])
    db.commit()
    db.close()


def get(url):
    t = time.perf_counter()
    urllib.request.urlopen(url).read()
    return (time.perf_counter() - t) * 1000


def once(cwd, command):
    port = free_port()
    d = tempfile.mkdtemp(prefix="reporting-bench-")
    seed(os.path.join(d, "r.db"))
    env = {"PATH": os.environ["PATH"], "HOME": d, "PORT": str(port), "REPORTING_DB_PATH": os.path.join(d, "r.db"),
           "REPORTING_REPO_ROOT": d, "DOCKER_SOCKET": os.path.join(d, "none.sock"),
           **{f"REPORTING_{k}_INTERVAL_MS": "3600000" for k in ("CONTAINERS", "HOST", "STORAGE", "PRUNE")}}
    t0 = time.perf_counter()
    proc = subprocess.Popen(command.split(), cwd=cwd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    while True:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/health").read()
            break
        except Exception:
            time.sleep(0.002)
    start_ms = (time.perf_counter() - t0) * 1000
    time.sleep(1)
    rss = rss_kb(proc.pid)
    health = statistics.median(get(f"http://127.0.0.1:{port}/health") for _ in range(200))
    series = statistics.median(get(f"http://127.0.0.1:{port}/api/metrics/series?window=24h") for _ in range(50))
    proc.terminate()
    proc.wait()
    return start_ms, rss, health, series


results = {label: [] for label, _ in specs}
for r in range(ROUNDS):
    order = specs if r % 2 == 0 else list(reversed(specs))
    for label, spec in order:
        cwd, command = spec.split("::", 1)
        runs = [once(cwd, command) for _ in range(RUNS)]
        results[label].append({k: round(statistics.median(x[i] for x in runs), 3) for i, k in enumerate(["start_ms", "idle_rss_kb", "health_p50_ms", "series_24h_p50_ms"])})

summary = {}
for label, rounds in results.items():
    summary[label] = {k: round(statistics.median(x[k] for x in rounds), 3) for k in rounds[0]}
    summary[label]["rounds"] = rounds
doc = {"method": __doc__.strip().split("\n")[0] + " See the script header.", "host": os.uname().machine, "implementations": summary}
json.dump(doc, open(out_path, "w"), indent=2)
print(json.dumps({k: {x: y for x, y in v.items() if x != "rounds"} for k, v in summary.items()}, indent=2))
