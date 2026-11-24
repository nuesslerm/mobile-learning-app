#!/usr/bin/env python3
"""The bundle of one real-backends run: reads the record each leg uploaded (real-backend-<leg>/real-backend-<leg>.json under --dir), lists the three backends with
the agent methods each answered and the turn's final state, writes real-backends.json beside them (--out) and a markdown table for the step summary (--summary),
and exits 1 when a backend's record is missing or says it did not pass. A missing record means its job never reached its result (the job's own log says why).

Usage: aggregate.py --dir <downloaded artifacts> --out <file> [--summary <file>]
"""
import json, os, sys

LEGS = ("claude", "codex", "opencode")


def record_of(directory, leg):
    path = os.path.join(directory, f"real-backend-{leg}", f"real-backend-{leg}.json")
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def build(directory):
    backends, missing, failed = [], [], []
    for leg in LEGS:
        record = record_of(directory, leg)
        if record is None:
            missing.append(leg)
            backends.append({"leg": leg, "ok": False, "missing": True})
            continue
        row = {"leg": leg, "ok": record.get("ok") is True, "methods": [m.get("method") for m in record.get("methods", []) if m.get("ok")], "final_state": record.get("final_state")}
        if record.get("missing_key"):
            row["missing_key"] = record["missing_key"]
        if not row["ok"]:
            failed.append(leg)
        backends.append(row)
    return {"schema": "real-backends/1", "ok": not missing and not failed, "backends": backends, "missing": missing, "failed": failed}


def table(bundle):
    lines = ["### Real backends", "", "| backend | result | methods answered | final state |", "| --- | --- | --- | --- |"]
    for b in bundle["backends"]:
        result = "missing" if b.get("missing") else ("pass" if b["ok"] else "fail" + (f" (no value for {b['missing_key']})" if b.get("missing_key") else ""))
        lines.append(f"| {b['leg']} | {result} | {', '.join(b.get('methods') or []) or '-'} | {b.get('final_state') or '-'} |")
    return "\n".join(lines) + "\n"


def main():
    arg = lambda name, default=None: sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default
    directory, out, summary = arg("--dir"), arg("--out"), arg("--summary")
    if not directory or not out:
        raise SystemExit("usage: aggregate.py --dir <artifacts> --out <file> [--summary <file>]")
    bundle = build(directory)
    with open(out, "w") as f:
        f.write(json.dumps(bundle, indent=2, sort_keys=True))
    if summary:
        with open(summary, "a") as f:
            f.write(table(bundle))
    print(json.dumps(bundle))
    sys.exit(0 if bundle["ok"] else 1)


if __name__ == "__main__":
    main()
