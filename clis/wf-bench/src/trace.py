"""OTLP/HTTP JSON capture for a benchmark subprocess."""

import argparse
import json
import math
import os
import platform
import re
import sqlite3
import subprocess
import sys
import threading
import time
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

SCHEMA = """
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY, revision TEXT, profile TEXT, suite TEXT,
    started TEXT DEFAULT CURRENT_TIMESTAMP, passed INTEGER
);
CREATE TABLE IF NOT EXISTS spans (
    run_id TEXT, trace_id TEXT, span_id TEXT, parent_id TEXT, name TEXT,
    start_ns INTEGER NOT NULL, end_ns INTEGER NOT NULL,
    PRIMARY KEY(run_id, trace_id, span_id)
);
CREATE INDEX IF NOT EXISTS spans_operation ON spans(run_id, name);
CREATE TABLE IF NOT EXISTS metrics (
    run_id TEXT, name TEXT, samples INTEGER, p95_ms REAL, budget_ms REAL,
    PRIMARY KEY(run_id, name)
);
"""


def receiver(database, run):
    class Receiver(BaseHTTPRequestHandler):
        timeout = 5

        def log_message(self, *args):
            pass

        def do_POST(self):
            if self.path != "/v1/traces":
                self.send_error(404)
                return
            try:
                length = int(self.headers["Content-Length"])
                if not 0 < length <= 8 * 1024 * 1024:
                    raise ValueError("invalid batch size")
                batch = json.loads(self.rfile.read(length))
                rows = []
                for resource in batch["resourceSpans"]:
                    for scope in resource["scopeSpans"]:
                        for span in scope["spans"]:
                            start = int(span["startTimeUnixNano"])
                            end = int(span["endTimeUnixNano"])
                            if (
                                end < start
                                or not re.fullmatch("[0-9a-fA-F]{32}", span["traceId"])
                                or not re.fullmatch("[0-9a-fA-F]{16}", span["spanId"])
                            ):
                                raise ValueError("invalid span")
                            rows.append(
                                (
                                    run,
                                    span["traceId"],
                                    span["spanId"],
                                    span.get("parentSpanId"),
                                    span["name"],
                                    start,
                                    end,
                                )
                            )
                with sqlite3.connect(database, timeout=30) as db:
                    db.executemany("INSERT OR IGNORE INTO spans VALUES (?,?,?,?,?,?,?)", rows)
            except (ValueError, KeyError, TypeError, OverflowError):
                self.send_error(400)
                return
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b"{}")

    return Receiver


def publish(metrics, suite, profile):
    host = os.environ["BETTERSTACK_METRICS_HOST"]
    if not re.fullmatch(r"[a-z0-9.-]+", host):
        raise ValueError("invalid Better Stack host")
    stamp = str(time.time_ns())

    def attrs(pairs):
        return [{"key": k, "value": {"stringValue": v}} for k, v in pairs.items()]

    points = [
        {
            "timeUnixNano": stamp,
            "asDouble": m["p95_ms"],
            "attributes": attrs({"suite": suite, "operation": m["name"], "profile": profile}),
        }
        for m in metrics
        if m["p95_ms"] is not None
    ]
    gauge = {"name": "benchmark.duration.p95", "unit": "ms", "gauge": {"dataPoints": points}}
    body = {
        "resourceMetrics": [
            {
                "resource": {"attributes": attrs({"service.name": "wf-bench"})},
                "scopeMetrics": [{"scope": {"name": "wf.bench"}, "metrics": [gauge]}],
            }
        ]
    }
    headers = {
        "Content-Type": "application/json",
        "Authorization": "Bearer " + os.environ["BETTERSTACK_METRICS_TOKEN"],
    }
    req = urllib.request.Request(f"https://{host}/v1/metrics", json.dumps(body).encode(), headers)
    with urllib.request.urlopen(req, timeout=6) as response:
        result = json.loads(response.read() or b"{}")
        if int(result.get("partialSuccess", {}).get("rejectedDataPoints", 0)):
            raise ValueError("Better Stack rejected data points")


def main():
    parser = argparse.ArgumentParser(prog="wf bench trace")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--budgets", type=Path)
    mode.add_argument("--merge", type=Path, nargs="+", help="merge downloaded performance databases")
    parser.add_argument(
        "--db", type=Path, default=Path(os.environ["WF_BENCH_ROOT"]) / "clis/app-bench/history.sqlite"
    )
    parser.add_argument(
        "--profile", default=f"local-{platform.system()}-{platform.machine()}-{platform.node()}"
    )
    parser.add_argument("--send", action="store_true", help="export summary gauges to Better Stack")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if args.merge:
        args.db.parent.mkdir(parents=True, exist_ok=True)
        with sqlite3.connect(args.db, timeout=30) as db:
            db.executescript(SCHEMA)
            for source in args.merge:
                if not source.is_file() or source.resolve() == args.db.resolve():
                    parser.error(f"expected another existing SQLite database: {source}")
                db.execute("ATTACH DATABASE ? AS imported", (str(source),))
                with db:
                    for table in ("runs", "spans", "metrics"):
                        db.execute(f"INSERT OR IGNORE INTO main.{table} SELECT * FROM imported.{table}")
                db.execute("DETACH DATABASE imported")
            print(json.dumps(dict(ok=True, runs=db.execute("SELECT count(*) FROM runs").fetchone()[0])))
        return 0
    if not command:
        parser.error("supply the workload command after --")
    spec = json.loads(args.budgets.read_text())
    suite = spec["suite"]
    budgets = spec["p95_ms"]
    minimum = spec["min_samples"]
    if (
        not budgets
        or not isinstance(minimum, int)
        or minimum < 1
        or any(not math.isfinite(v) or v <= 0 for v in budgets.values())
    ):
        parser.error("expected positive budgets and min_samples")
    args.db.parent.mkdir(parents=True, exist_ok=True)
    db = sqlite3.connect(args.db, timeout=30)
    db.executescript(SCHEMA)
    git = ["git", "-C", os.environ["WF_BENCH_ROOT"]]
    revision = subprocess.check_output([*git, "rev-parse", "HEAD"], text=True).strip()
    if subprocess.check_output(
        [
            *git,
            "status",
            "--porcelain",
            "--untracked-files=normal",
            "--",
            ".",
            ":(exclude)clis/app-bench/history.sqlite",
        ]
    ):
        revision += "-dirty"
    run = uuid.uuid4().hex
    with db:
        db.execute(
            "INSERT INTO runs(id,revision,profile,suite) VALUES(?,?,?,?)",
            (run, revision, args.profile, suite),
        )
    with HTTPServer(("127.0.0.1", 0), receiver(args.db, run)) as server:
        endpoint = f"http://127.0.0.1:{server.server_port}"
        env = dict(
            os.environ,
            OTEL_EXPORTER_OTLP_ENDPOINT=endpoint,
            OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=endpoint + "/v1/traces",
            OTEL_EXPORTER_OTLP_PROTOCOL="http/json",
            OTEL_EXPORTER_OTLP_TRACES_PROTOCOL="http/json",
        )
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            status = subprocess.run(command, env=env, stdout=sys.stderr).returncode
        except OSError as error:
            print(f"cannot start benchmark: {error}", file=sys.stderr)
            status = 1
        finally:
            server.shutdown()
            worker.join()
    baseline = db.execute(
        """SELECT id FROM runs WHERE suite=? AND profile=? AND passed=1
                             ORDER BY started, id LIMIT 1""",
        (suite, args.profile),
    ).fetchone()
    passed = status == 0
    metrics = []
    with db:
        for name, budget in budgets.items():
            times = db.execute(
                "SELECT (end_ns-start_ns)/1000000.0 FROM spans WHERE run_id=? AND name=? ORDER BY 1",
                (run, name),
            ).fetchall()
            count = len(times)
            p95 = times[math.ceil(0.95 * count) - 1][0] if count else None
            reference = None
            if baseline:
                row = db.execute(
                    "SELECT p95_ms FROM metrics WHERE run_id=? AND name=?", (baseline[0], name)
                ).fetchone()
                reference = row[0] if row else None
            if reference is not None:
                budget = min(budget, reference + max(reference * 0.5, 0.10))
            passed &= count >= minimum and p95 <= budget
            db.execute("INSERT INTO metrics VALUES(?,?,?,?,?)", (run, name, count, p95, budget))
            metrics.append(
                dict(name=name, samples=count, p95_ms=p95, baseline_ms=reference, budget_ms=budget)
            )
        db.execute("UPDATE runs SET passed=? WHERE id=?", (passed, run))
    db.close()
    export = "disabled"
    if args.send:
        try:
            publish(metrics, suite, args.profile)
            export = "sent"
        except (OSError, KeyError, ValueError) as error:
            export = f"failed ({type(error).__name__})"
    print(json.dumps(dict(ok=passed, run=run, database=str(args.db), metrics=metrics, export=export)))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
