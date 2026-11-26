"""Keep one XCTest accessibility and software-keyboard session on a selected simulator."""
from contextlib import closing
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import tempfile
import time
import uuid

RUNNER = "org.mobilelearning.NativeJourneyInputTests.xctrunner"


def target_app():
    return os.getenv("WF_IOS_APP_ID") or "host.exp.Exponent"


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


def tree(folder):
    deadline = time.monotonic() + 4
    while True:
        try:
            return call(folder, "tree")
        except RuntimeError as error:
            # A changing foreground window invalidates its snapshot between lookup and frame capture.
            transient = ("kAXErrorInvalidUIElement", "kAXErrorServerNotFound")
            if not any(code in str(error) for code in transient) or time.monotonic() >= deadline:
                raise
            time.sleep(.08)


def start(udid, runner):
    state = state_file(udid)
    if state.exists():
        previous = json.loads(state.read_text())
        try:
            os.kill(previous["pid"], 0)
        except ProcessLookupError:
            state.unlink()
        else:
            if previous.get("app", "host.exp.Exponent") != target_app():
                raise RuntimeError("Close the existing XCTest session before switching its target app")
            tree(Path(previous["folder"]))
            return
    session = "wf-ui-" + uuid.uuid4().hex
    log = state.with_suffix(".log")
    with log.open("w") as output:
        proc = subprocess.Popen(["xcodebuild", "test-without-building", "-xctestrun", runner, "-destination", "id=" + udid,
                                 "-parallel-testing-enabled", "NO", "-test-timeouts-enabled", "NO", "-collect-test-diagnostics", "never"],
                                env={**os.environ, "TEST_RUNNER_WF_UI_SESSION": session, "TEST_RUNNER_WF_IOS_APP_ID": target_app()},
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
        state.write_text(json.dumps({"folder": str(folder), "app": target_app(), "pid": proc.pid}))
    except BaseException:
        if proc.poll() is None:
            proc.terminate()
        raise


def stop(udid):
    state = state_file(udid)
    if state.exists():
        session = json.loads(state.read_text())
        pid = session["pid"]
        # XCTest releases the simulator after replying to the final command.
        with closing(select.kqueue()) as exits:
            event = select.kevent(pid, filter=select.KQ_FILTER_PROC, flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                                  fflags=select.KQ_NOTE_EXIT)
            exits.control([event], 0, 0)
            call(Path(session["folder"]), "stop")
            if not exits.control(None, 1, 30):
                os.kill(pid, signal.SIGTERM)
                if not exits.control(None, 1, 15):
                    raise RuntimeError(f"XCTest process {pid} has not released simulator {udid}")
        state.unlink()
