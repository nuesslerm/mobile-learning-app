"""Serve a single selected simulator over one idb companion connection."""
import asyncio
import json
import logging
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from contextlib import contextmanager

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


@contextmanager
def companion(udid, default_socket):
    developer = Path(os.getenv("DEVELOPER_DIR") or subprocess.check_output(["xcode-select", "-p"], text=True).strip())
    framework = developer.parent / "SharedFrameworks/SimulatorKit.framework"
    relative = Path("Library/PrivateFrameworks/SimulatorKit.framework")
    if (developer / relative).exists() or not framework.exists():
        yield default_socket
        return
    # Xcode 27 moved SimulatorKit. Give this companion the layout idb expects.
    with tempfile.TemporaryDirectory(dir="/tmp", prefix="wf-idb-") as temp:
        contents = Path(temp) / "Xcode.app/Contents"
        prefix = contents / "Developer"
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


def type_with_xctest(udid, value):
    project = Path(__file__).resolve().parent / "input/Input.xcodeproj"
    cache = Path(os.getenv("RUNNER_TEMP") or tempfile.gettempdir()) / "wf-ios-input"
    subprocess.run(["xcodebuild", "build-for-testing", "-project", str(project), "-scheme", "InputTests",
                    "-destination", "generic/platform=iOS Simulator", "-derivedDataPath", str(cache), "-quiet"],
                   check=True, stdout=sys.stderr)
    runner, = (cache / "Build/Products").glob("*.xctestrun")
    subprocess.run(["xcodebuild", "test-without-building", "-xctestrun", str(runner), "-destination",
                    "id=" + udid, "-parallel-testing-enabled", "NO", "-quiet"],
                   env={**os.environ, "TEST_RUNNER_WF_TEXT": value}, check=True, stdout=sys.stderr)
    subprocess.run(["xcrun", "simctl", "launch", udid, "host.exp.Exponent"], check=True, stdout=sys.stderr)


async def keyboard_hit(client, width, height, key, x, ys):
    for y in ys:
        hit = json.loads((await client.accessibility_info((int(x*width), int(y*height)), False)).json)
        for row in hit if isinstance(hit, list) else [hit]:
            if str(row.get("AXLabel") or "").lower() == key:
                return row["frame"]
    return None


def key_position(key):
    for letters, left, y in (("qwertyuiop", .05, .715), ("asdfghjkl", .10, .775),
                             ("zxcvbnm", .20, .84), ("1234567890", .05, .715)):
        if key in letters and len(key) == 1:
            return left + letters.index(key)*.10, y
    return {"space": (.50,.90), "numbers": (.25,.90), "letters": (.12,.90)}.get(key)


async def type_on_keyboard(client, value, dimensions):
    height = dimensions["height_points"]
    for char in value:
        deadline = time.monotonic() + 2
        while True:
            rows = json.loads((await client.accessibility_info(None, False)).json)
            keys = {str(row.get("AXLabel") or "").lower(): row["frame"] for row in rows
                    if row.get("type") in ("Key", "Button") and row["frame"]["y"] > height*.58}
            wanted = "space" if char == " " else char.lower()
            box = keys.get(wanted)
            width = dimensions["width_points"]
            pos = key_position(wanted)
            if not box and pos:
                x, y = pos
                box = await keyboard_hit(client, width, height, wanted, x, (y, y+.045, y-.04))
            if box is not None:
                await client.tap(box["x"] + box["width"]/2, box["y"] + box["height"]/2, duration=.07)
                break
            if time.monotonic() > deadline:
                raise ValueError(f"Software keyboard did not expose {wanted}, keys {list(keys)}")
            toggle = ("more, numbers", "numbers") if char.isdigit() else ("more, letters", "letters")
            switch = next((keys[name] for name in toggle if name in keys), None)
            if switch is None:
                for x in (.06,.19,.25):
                    switch = await keyboard_hit(client, width, height, toggle[-1], x, (.895,.915))
                    if switch:
                        break
            if switch:
                await client.tap(switch["x"] + switch["width"]/2, switch["y"] + switch["height"]/2)
            await asyncio.sleep(.1)


async def serve(udid):
    info = json.loads(subprocess.check_output(["idb", "describe", "--udid", udid, "--json"]))
    if info["state"] != "Booted" or info["target_type"] != "simulator" or info["udid"] != udid:
        raise ValueError(f"{udid} must identify a booted simulator")
    socket = info["companion_info"]["address"]["path"]
    with companion(udid, socket) as selected:
        await drive(selected, udid, info)


async def drive(socket, udid, info):
    async with Client.build(DomainSocketAddress(socket), logging.getLogger(__name__)) as client:
        await client.set_hardware_keyboard(False)
        for line in sys.stdin:
            op, *args = json.loads(line)
            try:
                result = None
                if op == "tree":
                    result = json.loads((await client.accessibility_info(None, False)).json)
                elif op == "keyboard":
                    dims = info["screen_dimensions"]
                    width, height = dims["width_points"], dims["height_points"]
                    rows = json.loads((await client.accessibility_info(None, False)).json)
                    # idb lists the application tree. The keyboard has its own window.
                    for x, y in ((.5, .91), (.05, .73), (.08, .85)):
                        element = json.loads((await client.accessibility_info((int(width*x), int(height*y)), False)).json)
                        rows.extend(element if isinstance(element, list) else [element])
                    result = any(r.get("type") in ("Keyboard", "Key") or r.get("AXLabel") in
                                 ("space", "Space", "shift", "Shift", "more, numbers") or
                                 r.get("AXUniqueId") in ("SymbolBarGroup-g3", "EditorAction-Done") for r in rows)
                elif op == "screen":
                    dims = info["screen_dimensions"]
                    result = dict(width=dims["width_points"], height=dims["height_points"], dpr=dims["density"])
                elif op == "stop":
                    await client.terminate("host.exp.Exponent")
                elif op == "open":
                    await client.open_url(args[0])
                elif op == "tap":
                    await client.tap(*args)
                elif op == "swipe":
                    await client.swipe(tuple(args[0]), tuple(args[1]), duration=args[2]/1000)
                elif op == "text":
                    if args[0].isascii() and all(c.islower() or c.isdigit() or c == " " for c in args[0]):
                        await type_on_keyboard(client, args[0], info["screen_dimensions"])
                    else:
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
    asyncio.run(serve(sys.argv[1]))
