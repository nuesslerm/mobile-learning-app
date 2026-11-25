"""Replay the native exercise goldens on a disposable Expo Go simulator."""
import argparse
import asyncio
import json
import logging
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import time

from idb.common.types import DomainSocketAddress, HIDDelay, HIDDirection, HIDPress, HIDTouch, Point
from idb.grpc.client import Client


ROOT = Path(__file__).resolve().parents[2]
FEATURES = ROOT / "frontend/mla-tamagui/features"
TASKS = {
    "translate": ("accommodation", "accommodation-3ebaa1a106137eccffa8"),
    "listen": ("animals", "animals-85960d4c1502423c39fd"),
    "fill": ("accommodation", "accommodation-4dd738e50c542e9c0c3f"),
}


class ExerciseRun:
    def __init__(self, args):
        self.args = args
        self.simulator = Path.home() / "Library/Developer/CoreSimulator/Devices" / args.device
        metadata = plistlib.loads((self.simulator / "device.plist").read_bytes())
        if (metadata["deviceType"].split(".")[-1], metadata["runtime"].split(".")[-1]) != (
            "iPhone-18-Pro-Max", "iOS-27-0"
        ):
            raise ValueError("Exercise goldens require iPhone 18 Pro Max running iOS 27.0")
        self.socket = args.companion or f"/tmp/idb/{args.device}_companion.sock"
        self.env = {k: os.environ[k] for k in ("PATH", "HOME", "LANG", "TMPDIR") if k in os.environ}
        if args.companion:
            self.env["IDB_COMPANION"] = args.companion

    def idb(self, *args):
        return subprocess.check_output(
            ["idb", "--companion", self.socket, *args], env=self.env, timeout=90
        )

    def frame(self, selector, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for row in json.loads(self.idb("ui", "describe-all", "--json")):
                if selector in (row.get("AXLabel"), row.get("AXUniqueId")):
                    frame = row["frame"]
                    if frame["width"] and frame["height"]:
                        return frame
            time.sleep(0.25)
        raise RuntimeError(f"Expected visible element {selector!r}")

    def tap(self, selector):
        frame = self.frame(selector)
        self.idb("ui", "tap", str(round(frame["x"] + frame["width"] / 2)),
                 str(round(frame["y"] + frame["height"] / 2)))

    def open(self, route):
        self.idb("open", self.args.base.rstrip("/") + "/" + route.lstrip("/"))

    def screenshot(self, name, guard):
        self.frame(guard)
        time.sleep(0.6)
        command = [self.args.wf, "test", "ios-visual", f"{name}-{self.args.appearance}",
                   "--device", self.args.device, "--expect", guard]
        if self.args.update_goldens:
            command.append("--update-goldens")
        subprocess.run(command, cwd=ROOT, env=self.env, check=True, timeout=90)

    def prepare(self):
        self.idb("terminate", "host.exp.Exponent")
        containers = self.simulator / "data/Containers/Data/Application"
        folders = list(containers.glob(
            "*/Documents/ExponentExperienceData/@anonymous/mla-tamagui-*/tamil-progress"
        ))
        if len(folders) > 1:
            raise ValueError("Use a dedicated simulator with one MLA project")
        if folders:
            backup = Path(tempfile.mkdtemp(prefix="mla-exercise-progress-")) / "tamil-progress"
            shutil.move(folders[0], backup)
            print(f"Previous course test state saved to {backup}", flush=True)
        self.open("/")
        self.frame("LibraryHomeMenuButton", timeout=90)
        self.open("settings/appearance?visualCapture=1")
        self.tap(self.args.appearance.title())

    def lessons(self):
        curriculum = json.loads((FEATURES / "tamil-writing/data/lessons/course.json").read_text())
        tasks = {task["id"]: task for section in curriculum["sections"]
                 for topic in section["topics"] for task in topic["tasks"]}
        for kind, (topic, task_id) in TASKS.items():
            exercise = tasks[task_id]["exercises"][0]
            if exercise["kind"] != kind:
                raise ValueError(f"Choose a task starting with {kind}")
            self.open(f"tamil/lessons/{topic}/{task_id}?visualCapture=1")
            self.tap("TamilStartPractice")
            self.frame({"translate": "TamilSwitchAnswer", "listen": "TamilPlayAudio",
                        "fill": "TamilTile-bank-0"}[kind])
            self.screenshot(f"tamil-{kind}-prompt", "TamilCheck")
            if kind == "listen":
                continue
            if kind == "translate":
                for word in exercise["words"]:
                    self.tap(f"Add {word}")
                verdict = "Correct"
            else:
                self.tap("TamilTile-bank-1")
                verdict = "Incorrect"
            self.tap("TamilCheck")
            self.frame(verdict)
            self.screenshot(f"tamil-{kind}-{verdict.lower()}", "TamilContinue")

    async def trace_letter(self, points):
        async def events():
            for point in points:
                yield HIDPress(action=HIDTouch(point), direction=HIDDirection.DOWN)
                yield HIDDelay(0.035)
            yield HIDPress(action=HIDTouch(points[-1]), direction=HIDDirection.UP)

        async with Client.build(DomainSocketAddress(self.socket), logging.getLogger(__name__)) as client:
            await client.hid(events())

    def handwriting(self):
        self.open("tamil/uyir-1?visualCapture=1")
        self.frame("Stroke 1 of 2")
        self.screenshot("tamil-handwriting-guide", "TamilPracticeUndo")
        glyphs = FEATURES / "tamil-writing/domain/glyphs.generated.ts"
        js = ("import {pathToFileURL} from 'node:url';"
              "const {GENERATED_GLYPHS}=await import(pathToFileURL(process.argv[1]));"
              "console.log(JSON.stringify(GENERATED_GLYPHS.find(g=>g.characterId==='uyir-a').strokes[0]))")
        stroke = json.loads(subprocess.check_output(
            ["node", "--experimental-strip-types", "--input-type=module", "-e", js, str(glyphs)]
        ))
        # The native drawing surface is absent from AX. The Undo row anchors the 404 x 300 pt pad.
        top = self.frame("TamilPracticeUndo")["y"] - 12 - 300
        points = [Point(18 + 404 * p["x"], top + 300 * p["y"]) for p in stroke]
        asyncio.run(self.trace_letter(points))
        self.frame("Stroke 2 of 2")
        self.screenshot("tamil-handwriting-accepted", "TamilPracticeUndo")

    def wait_for_code_line(self):
        # The WebView exposes individual lines to point queries after CodeMirror hydrates.
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            row = json.loads(self.idb("ui", "describe-point", "--json", "210", "260"))
            if row.get("AXLabel") == "twoSum(nums, target) {":
                return
            time.sleep(0.25)
        raise RuntimeError("The native editor did not expose the Two Sum starter")

    def two_sum(self):
        self.open("leetcode/library/collection?collection=All%20Topics")
        self.tap("AlgorithmsProblem-two-sum")
        self.screenshot("leetcode-two-sum-description", "ProblemOpenEditor")
        self.tap("ProblemOpenEditor")
        self.frame("ConsoleBarRun")
        self.wait_for_code_line()
        self.screenshot("leetcode-two-sum-editor", "ConsoleBarRun")
        self.idb("ui", "tap", "210", "260")
        self.frame("SymbolBarGroup-g3")
        if any(row.get("AXLabel") == "Continue" and row["frame"]["width"]
               for row in json.loads(self.idb("ui", "describe-all", "--json"))):
            self.tap("Continue")
        self.screenshot("leetcode-two-sum-keyboard", "SymbolBarGroup-g3")
        self.tap("Hide Keyboard")
        time.sleep(0.6)
        self.tap("ConsoleBarRun")
        self.frame("ConsoleSheetHandle")
        time.sleep(2)
        self.screenshot("leetcode-two-sum-result", "ConsoleSheetHandle")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--device", required=True, help="Dedicated exercise simulator UDID")
    parser.add_argument("--base", required=True, help="Expo Go project link ending in /--")
    parser.add_argument("--appearance", choices=["light", "dark"], required=True)
    parser.add_argument("--companion", help="IDB companion Unix socket")
    parser.add_argument("--wf", default="wf")
    parser.add_argument("--update-goldens", action="store_true")
    test = ExerciseRun(parser.parse_args())
    test.prepare()
    test.lessons()
    test.handwriting()
    test.two_sum()
