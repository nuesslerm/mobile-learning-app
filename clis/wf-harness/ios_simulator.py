"""Provision the hosted test device and lend a clean Expo experience to one journey."""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import sys
import tempfile
import urllib.request
import uuid
import xml.etree.ElementTree as ET

RUNTIME = "com.apple.CoreSimulator.SimRuntime.iOS-27-0"
MODEL = "com.apple.CoreSimulator.SimDeviceType.iPhone-17"
EXPO = "host.exp.Exponent"
VERSION = "58.0.7"
ARCHIVE = f"Expo-Go-{VERSION}.tar.gz"
DOWNLOAD = f"https://github.com/expo/expo-go-releases/releases/download/Expo-Go-{VERSION}/{ARCHIVE}"
SHA256 = "17ab20f856c72690a750e79826ab5c90550a1441b1d1d67db6cd252af979b903"


def sim(*args):
    return subprocess.check_output(["xcrun", "simctl", *args], text=True).strip()


def device_info(udid):
    rows = json.loads(sim("list", "devices", "available", "-j"))["devices"].get(RUNTIME, [])
    found = next((d for d in rows if d["udid"] == udid and d["deviceTypeIdentifier"] == MODEL), None)
    if found is None or found["state"] != "Booted":
        raise ValueError(f"Expected a booted iPhone 17 on iOS 27.0 for {udid}")
    return found


def prepare(udid):
    if not udid:
        if os.getenv("GITHUB_ACTIONS") != "true":
            raise ValueError("Local runs require --device <UDID>")
        xcode = subprocess.check_output(["xcodebuild", "-version"], text=True).strip()
        print(f"{xcode}\nChecking {RUNTIME} for {MODEL}", file=sys.stderr, flush=True)
        runtimes = json.loads(sim("list", "runtimes", "-j"))["runtimes"]
        runtime = next((r for r in runtimes if r["identifier"] == RUNTIME and r["isAvailable"]), None)
        if runtime is None:
            raise ValueError(f"The runner has no available {RUNTIME}")
        udid = sim("create", "tamil", MODEL, RUNTIME)
        print(f"Booting {udid} with {runtime['name']} ({runtime['buildversion']})", file=sys.stderr, flush=True)
        sim("boot", udid)
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            archive = folder / ARCHIVE
            print(f"Downloading Expo Go {VERSION}", file=sys.stderr, flush=True)
            urllib.request.urlretrieve(DOWNLOAD, archive)
            if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
                raise ValueError("Expo Go simulator download failed checksum verification")
            app = folder / "Expo Go.app"
            app.mkdir()
            subprocess.run(["tar", "-xzf", archive, "-C", app], check=True)
            subprocess.run(["xcrun", "simctl", "bootstatus", udid, "-b"], check=True, stdout=sys.stderr)
            print(f"Installing Expo Go {VERSION} on {udid}", file=sys.stderr, flush=True)
            sim("install", udid, str(app))
        # A fresh simulator may expose only an empty AXApplication until these are on before app launch.
        for key in ("AccessibilityEnabled", "ApplicationAccessibilityEnabled"):
            sim("spawn", udid, "defaults", "write", "com.apple.Accessibility", key, "-bool", "true")
        # CoreSimulatorBridge opens update links. Its first-open prompt belongs to SpringBoard.
        sim("spawn", udid, "defaults", "write", "com.apple.launchservices.schemeapproval",
            "com.apple.CoreSimulator.CoreSimulatorBridge-->exp", "-string", EXPO)
        prefs = Path(sim("get_app_container", udid, EXPO, "data")) / "Library/Preferences" / (EXPO + ".plist")
        prefs.parent.mkdir(parents=True, exist_ok=True)
        settings = plistlib.loads(prefs.read_bytes()) if prefs.exists() else {}
        settings.update(EXDevMenuShowFloatingActionButton=False, EXDevMenuIsOnboardingFinished=True)
        prefs.write_bytes(plistlib.dumps(settings, fmt=plistlib.FMT_BINARY))
        sim("launch", udid, EXPO)
    print(json.dumps(simulator_manifest(udid)))


def simulator_manifest(udid):
    device = device_info(udid)
    app = Path(sim("get_app_container", udid, EXPO, "app"))
    version = plistlib.loads((app / "Info.plist").read_bytes())["CFBundleShortVersionString"]
    if version != VERSION:
        raise ValueError(f"Expected Expo Go {VERSION} on {udid}, found {version}")
    return {"device": device["name"], "udid": udid, "model": MODEL, "runtime": RUNTIME,
                      "expoGo": version, "xcode": subprocess.check_output(["xcodebuild", "-version"], text=True).strip()}


def stop_expo(udid):
    # terminate exits nonzero when the app is already down.
    result = subprocess.run(["xcrun", "simctl", "terminate", udid, EXPO], capture_output=True, text=True)
    if result.returncode and not any(s in result.stderr for s in ("found nothing to terminate", "No such process")):
        raise RuntimeError(result.stderr)


@contextmanager
def clean_experience(udid):
    root = Path(sim("get_app_container", udid, EXPO, "data"))
    scope = root / "Documents/ExponentExperienceData/@nuesslerm/mla-tamagui"
    scope.parent.mkdir(parents=True, exist_ok=True)
    backup = root / ".wf-mla-original"
    journal = root / ".wf-mla-case"
    with (root / ".wf-mla.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        stop_expo(udid)

        def restore():
            if journal.exists():
                # On interruption between journaling and rename, the original is still in place.
                if backup.exists() or journal.read_text() == "empty":
                    if scope.exists():
                        scope.rename(root / f".wf-expired-{uuid.uuid4()}")
                    if backup.exists():
                        backup.rename(scope)
                journal.unlink()
            for old in root.glob(".wf-expired-*"):
                shutil.rmtree(old)

        restore()
        journal.write_text("original" if scope.exists() else "empty")
        if scope.exists():
            scope.rename(backup)
        try:
            yield
        finally:
            stop_expo(udid)
            restore()


def configure_android_dev_menu(adb):
    prefs = "/data/user/0/host.exp.exponent/shared_prefs/expo.modules.devmenu.sharedpreferences.xml"
    existing = subprocess.check_output([*adb, f'if [ -f {prefs} ]; then cat {prefs}; else echo "<map/>"; fi'])
    root = ET.fromstring(existing)
    changed = False
    for name, value in {"showFab": "false", "showsAtLaunch": "false", "isOnboardingFinished": "true"}.items():
        entry = root.find(f"./boolean[@name='{name}']")
        if entry is None:
            entry = ET.SubElement(root, "boolean", name=name)
        if entry.get("value") != value:
            entry.set("value", value)
            changed = True
    if changed:
        directory = "/data/user/0/host.exp.exponent"
        write = (f'set -e; cd {directory}; mkdir -p shared_prefs; cat > {prefs}; '
                 f'chown "$(stat -c %u .):$(stat -c %g .)" shared_prefs {prefs}; '
                 f'chmod 660 {prefs}; restorecon shared_prefs {prefs}')
        subprocess.run([*adb, write], input=ET.tostring(root, encoding="utf-8"), check=True)


def run_case(args):
    if args.platform == "ios":
        prepared = os.getenv("WF_NATIVE_DEVICE_INFO")
        if prepared and os.getenv("GITHUB_ACTIONS") == "true":
            manifest = json.loads(prepared)
            if (manifest.get("udid"), manifest.get("model"), manifest.get("runtime"), manifest.get("expoGo")) != (
                    args.device, MODEL, RUNTIME, VERSION):
                raise ValueError("Prepared simulator details do not match the selected test device")
        else:
            manifest = simulator_manifest(args.device)
        with clean_experience(args.device):
            # LaunchServices can time out while dispatching a URL to a terminated Expo Go.
            sim("launch", args.device, EXPO)
            env = {**os.environ, "WF_NATIVE_DEVICE_INFO": json.dumps(manifest)}
            return subprocess.call(args.command, env=env)
    if not (os.getenv("GITHUB_ACTIONS") == "true" and args.device.startswith("emulator-")):
        raise ValueError("Android learner suites require a hosted emulator")
    adb = ["adb", "-s", args.device, "shell"]
    # Expo Go’s ScopedContext puts learner files and preferences under the encoded experience key.
    subprocess.run([*adb, """set -e
am force-stop host.exp.exponent
cd /data/user/0/host.exp.exponent
for dir in files/ExperienceData/*; do
  [ -d "$dir" ] || continue
  scope=${dir##*/}
  rm -f -- databases/"$scope"-* shared_prefs/"$scope"-*
  rm -rf -- "cache/ExperienceData/$scope" "no_backup/ExperienceData/$scope" "$dir"
  echo "Reset Expo learner scope $scope"
done
rm -f -- databases/RKStorage*
"""], check=True)
    configure_android_dev_menu(adb)

    def prop(key):
        return subprocess.check_output([*adb, "getprop", key], text=True).strip()
    package_info = subprocess.check_output([*adb, "dumpsys", "package", "host.exp.exponent"], text=True)
    version = next(line.strip().split("=", 1)[1] for line in package_info.splitlines() if "versionName=" in line)
    manifest = {"udid": args.device, "model": prop("ro.product.model"), "runtime": prop("ro.build.version.release"),
                "sdk": prop("ro.build.version.sdk"), "expoGo": version}
    return subprocess.call(args.command, env={**os.environ, "WF_NATIVE_DEVICE_INFO": json.dumps(manifest)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    verbs = parser.add_subparsers(dest="verb", required=True)
    target = verbs.add_parser("prepare").add_mutually_exclusive_group()
    target.add_argument("--device")
    target.add_argument("--build-input", action="store_true")
    case = verbs.add_parser("case")
    case.add_argument("--platform", choices=("ios", "android"), required=True)
    case.add_argument("--device", required=True)
    case.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.verb == "prepare":
        if args.build_input:
            bridge = Path(__file__).parent / "src/ios/bridge.py"
            return subprocess.call([sys.executable, bridge, "--build-input"])
        prepare(args.device)
        return 0
    # The parent runner signals the whole process group, including the child.
    # Keep the wrapper alive until the child has closed its report and storage.
    signal.signal(signal.SIGTERM, lambda *_: None)
    return run_case(args)


if __name__ == "__main__":
    sys.exit(main())
