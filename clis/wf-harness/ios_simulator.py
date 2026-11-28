"""Provision the hosted test device and lend a clean Expo experience to one journey."""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time
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
    # simctl aborts before it runs its verb when it reads the device set while CoreSimulator rewrites the device pairs
    # ("Invalid parameter not satisfying: devicePair"): that is no answer from the simulator, so the command is asked again
    for attempt in range(3):
        try:
            return subprocess.check_output(["xcrun", "simctl", *args], text=True).strip()
        except subprocess.CalledProcessError as error:
            if error.returncode != -signal.SIGABRT or attempt == 2:
                raise
            time.sleep(2)


def quiet_dev_menu(udid):
    """Expo Go's own developer button floats over the app's top right corner and its first-run sheet over everything: both off."""
    prefs = Path(sim("get_app_container", udid, EXPO, "data")) / "Library/Preferences" / (EXPO + ".plist")
    prefs.parent.mkdir(parents=True, exist_ok=True)
    settings = plistlib.loads(prefs.read_bytes()) if prefs.exists() else {}
    settings.update(EXDevMenuShowFloatingActionButton=False, EXDevMenuIsOnboardingFinished=True)
    prefs.write_bytes(plistlib.dumps(settings, fmt=plistlib.FMT_BINARY))


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
        quiet_dev_menu(udid)
        sim("launch", udid, EXPO)
    manifest = simulator_manifest(udid)
    if os.getenv("WF_IOS_XCTEST_DRIVER") == "1":
        import xcui
        xcui.start(udid, os.environ["WF_IOS_INPUT_RUNNER"])
    print(json.dumps(manifest))


def installed_app(udid, bundle):
    """The .app of a bundle id inside a simulator's data, read from disk so the device need not be booted."""
    root = Path.home() / "Library/Developer/CoreSimulator/Devices" / udid / "data/Containers/Bundle/Application"
    for plist in root.glob("*/*.app/Info.plist"):
        info = plistlib.loads(plist.read_bytes())
        if info.get("CFBundleIdentifier") == bundle:
            return plist.parent, info.get("CFBundleShortVersionString", "")
    return None, ""


def fresh(name, source, bundles, schemes=(), reset=()):
    """A simulator of its own for one piece of work: the source's model and runtime, with the named apps copied from it.
    The source is only read. A device of that name is used again, and gets the apps it lacks."""
    devices = [dict(d, runtime=runtime) for runtime, rows in json.loads(sim("list", "devices", "available", "-j"))["devices"].items() for d in rows]
    origin = next((d for d in devices if source in (d["udid"], d["name"])), None)
    if origin is None:
        raise ValueError(f"No simulator is named {source}")
    target = next((d for d in devices if d["name"] == name), None)
    udid = target["udid"] if target else sim("create", name, origin["deviceTypeIdentifier"], origin["runtime"])
    if not target or target["state"] != "Booted":
        sim("boot", udid)
    subprocess.run(["xcrun", "simctl", "bootstatus", udid, "-b"], check=True, stdout=sys.stderr)
    apps = []
    for bundle in bundles:
        if bundle in reset:
            # An app's pairings and caches go with it: the next install starts as a new user's.
            subprocess.run(["xcrun", "simctl", "uninstall", udid, bundle], check=True, stdout=sys.stderr)
        app, version = (None, "") if bundle in reset else installed_app(udid, bundle)
        if app is None:
            app, version = installed_app(origin["udid"], bundle)
            if app is None:
                raise ValueError(f"{source} has no app {bundle}")
            print(f"Installing {bundle} {version} on {name}", file=sys.stderr, flush=True)
            sim("install", udid, str(app))
        apps.append({"bundle": bundle, "version": version})
    if EXPO in bundles:
        quiet_dev_menu(udid)
    for key in ("AccessibilityEnabled", "ApplicationAccessibilityEnabled"):
        sim("spawn", udid, "defaults", "write", "com.apple.Accessibility", key, "-bool", "true")
    # CoreSimulatorBridge opens links. Its first-open prompt per scheme belongs to SpringBoard, where no test can tap it.
    for scheme, bundle in (pair.split("=", 1) for pair in schemes):
        sim("spawn", udid, "defaults", "write", "com.apple.launchservices.schemeapproval",
            f"com.apple.CoreSimulator.CoreSimulatorBridge-->{scheme}", "-string", bundle)
    print(json.dumps({"device": name, "udid": udid, "model": origin["deviceTypeIdentifier"], "runtime": origin["runtime"],
                      "created": target is None, "appsFrom": origin["name"], "apps": apps, "schemes": list(schemes)}))


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


def prepare_android_learner(device, base, manifest):
    adb = ["adb", "-s", device]
    revision = hashlib.sha256(base.encode()).hexdigest()[:16]
    name = "wf-learner-" + revision
    directory = Path(os.environ["RUNNER_TEMP"]) / f"{device}-{name}"
    directory.mkdir(exist_ok=True)
    saved = directory / "saved"

    def snapshot(operation):
        result = subprocess.check_output([*adb, "emu", "avd", "snapshot", operation, name], text=True)
        if any(line.lstrip().startswith("KO") for line in result.splitlines()):
            raise RuntimeError(f"Could not {operation} learner snapshot: {result}")

    if saved.exists():
        try:
            snapshot("load")
            subprocess.run([*adb, "wait-for-device"], check=True, timeout=30)
        except (RuntimeError, subprocess.SubprocessError):
            # twice the emulator was gone for good after a load and adb could say nothing: its own log is the only witness
            log = Path(os.environ["RUNNER_TEMP"]) / "emulator.log"
            lines = log.read_text(errors="replace").splitlines()[-60:] if log.exists() else ["no emulator.log"]
            print("The emulator after the snapshot load:", *lines, sep="\n", file=sys.stderr, flush=True)
            raise
        return
    shell = [*adb, "shell"]
    # Expo Go’s ScopedContext puts learner files and preferences under the encoded experience key.
    subprocess.run([*shell, """set -e
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
    configure_android_dev_menu(shell)
    preflight = directory / "preflight"
    with (directory / "preflight.log").open("w") as logfile:
        subprocess.run(["wf", "android", "test", "e2e/support/native-ready.lua", "--device", device,
                        "--env", f"base={base}", "--test-output-dir", str(preflight), "--publish"],
                       check=True, stdout=logfile, stderr=subprocess.STDOUT,
                       env={**os.environ, "WF_NATIVE_DEVICE_INFO": json.dumps(manifest),
                            "WF_RUN_TIMING": str(preflight / "timing.jsonl")})
    # The checkpoint contains the verified library and its empty learner data, including JS memory.
    snapshot("save")
    listed = subprocess.check_output([*adb, "emu", "avd", "snapshot", "list"], text=True)
    if name not in listed:
        raise RuntimeError(f"Emulator did not retain learner snapshot {name}: {listed}")
    saved.touch()


def paired_desktop(args):
    """A pairing link of the desktop `wf test backends up --only agent-desktop` started, for the case that pairs with one."""
    if "case=paired" not in args.command:
        return []
    state = Path(__file__).resolve().parents[2] / ".agents/tmp/e2e-backends/backends.json"
    desktop = next((b for b in json.loads(state.read_text())["backends"] if b["name"] == "agent-desktop"), None) if state.exists() else None
    if desktop is None:
        raise ValueError("case=paired needs a desktop: wf test backends up --only agent-desktop")
    if args.platform == "android":
        # the app takes plain http only from this device, so the emulator's own port is sent on to the runner's
        subprocess.run(["adb", "-s", args.device, "reverse", f"tcp:{desktop['port']}", f"tcp:{desktop['port']}"], check=True)
    minted = subprocess.check_output([*shlex.split(desktop["mint"]), "--base-url", desktop["url"]], text=True)
    return ["--env", "pair_url=" + json.loads(minted)["url"], "--env", "reply=" + desktop["reply"]]


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
            return subprocess.call([*args.command, *paired_desktop(args)], env=env)
    if not (os.getenv("GITHUB_ACTIONS") == "true" and args.device.startswith("emulator-")):
        raise ValueError("Android learner suites require a hosted emulator")
    adb = ["adb", "-s", args.device, "shell"]
    def prop(key):
        return subprocess.check_output([*adb, "getprop", key], text=True).strip()
    package_info = subprocess.check_output([*adb, "dumpsys", "package", "host.exp.exponent"], text=True)
    version = next(line.strip().split("=", 1)[1] for line in package_info.splitlines() if "versionName=" in line)
    manifest = {"udid": args.device, "model": prop("ro.product.model"), "runtime": prop("ro.build.version.release"),
                "sdk": prop("ro.build.version.sdk"), "expoGo": version}
    prepare_android_learner(args.device, os.environ["WF_NATIVE_UPDATE_BASE"], manifest)
    status = subprocess.call([*args.command, *paired_desktop(args)], env={**os.environ, "WF_NATIVE_DEVICE_INFO": json.dumps(manifest)})
    if status and os.getenv("WF_RUN_TIMING"):
        diagnostics = Path(os.environ["WF_RUN_TIMING"]).with_name("android-process.log")
        with diagnostics.open("w") as output:
            for command in (["dumpsys", "activity", "exit-info", "host.exp.exponent"],
                            ["logcat", "-b", "crash", "-d", "-t", "100"]):
                subprocess.run([*adb, *command], stdout=output, stderr=subprocess.STDOUT)
    return status


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    verbs = parser.add_subparsers(dest="verb", required=True)
    prepare_verb = verbs.add_parser("prepare")
    target = prepare_verb.add_mutually_exclusive_group()
    target.add_argument("--device")
    target.add_argument("--stop-automation", metavar="UDID")
    target.add_argument("--build-input", action="store_true")
    target.add_argument("--fresh", metavar="NAME")
    prepare_verb.add_argument("--orientation", choices=("portrait", "landscape-left", "landscape-right"))
    prepare_verb.add_argument("--apps-from", metavar="DEVICE")
    prepare_verb.add_argument("--app", action="append", default=[], metavar="BUNDLE")
    prepare_verb.add_argument("--scheme", action="append", default=[], metavar="SCHEME=BUNDLE")
    prepare_verb.add_argument("--reset", action="append", default=[], metavar="BUNDLE")
    case = verbs.add_parser("case")
    case.add_argument("--platform", choices=("ios", "android"), required=True)
    case.add_argument("--device", required=True)
    case.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.verb == "prepare":
        sys.path.insert(0, str(Path(__file__).resolve().parent / "src/ios"))
        if args.orientation:
            if not args.device:
                parser.error("--orientation requires --device UDID")
            import xcui
            session = json.loads(xcui.state_file(args.device).read_text())
            xcui.call(Path(session["folder"]), "orientation", value=args.orientation)
            print(json.dumps({"device": args.device, "orientation": args.orientation}))
            return 0
        if args.stop_automation:
            import xcui
            xcui.stop(args.stop_automation)
            print(json.dumps({"stopped": args.stop_automation}))
            return
        elif args.build_input:
            bridge = Path(__file__).parent / "src/ios/bridge.py"
            return subprocess.call([sys.executable, bridge, "--build-input"])
        if args.fresh:
            if not args.apps_from:
                raise ValueError("--fresh needs --apps-from <device>: the simulator whose model, runtime and apps it takes")
            fresh(args.fresh, args.apps_from, args.app, args.scheme, args.reset)
            return 0
        prepare(args.device)
        return 0
    # The parent runner signals the whole process group, including the child.
    # Keep the wrapper alive until the child has closed its report and storage.
    signal.signal(signal.SIGTERM, lambda *_: None)
    return run_case(args)


if __name__ == "__main__":
    sys.exit(main())
