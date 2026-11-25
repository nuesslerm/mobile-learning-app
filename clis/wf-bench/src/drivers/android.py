"""Sample Android RSS and release wf's UiAutomation connection between drivers."""
import csv
import subprocess
import threading
import time


class AndroidDevice:
    def __init__(self, serial, app_id):
        self.adb = ["adb", "-s", serial, "shell"]
        self.app_id = app_id

    def processes(self):
        listing = subprocess.check_output([*self.adb, "ps", "-A", "-o", "PID,RSS,ARGS"],
                                          text=True, timeout=5)
        for line in listing.splitlines():
            fields = line.split(None, 2)
            if len(fields) != 3 or not all(field.isdecimal() for field in fields[:2]):
                continue
            pid, rss, cmd = fields
            args = cmd.split()
            if args == ["com.wetest.uia2.Main"] or args[-2:] == ["/", "com.wetest.uia2.Main"]:
                yield int(pid), int(rss), "wf-u2"
            elif args[0] == self.app_id or args[0].startswith(self.app_id + ":"):
                yield int(pid), int(rss), "app"
            elif args[0] in ("dev.mobile.maestro", "dev.mobile.maestro.test"):
                yield int(pid), int(rss), "maestro"

    def release_wf(self):
        for pid, _, name in self.processes():
            if name == "wf-u2":
                subprocess.run([*self.adb, "kill", str(pid)], check=True, timeout=5)
        deadline = time.monotonic() + 5
        while any(name == "wf-u2" for _, _, name in self.processes()):
            if time.monotonic() > deadline:
                raise RuntimeError("wf UiAutomation helper did not exit")
            time.sleep(.1)

    def sample(self, stop, directory, result):
        start = time.monotonic()
        with (directory / "device-rss.csv").open("w") as stream:
            writer = csv.writer(stream)
            writer.writerow(["elapsed_s", "pid", "process", "rss_kib"])
            while not stop.is_set():
                try:
                    rows = list(self.processes())
                except (OSError, subprocess.SubprocessError) as error:
                    result["deviceSamplingError"] = str(error)
                    break
                elapsed = round(time.monotonic() - start, 3)
                for pid, rss, name in rows:
                    writer.writerow([elapsed, pid, name, rss])
                app = [rss for _, rss, name in rows if name == "app"]
                if app:
                    mib = sum(app) / 1024
                    result.setdefault("firstAppMiB", mib)
                    result["peakAppMiB"] = max(result.get("peakAppMiB", 0), mib)
                stop.wait(.5)

    def measure(self, operation, folder):
        stop = threading.Event()
        memory = {}
        worker = threading.Thread(target=self.sample, args=(stop, folder, memory))
        worker.start()
        try:
            result = operation()
        finally:
            stop.set()
            worker.join()
        return dict(result, **memory)
