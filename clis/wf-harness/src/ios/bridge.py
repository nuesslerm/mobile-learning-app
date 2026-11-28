"""Serve a single selected simulator over one idb companion connection."""
import asyncio
import json
import fcntl
import logging
import math
import os
from pathlib import Path
import shutil
import socket as sockets
import subprocess
import sys
import tempfile
import time

import xcui
from contextlib import contextmanager
from dataclasses import asdict

# pipx installs the idb command and its Python libraries in the same virtualenv.
try:
    import idb
except ModuleNotFoundError:
    command = shutil.which("idb")
    if not command:
        raise SystemExit("Install fb-idb and idb-companion for simulator tests")
    python = Path(command).resolve().with_name("python")
    if not python.is_file() or str(python) == sys.executable:
        raise SystemExit("Set WF_IDB_PYTHON to the Python environment containing fb-idb")
    os.execv(python, [str(python), *sys.argv])

from idb.common.types import DomainSocketAddress, HIDDirection, HIDPress, HIDKey, HIDDelay, HIDTouch, Point
from idb.grpc.client import Client


async def events(items):
    for item in items:
        yield item


async def chord(client, modifier, key):
    await client.hid(events([
        HIDPress(HIDKey(modifier), HIDDirection.DOWN), HIDDelay(.06), HIDPress(HIDKey(key), HIDDirection.DOWN), HIDDelay(.06),
        HIDPress(HIDKey(key), HIDDirection.UP), HIDPress(HIDKey(modifier), HIDDirection.UP),
    ]))


def developer_dir():
    return Path(os.getenv("DEVELOPER_DIR") or subprocess.check_output(["xcode-select", "-p"], text=True).strip())


def compatible_developer(directory):
    developer = developer_dir()
    framework = developer.parent / "SharedFrameworks/SimulatorKit.framework"
    relative = Path("Library/PrivateFrameworks/SimulatorKit.framework")
    if (developer / relative).exists() or not framework.exists():
        return developer
    contents = Path(directory) / "Xcode.app/Contents"
    prefix = contents / "Developer"
    if (prefix / relative).exists():
        return prefix
    # Xcode 27 moved SimulatorKit. Give idb the layout it expects.
    for source, target, skip in (
        (developer.parent, contents, "Developer"),
        (developer, prefix, "Library"),
        (developer / "Library", prefix / "Library", "PrivateFrameworks"),
        (developer / "Library/PrivateFrameworks", prefix / "Library/PrivateFrameworks", "SimulatorKit.framework"),
    ):
        target.mkdir(parents=True, exist_ok=True)
        if source.exists():
            for entry in source.iterdir():
                if entry.name != skip:
                    (target / entry.name).symlink_to(entry)
    (prefix / relative).symlink_to(framework)
    return prefix


@contextmanager
def companion(udid, default_socket):
    with tempfile.TemporaryDirectory(dir="/tmp", prefix="wf-idb-") as temp:
        prefix = compatible_developer(temp)
        if prefix == developer_dir():
            yield default_socket
            return
        socket = str(Path(temp) / "idb.sock")
        proc = subprocess.Popen(["idb_companion", "--udid", udid, "--grpc-domain-sock", socket],
                                env={**os.environ, "DEVELOPER_DIR": str(prefix)}, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 15
            while not Path(socket).exists():
                if proc.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("The Xcode 27 HID companion failed to start")
                time.sleep(.05)
            yield socket
        finally:
            proc.terminate()
            proc.wait(timeout=10)


@contextmanager
def preserve_accessibility(udid):
    defaults = ["xcrun", "simctl", "spawn", udid, "defaults"]
    domain = "com.apple.Accessibility"

    def read(key):
        value = subprocess.run([*defaults, "read", domain, key], capture_output=True, text=True)
        if value.returncode == 0:
            return value.stdout.strip()
        if "does not exist" in value.stderr:
            return None
        raise RuntimeError(value.stderr)

    saved = {key: read(key) for key in ("AccessibilityEnabled", "ApplicationAccessibilityEnabled")}
    try:
        yield
    finally:
        for key, value in saved.items():
            if read(key) != value:
                # defaults prints a boolean as 1 or 0 and takes it back only as a word
                command = ["delete", domain, key] if value is None else ["write", domain, key, "-bool", "true" if value == "1" else "false"]
                subprocess.run([*defaults, *command], check=True, stdout=sys.stderr)


def build_input_runner():
    project = Path(__file__).resolve().parent / "input/Input.xcodeproj"
    cache = Path(os.getenv("RUNNER_TEMP") or tempfile.gettempdir()) / "wf-ios-input"
    subprocess.run(["xcodebuild", "build-for-testing", "-project", str(project), "-scheme", "InputTests",
                    "-destination", "generic/platform=iOS Simulator", "-derivedDataPath", str(cache), "-quiet"],
                   check=True, stdout=sys.stderr)
    runner, = (cache / "Build/Products").glob("*.xctestrun")
    return str(runner)


def type_with_xctest(udid, value):
    runner = os.getenv("WF_IOS_INPUT_RUNNER") or build_input_runner()
    with preserve_accessibility(udid):
        subprocess.run(["xcodebuild", "test-without-building", "-xctestrun", str(runner), "-destination",
                        "id=" + udid, "-parallel-testing-enabled", "NO", "-quiet"],
                       env={**os.environ, "TEST_RUNNER_WF_TEXT": value, "TEST_RUNNER_WF_IOS_APP_ID": xcui.target_app()}, check=True, stdout=sys.stderr)
    subprocess.run(["xcrun", "simctl", "launch", udid, xcui.target_app()], check=True, stdout=sys.stderr)


async def elements_at(client, x, y):
    try:
        hit = json.loads((await client.accessibility_info((int(x), int(y)), False)).json)
    except Exception as error:
        if "No accessibility element at" in str(error):
            return []
        raise
    return hit if isinstance(hit, list) else [hit]


async def keyboard_hit(client, width, height, key, x, ys):
    for y in ys:
        for row in await elements_at(client, x*width, y*height):
            if str(row.get("AXLabel") or "").lower() == key:
                return row["frame"]
    return None


def key_position(key):
    for letters, left, y in (("qwertyuiop", .05, .715), ("asdfghjkl", .10, .775),
                             ("zxcvbnm", .20, .84), ("1234567890", .05, .715)):
        if key in letters and len(key) == 1:
            return left + letters.index(key)*.10, y
    return {"space": (.50,.90), "numbers": (.25,.90), "letters": (.12,.90)}.get(key)


async def dismiss_keyboard_intro(client, rows, height):
    labels = " ".join(str(row.get("AXLabel") or "").lower() for row in rows)
    if "sliding your finger" not in labels and "slide to type" not in labels and "quickpath" not in labels:
        return False
    button = next((r["frame"] for r in rows if r.get("AXLabel") == "Continue"
                   and r.get("type") == "Button" and r["frame"]["y"] > height*.58), None)
    if button:
        await client.tap(button["x"] + button["width"]/2, button["y"] + button["height"]/2)
    return bool(button)


async def type_on_keyboard(client, value, dimensions, tree):
    width, height = dimensions["width_points"], dimensions["height_points"]
    keys = None
    for char in value:
        switched = False
        deadline = time.monotonic() + 2
        while True:
            if keys is None:
                rows = await tree()
                if time.monotonic() < deadline and await dismiss_keyboard_intro(client, rows, height):
                    await asyncio.sleep(.15)
                    continue
                keys = {str(row.get("AXLabel") or "").lower(): row["frame"] for row in rows
                        if row.get("type") in ("Key", "Button") and row["frame"]["y"] > height*.58}
            wanted = "space" if char == " " else char.lower()
            box = keys.get(wanted)
            pos = key_position(wanted)
            if not box and pos:
                x, y = pos
                box = await keyboard_hit(client, width, height, wanted, x, (y, y+.045, y-.04))
            if box is not None:
                await client.tap(box["x"] + box["width"]/2, box["y"] + box["height"]/2, duration=.07)
                keys[wanted] = box
                if char == " ":
                    keys = None
                break
            toggle = ("more, numbers", "numbers") if char.isdigit() else ("more, letters", "letters")
            switch = next((keys[name] for name in toggle if name in keys), None)
            if switch is None:
                for x in (.06,.19,.25):
                    switch = await keyboard_hit(client, width, height, toggle[-1], x, (.895,.915))
                    if switch:
                        break
            if switch and not switched:
                await client.tap(switch["x"] + switch["width"]/2, switch["y"] + switch["height"]/2)
                switched = True
                deadline = time.monotonic() + 2
            elif time.monotonic() > deadline:
                raise ValueError(f"Software keyboard did not expose {wanted}, keys {list(keys)}")
            keys = None
            await asyncio.sleep(.1)


async def accessibility_tree(client):
    try:
        return json.loads((await client.accessibility_info(None, False)).json)
    except Exception as error:
        if "no frontmost application" in str(error):
            return []
        if "accessibility service did not answer" not in str(error):
            raise
    # The simulator can reject a read while handing its window from SpringBoard to Expo Go.
    return json.loads((await client.accessibility_info(None, False)).json)


def has_keyboard(rows):
    return any(row.get("type") in ("Keyboard", "Key") or row.get("AXLabel") in
               ("space", "Space", "shift", "Shift", "more, numbers") or
               row.get("AXUniqueId") in ("SymbolBarGroup-g3", "EditorAction-Done") for row in rows)


def describe(udid):
    info = json.loads(subprocess.check_output(["idb", "describe", "--udid", udid, "--json"]))
    if info["state"] != "Booted" or info["target_type"] != "simulator" or info["udid"] != udid:
        raise ValueError(f"{udid} must identify a booted simulator")
    return info


def job_companion(runner, udid):
    directory = Path(runner) / "wf-ios-companion" / udid
    directory.mkdir(parents=True, exist_ok=True)
    state = directory / "connection.json"
    with (directory / "lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if state.is_file():
            address = json.loads(state.read_text())["socket"]
            with sockets.socket(sockets.AF_UNIX) as probe:
                if probe.connect_ex(address) == 0:
                    return address
        address = str(Path(tempfile.mkdtemp(dir="/tmp", prefix="wf-ios-")) / "idb.sock")
        with (directory / "companion.log").open("a") as log:
            proc = subprocess.Popen(["idb_companion", "--udid", udid, "--grpc-domain-sock", address,
                                     "--log-level", "info"],
                                    env={**os.environ, "DEVELOPER_DIR": str(compatible_developer(directory))},
                                    stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        deadline = time.monotonic() + 30
        while not Path(address).is_socket():
            if proc.poll() is not None or time.monotonic() > deadline:
                if proc.poll() is None:
                    proc.terminate()
                    proc.wait(timeout=10)
                raise RuntimeError(f"Cannot start the job's iOS companion. See {directory / 'companion.log'}")
            time.sleep(.05)
        state.write_text(json.dumps({"pid": proc.pid, "socket": address}))
        return address


async def serve(udid):
    runner = os.getenv("RUNNER_TEMP")
    if os.getenv("GITHUB_ACTIONS") == "true" and runner:
        await drive(job_companion(runner, udid), udid)
    else:
        info = describe(udid)
        with companion(udid, info["companion_info"]["address"]["path"]) as selected:
            await drive(selected, udid, info)


async def drive(socket, udid, info=None):
    async with Client.build(DomainSocketAddress(socket), logging.getLogger(__name__)) as client:
        if info is None:
            target = await client.describe()
            if target.udid != udid or target.state != "Booted" or target.target_type.value != "simulator":
                raise ValueError(f"The companion is not connected to the selected booted simulator {udid}")
            info = {"screen_dimensions": asdict(target.screen_dimensions)}
        await client.set_hardware_keyboard(False)
        folder = None
        if os.getenv("WF_IOS_XCTEST_DRIVER") == "1":
            session = json.loads(xcui.state_file(udid).read_text())
            if session.get("app", "host.exp.Exponent") != xcui.target_app():
                raise RuntimeError("XCTest session belongs to another target app")
            folder = Path(session["folder"])

        async def tree():
            if folder:
                return xcui.tree(folder)
            return await accessibility_tree(client)

        tree_at, last_tree = 0.0, []
        for line in sys.stdin:
            op, *args = json.loads(line)
            try:
                result = None
                if op not in ("tree", "screen", "keyboard", "screenshot"):
                    tree_at = 0.0
                if op == "tree":
                    # Share one lookup across adjacent selectors. The 100 ms geometry settle must read a new frame.
                    if time.monotonic() - tree_at > .045:
                        last_tree = await tree()
                        tree_at = time.monotonic()
                    result = last_tree
                elif op == "keyboard":
                    dims = info["screen_dimensions"]
                    width, height = dims["width_points"], dims["height_points"]
                    result = has_keyboard(await tree())
                    # A keyboard in a separate window can only be reached by hit testing.
                    for x, y in ((.5, .91), (.05, .73), (.08, .85)):
                        if result:
                            break
                        result = has_keyboard(await elements_at(client, width*x, height*y))
                elif op == "screen":
                    dims = info["screen_dimensions"]
                    result = dict(width=dims["width_points"], height=dims["height_points"], dpr=dims["density"])
                elif op == "stop":
                    try:
                        await client.terminate(xcui.target_app())
                    except Exception as error:
                        if "found nothing to terminate" not in str(error):
                            raise
                elif op == "open":
                    await client.open_url(args[0])
                elif op == "tap-labeled-button":
                    result = bool(folder and xcui.call(folder, "tap-button", identifier=args[0], label=args[1]))
                elif op == "tap":
                    await client.tap(*args)
                elif op == "swipe":
                    start, end, duration = tuple(args[0]), tuple(args[1]), args[2] / 1000
                    if start == end:
                        await client.tap(*start, duration=duration)
                    else:
                        await client.swipe(start, end, duration=duration)
                elif op == "text":
                    if folder:
                        rows = await tree()
                        await dismiss_keyboard_intro(client, rows, info["screen_dimensions"]["height_points"])
                        xcui.call(folder, "text", text=args[0])
                    elif args[0].isascii() and all(c.islower() or c.isdigit() or c == " " for c in args[0]):
                        await type_on_keyboard(client, args[0], info["screen_dimensions"], tree)
                    else:
                        rows = json.loads((await client.accessibility_info(None, False)).json)
                        await dismiss_keyboard_intro(client, rows, info["screen_dimensions"]["height_points"])
                        type_with_xctest(udid, args[0])
                elif op == "select-all":
                    await chord(client, 227, 4)
                elif op == "end":
                    await chord(client, 227, 81)
                elif op == "key":
                    await client.key(args[0])
                elif op == "stroke":
                    coords = args[0]
                    points = [Point(*coords[0])]
                    # Straight guides contain just their endpoints. The scorer consumes touch samples along the stroke.
                    for (x, y), (end_x, end_y) in zip(coords, coords[1:]):
                        steps = max(1, math.ceil(math.hypot(end_x-x, end_y-y)/4))
                        for i in range(1, steps+1):
                            points.append(Point(x+(end_x-x)*i/steps, y+(end_y-y)*i/steps))
                    sequence = []
                    for point in points:
                        sequence.extend((HIDPress(HIDTouch(point), HIDDirection.DOWN), HIDDelay(.018)))
                    sequence.append(HIDPress(HIDTouch(points[-1]), HIDDirection.UP))
                    await client.hid(events(sequence))
                elif op == "screenshot":
                    Path(args[0]).write_bytes(await client.screenshot())
                else:
                    raise ValueError(f"Unknown simulator command {op}")
                print(json.dumps({"value": result}), flush=True)
            except Exception as error:
                print(json.dumps({"error": str(error)}), flush=True)


if __name__ == "__main__":
    if sys.argv[1:] == ["--build-input"]:
        print(json.dumps({"xctestrun": build_input_runner()}))
    else:
        asyncio.run(serve(sys.argv[1]))
