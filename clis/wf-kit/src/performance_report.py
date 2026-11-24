"""Build an offline per-invocation report from the repository's OTLP history."""
import base64
import gzip
import json
import math
import sqlite3
from collections import defaultdict
from performance_catalog import host_entries


def report(database, destination, root):
    inventory = {endpoint: (probe, operation, note) for endpoint, probe, operation, note in host_entries(root)}
    with sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True) as db:
        runs = list(db.execute("SELECT id, started, profile, suite, revision, passed FROM runs ORDER BY started DESC, rowid DESC"))
        tables = {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        requirements = dict(db.execute("SELECT run_id,minimum FROM sample_requirements")) if "sample_requirements" in tables else {}
        for r in runs:
            if r[3] == "rca-hosts-file-chat-v2":
                requirements.setdefault(r[0], 30)
        notes = dict(db.execute("SELECT run_id,note FROM run_notes")) if "run_notes" in tables else {}
        checks = []
        if "catalog" in tables:
            checks = list(db.execute("""SELECT run_id,endpoint,samples,outcomes,median_ms,reference_ms,limit_ms,verdict
                                      FROM catalog ORDER BY endpoint,run_id"""))
            for endpoint, probe, operation, note in db.execute("SELECT DISTINCT endpoint,probe,operation,note FROM catalog"):
                inventory[endpoint] = (probe, operation, note)
        fields, joins = [], []
        for table, columns in [("span_results", ["outcome", "http_status"]), ("span_sources", ["service", "caller_file", "caller_line"])]:
            if table in tables:
                joins.append(f"LEFT JOIN {table} USING (run_id, trace_id, span_id)")
                fields.extend(columns)
            else:
                fields.extend("NULL" for _ in columns)
        rows = db.execute(f"""SELECT run_id,name,start_ns,end_ns-start_ns,trace_id,span_id,parent_id,{','.join(fields)}
                              FROM spans {' '.join(joins)} ORDER BY start_ns,span_id""")
        calls = []
        durations = defaultdict(list)
        for run, name, start, elapsed, trace, span, parent, outcome, status, service, file, line in rows:
            durations[run, name].append(elapsed / 1e6)
            calls.append([run, name, start // 1000, elapsed, outcome, status, service, trace, span, parent, file, line])
        metrics = {(r, n): (p, b) for r, n, p, b in db.execute("SELECT run_id,name,p95_ms,budget_ms FROM metrics")}
    operations = []
    for run, name in sorted(durations.keys() | metrics.keys()):
        times = sorted(durations.get((run, name), []))
        p95, budget = metrics.get((run, name), (None, None))
        if times:
            p95 = times[math.ceil(.95 * len(times)) - 1]
        operations.append([run, name, len(times), times[math.ceil(.5 * len(times)) - 1] if times else None, p95, budget])
    payload = dict(runs=runs, operations=operations, calls=calls, inventory=[[e, *i] for e, i in sorted(inventory.items())], checks=checks, requirements=requirements, notes=notes)
    packed = gzip.compress(json.dumps(payload, separators=(",", ":")).encode(), compresslevel=6, mtime=0)
    template = (root / "clis/wf-kit/src/performance_report.html").read_text()
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(template.replace("__DATA__", base64.b64encode(packed).decode()))
    return dict(report=str(destination), runs=len(runs), spans=len(calls), operations=len({o[1] for o in operations}), endpoints=len(inventory))
