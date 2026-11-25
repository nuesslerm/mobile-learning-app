"""Run alternating wf/Maestro Tamil journeys. Both clocks exclude preparation and export."""
import argparse
import csv
import json
import os
from pathlib import Path
import secrets
import shutil
import signal
import subprocess
import sys
import time
import urllib.request

from android import AndroidDevice

HERE = Path(__file__).resolve().parent


def memory_rows(root_pid, registry):
    roots = {root_pid}
    if registry.is_file():
        try:
            for slot in json.loads(registry.read_text())["slots"].values():
                daemon = slot.get("browserMcp", {}).get("pid")
                if daemon:
                    roots.add(daemon)
        except (ValueError, KeyError, FileNotFoundError):
            pass  # A daemon can register or unregister during the sample.
    children = {}
    for row in subprocess.check_output(["ps", "-axo", "pid=,ppid=,rss=,comm="], text=True).splitlines():
        fields = row.split(None, 3)
        if len(fields) == 4:
            pid, parent, rss = map(int, fields[:3])
            children[pid] = parent, rss, Path(fields[3]).name
    while True:
        more = {pid for pid, (parent, _, _) in children.items() if parent in roots}
        if more <= roots:
            return [(pid, *children[pid][1:]) for pid in sorted(roots & children.keys())]
        roots.update(more)


def export_maestro_trace(endpoint, directory, started, finished, passed):
    trace = secrets.token_hex(16)
    def span(name, begin, end, success):
        return dict(traceId=trace, spanId=secrets.token_hex(8), name=name,
                    startTimeUnixNano=str(begin), endTimeUnixNano=str(end),
                    status={"code": 1 if success else 2})
    root = span("maestro.process", started, finished, passed)
    spans = [root]
    for source in sorted(directory.rglob("commands-*.json")):
        for command in json.loads(source.read_text()):
            timing = command["metadata"]
            if "timestamp" not in timing or "duration" not in timing:
                continue
            name = next(iter(command["command"]))
            begin = timing["timestamp"] * 1_000_000
            item = span("maestro." + name, begin, begin + timing["duration"] * 1_000_000,
                        timing["status"] == "COMPLETED")
            spans.append(dict(item, parentSpanId=root["spanId"]))
    service = {"key": "service.name", "value": {"stringValue": "maestro"}}
    packet = {"resourceSpans": [{"resource": {"attributes": [service]}, "scopeSpans": [
        {"scope": {"name": "wf.bench.drivers"}, "spans": spans}]}]}
    request = urllib.request.Request(endpoint, data=json.dumps(packet).encode(),
                                     headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=5) as response:
        response.read()


def timed(command, env, directory, registry):
    start_ns = time.time_ns()
    start = time.monotonic()
    peak = None
    timed_out = False
    with (directory / "process.log").open("w") as log, (directory / "rss.csv").open("w") as stream:
        writer = csv.writer(stream)
        writer.writerow(["elapsed_s", "pid", "process", "rss_kib"])
        with subprocess.Popen(command, env=env, stdout=log, stderr=log, start_new_session=True) as proc:
            while proc.poll() is None:
                elapsed = time.monotonic() - start
                if elapsed > 120:
                    timed_out = True
                    os.killpg(proc.pid, signal.SIGKILL)
                    break
                rows = memory_rows(proc.pid, registry)
                if rows:
                    peak = max(peak or 0, sum(rss for _, rss, _ in rows) / 1024)
                writer.writerows((round(elapsed, 3), pid, name, rss) for pid, rss, name in rows)
                time.sleep(.15)
            status = proc.wait()
    result = dict(exit=status, timedOut=timed_out, totalS=time.monotonic()-start, peakHostMiB=peak)
    finished_ns = time.time_ns()
    if command[0] == "maestro" and env.get("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"):
        export_maestro_trace(env["OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"], directory, start_ns, finished_ns, status == 0)
    if status == 0:
        def marker(name):
            return next(directory.rglob(name + ".png")).stat().st_mtime_ns
        result["startupS"] = (marker("start") - start_ns) / 1e9
        result["journeyS"] = (marker("finish") - marker("start")) / 1e9
    return result


def run(args):
    args.out.mkdir(parents=True, exist_ok=False)
    out = args.out.resolve()
    native = args.platform != "browser"
    spec = HERE / ("library.yaml" if native else "library-web.yaml")
    android = AndroidDevice(args.device, args.app_id) if args.platform == "android" else None
    samples = []
    for pair in range(1, args.pairs + 1):
        order = [args.first, "maestro" if args.first == "wf" else "wf"]
        for driver in order if pair % 2 else reversed(order):
            folder = out / f"{pair}-{driver}"
            folder.mkdir()
            registry = folder / "registry"
            env = dict(os.environ, WF_REGISTRY_DIR=str(registry), WF_USAGE="off", MAESTRO_CLI_NO_ANALYTICS="1")
            device = ["--device", args.device] if native else []
            wf = [args.wf, args.platform, "test"]
            if native:
                with (folder / "prepare.log").open("w") as log:
                    try:
                        setup = subprocess.run([*wf, HERE / "prepare.lua", *device, "--no-record", "--env", "url="+args.url,
                                            "--test-output-dir", folder / "prepare"], env=env, stdout=log, stderr=log, timeout=90)
                    except subprocess.TimeoutExpired:
                        setup = subprocess.CompletedProcess(wf, 124)
                if setup.returncode:
                    samples.append(dict(pair=pair, driver=driver, stage="prepare", exit=setup.returncode))
                    (out / "samples.json").write_text(json.dumps(samples, indent=2)+"\n")
                    continue
            if driver == "wf":
                command = [*wf, HERE / "library.lua", *device, "--env", "out="+str(folder), "--test-output-dir", folder]
                if not native:
                    command += ["--env", "url="+args.url]
            else:
                settings = ["--device", args.device] if native else ["--platform", "web"]
                command = ["maestro", *settings, "test", spec, "--test-output-dir", folder, "-e", "APP_ID="+args.app_id]
                if android:
                    command += ["--debug-output", folder / "debug"]
                if not native:
                    command += ["--headless", "--screen-size", "390x844", "-e", "URL="+args.url]
            try:
                if android and driver == "maestro":
                    android.release_wf()
                def run_driver():
                    return timed(command, env, folder, registry / "registry.json")
                result = android.measure(run_driver, folder) if android else run_driver()
            except (OSError, RuntimeError, ValueError, StopIteration, subprocess.SubprocessError) as error:
                result = dict(exit=1, error=str(error) or "missing screenshot marker")
            if registry.exists() and not native:
                try:
                    subprocess.run([args.wf, "browser", "stop"], env=env, capture_output=True, timeout=30, check=True)
                except (OSError, subprocess.SubprocessError) as error:
                    result["cleanupError"] = str(error)
                    result["exit"] = result["exit"] or 1
            samples.append(dict(pair=pair, driver=driver, stage="journey", **result))
            (out / "samples.json").write_text(json.dumps(samples, indent=2)+"\n")
    return dict(ok=all(s["exit"] == 0 for s in samples), platform=args.platform, samples=samples)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--wf", required=True)
    parser.add_argument("--platform", required=True, choices=["ios", "android", "browser"])
    parser.add_argument("--device")
    parser.add_argument("--app-id", default="")
    parser.add_argument("--url", required=True)
    parser.add_argument("--pairs", type=int, default=5)
    parser.add_argument("--first", choices=["wf", "maestro"], default="wf")
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    try:
        if args.pairs < 1 or (args.platform != "browser" and not (args.device and args.app_id)):
            raise ValueError("expected positive pairs and an explicit device and app ID on native platforms")
        if Path(args.wf).stem != "wf":
            args.wf = shutil.which("wf") or str(Path(args.wf).with_name("wf"))
        result = run(args)
    except (OSError, RuntimeError, ValueError, StopIteration, subprocess.SubprocessError) as error:
        result = dict(ok=False, error=str(error) or "missing screenshot marker")
    print(json.dumps(result))
    sys.exit(0 if result["ok"] else 1)
