"""Repeat isolated API contract probes and compare matching outcomes on the same runner."""
import json
import statistics
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

SAMPLES = 5


def host_entries(root):
    for host in ("orca", "t3"):
        path = root / f"clis/{host}-serve/cassettes/{host}-api/inventory.json"
        for row in json.loads(path.read_text())["endpoints"]:
            kind, name = row["kind"], row["name"]
            endpoint = f"{host}.{kind}.{name}"
            operation = f"{host}.http.{row['method']} {row['path']}" if "path" in row and kind == "http" else endpoint
            external = kind == "http" and host == "orca" and "third-party" in row.get("skipped", "")
            yield endpoint, None if external else "catalog." + endpoint, operation, row.get("skipped", "") if external else ""


def exercise(host_command, root, env):
    commands = [
        host_command,
        ["cargo", "test", "--locked", "--manifest-path", str(root / "clis/Cargo.toml"), "-p", "rca-exec", "--lib", "performance_catalog", "--", "--ignored"],
        ["cargo", "test", "--locked", "--manifest-path", str(root / "backend/remote-chat-agent/Cargo.toml"),
         "--target-dir", env.get("CARGO_TARGET_DIR", str(root / "clis/target/rca-service-build")), "-p", "rca-service", "--test", "flow", "performance_catalog", "--", "--ignored"],
    ]
    entries = list(host_entries(root))
    with tempfile.TemporaryDirectory() as temp:
        env = dict(env, WF_PERFORMANCE_CATALOG_DIR=temp)
        status = 0
        for command in commands:
            for _ in range(SAMPLES):
                result = subprocess.run(command, env=env, stdout=sys.stderr).returncode
                if result:
                    status = result
                    break
        for family, operation in [("rca.rpc", "rca.server"), ("rca.executor", "rca.executor"), ("rca.http", "rca.http")]:
            path = Path(temp) / (family + ".json")
            if not path.is_file():
                status = status or 1
                continue
            names = json.loads(path.read_text())
            if not names or len(set(names)) != len(names):
                raise ValueError(f"invalid {family} registry")
            for name in names:
                endpoint = family + "." + name
                probe = operation + "." + name if family == "rca.executor" else "catalog." + endpoint
                entries.append((endpoint, probe, operation + "." + name, ""))
    return status, entries


def check(db, run, baseline, entries):
    passed = True
    for endpoint, probe, operation, note in entries:
        samples = db.execute("""SELECT (end_ns-start_ns)/1000000.0, outcome FROM spans
            LEFT JOIN span_results USING(run_id, trace_id, span_id) WHERE run_id=? AND name=?""", (run, probe)).fetchall()
        outcomes = json.dumps(sorted(Counter(outcome or "unrecorded" for _, outcome in samples).items()))
        median = statistics.median(ms for ms, _ in samples) if samples else None
        reference = None
        fewer_successes = False
        if baseline and probe:
            row = db.execute("SELECT median_ms,outcomes FROM catalog WHERE run_id=? AND endpoint=?", (baseline[0], endpoint)).fetchone()
            if row:
                if outcomes == row[1]:
                    reference = row[0]
                else:
                    before, now = (dict(json.loads(value)).get("ok", 0) for value in (row[1], outcomes))
                    fewer_successes = now < before
        limit = reference + max(5., reference * .5) if reference is not None else None
        if probe and (len(samples) != SAMPLES or any(not outcome for _, outcome in samples)):
            verdict = "Incomplete"
        elif fewer_successes:
            verdict = "Fewer successful calls"
        elif limit is not None:
            verdict = "Within limit" if median <= limit else "Exceeded"
        else:
            verdict = "No comparable baseline" if probe else "External"
        if verdict in ("Exceeded", "Incomplete", "Fewer successful calls"):
            passed = False
            print(f"{endpoint}: {verdict}, samples={len(samples)}, median={median}, limit={limit}", file=sys.stderr)
        db.execute("INSERT INTO catalog VALUES(?,?,?,?,?,?,?,?,?,?,?)",
                   (run, endpoint, probe, operation, note, len(samples), outcomes, median, reference, limit, verdict))
    return passed
