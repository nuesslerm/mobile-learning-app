#!/usr/bin/env python3
"""n=5 rounds of `wf log emit` against a local HTTPS OTLP ingest, binaries interleaved per round.

usage: bench.py OUT.json label=/path/to/wf [label=/path/to/wf ...]
Each round runs RUNS emits per binary; the round's value is the median wall time and the median peak RSS of its runs.
"""
import http.server, json, os, shutil, ssl, statistics, subprocess, sys, tempfile, threading, time

RUNS, ROUNDS = 10, 5
out_path, specs = sys.argv[1], [a.split("=", 1) for a in sys.argv[2:]]

d = tempfile.mkdtemp(prefix="wf-otlp-bench-")
at = lambda n: os.path.join(d, n)
sh = lambda *a: subprocess.run(a, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
sh("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-keyout", at("ca-key.pem"), "-out", at("ca.pem"),
   "-subj", "/CN=wf-bench-ca", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign")
sh("openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", at("key.pem"), "-out", at("leaf.csr"), "-subj", "/CN=127.0.0.1")
open(at("leaf.ext"), "w").write("subjectAltName=IP:127.0.0.1\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n")
sh("openssl", "x509", "-req", "-in", at("leaf.csr"), "-CA", at("ca.pem"), "-CAkey", at("ca-key.pem"), "-CAcreateserial",
   "-out", at("cert.pem"), "-days", "2", "-extfile", at("leaf.ext"))

hits = {"n": 0, "bad": 0}


class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("content-length", 0)))
        hits["n"] += 1
        if self.path != "/v1/logs" or self.headers.get("authorization") != "Bearer bench-token" or b"resourceLogs" not in body:
            hits["bad"] += 1
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"{}")

    def log_message(self, *a):
        pass


srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), H)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(at("cert.pem"), at("key.pem"))
srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
threading.Thread(target=srv.serve_forever, daemon=True).start()
port = srv.server_address[1]

env = {"PATH": os.environ.get("PATH", ""), "HOME": d, "BETTERSTACK_OTLP_TOKEN": "bench-token",
       "BETTERSTACK_OTLP_HOST": f"127.0.0.1:{port}", "SSL_CERT_FILE": at("ca.pem")}
args = ["log", "emit", "--service", "bench", "--message", "bench.event", "--attr", "a=b", "--attr", "k=v"]


def once(binary):
    t0 = time.perf_counter()
    pid = subprocess.Popen([binary, *args], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, cwd=d)
    out = pid.stdout.read()
    _, status, ru = os.wait4(pid.pid, 0)
    ms = (time.perf_counter() - t0) * 1000
    pid.returncode = 0
    ok = status == 0 and out.startswith(b'{"sent":true}')
    return ms, ru.ru_maxrss, ok


results = {label: {"rounds": [], "failures": 0, "size_bytes": os.path.getsize(path)} for label, path in specs}
for label, path in specs:  # warm the page cache, not counted
    once(path)
for r in range(ROUNDS):
    order = specs if r % 2 == 0 else list(reversed(specs))
    for label, path in order:
        ms, rss = [], []
        for _ in range(RUNS):
            m, k, ok = once(path)
            ms.append(m)
            rss.append(k)
            results[label]["failures"] += 0 if ok else 1
        results[label]["rounds"].append({"median_ms": round(statistics.median(ms), 2), "median_peak_rss_kb": int(statistics.median(rss))})

summary = {}
for label, v in results.items():
    medians = [x["median_ms"] for x in v["rounds"]]
    rss = [x["median_peak_rss_kb"] for x in v["rounds"]]
    summary[label] = {"size_bytes": v["size_bytes"], "emit_ms_median": round(statistics.median(medians), 2), "emit_ms_min": min(medians),
                      "emit_ms_max": max(medians), "peak_rss_kb_median": int(statistics.median(rss)), "failures": v["failures"], "rounds": v["rounds"]}
doc = {"method": f"{ROUNDS} rounds x {RUNS} runs of `wf log emit --service bench --message bench.event --attr a=b --attr k=v` against a local HTTPS OTLP ingest; binaries interleaved per round; value = median of the round medians",
       "server": {"requests": hits["n"], "malformed": hits["bad"]}, "host": os.uname().machine, "binaries": summary}
json.dump(doc, open(out_path, "w"), indent=2)
print(json.dumps({k: {x: y for x, y in v.items() if x != "rounds"} for k, v in summary.items()}, indent=2))
print("server requests", hits)
shutil.rmtree(d, ignore_errors=True)
