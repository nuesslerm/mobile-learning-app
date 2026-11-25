"""Keep one XCTest accessibility and software-keyboard session on a selected simulator."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

RUNNER = "org.mobilelearning.NativeJourneyInputTests.xctrunner"


def state_file(udid):
    return Path(os.getenv("RUNNER_TEMP") or tempfile.gettempdir()) / f"wf-xcui-{udid}.json"


def call(folder, op, **fields):
    ident = uuid.uuid4().hex
    temp = folder / (ident + ".tmp")
    reply = folder / (ident + ".json")
    temp.write_text(json.dumps({"op": op, "id": ident, **fields}))
    temp.replace(folder / "request.json")
    deadline = time.monotonic() + 30
    while not reply.exists():
        if time.monotonic() >= deadline:
            raise RuntimeError(f"XCTest did not answer {op}")
        time.sleep(.015)
    message = json.loads(reply.read_text())
    reply.unlink()
    if "error" in message:
        raise RuntimeError(message["error"])
    return message.get("value")


def start(udid, runner):
    state = state_file(udid)
    if state.exists():
        previous = json.loads(state.read_text())
        try:
            os.kill(previous["pid"], 0)
        except ProcessLookupError:
            state.unlink()
        else:
            call(Path(previous["folder"]), "tree")
            return
    session = "wf-ui-" + uuid.uuid4().hex
    log = state.with_suffix(".log")
    with log.open("w") as output:
        proc = subprocess.Popen(["xcodebuild", "test-without-building", "-xctestrun", runner, "-destination", "id=" + udid,
                                 "-parallel-testing-enabled", "NO", "-test-timeouts-enabled", "NO"],
                                env={**os.environ, "TEST_RUNNER_WF_UI_SESSION": session},
                                stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        deadline = time.monotonic() + 150
        folder = None
        while not folder or not (folder / "ready").exists():
            if proc.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError("XCTest session did not start: " + log.read_text()[-2400:])
            found = subprocess.run(["xcrun", "simctl", "get_app_container", udid, RUNNER, "data"],
                                   capture_output=True, text=True)
            if found.returncode == 0:
                folder = Path(found.stdout.strip()) / "Documents" / session
            time.sleep(.5)
        state.write_text(json.dumps({"folder": str(folder), "pid": proc.pid}))
    except BaseException:
        if proc.poll() is None:
            proc.terminate()
        raise


def stop(udid):
    state = state_file(udid)
    if state.exists():
        call(Path(json.loads(state.read_text())["folder"]), "stop")
        state.unlink()
