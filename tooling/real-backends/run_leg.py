#!/usr/bin/env python3
"""One real agent CLI through rca-service and rca-exec: installs the pinned CLI (checked against tooling/mimic-drivers/provision/real-agents-pin.json), reads its one credential
from the vault in memory, and runs that backend's ignored flow test (backend/remote-chat-agent/rs/tests/flow/real_backends.rs) with the credential in the environment of
that test alone. The report is the test's verdict with every credential value scrubbed.

Usage: run_leg.py --leg claude|codex|opencode
"""
import hashlib, json, os, re, subprocess, sys, tempfile, urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
PINS = json.load(open(os.path.join(ROOT, "tooling", "mimic-drivers", "provision", "real-agents-pin.json")))
LEGS = {
    "claude": {"key": "CLAUDE_CODE_OAUTH_TOKEN", "env": "CLAUDE_CODE_OAUTH_TOKEN", "test": "given_the_pinned_claude_when_a_turn_writes_a_file"},
    "opencode": {"key": "OPENCODE_GO_API_KEY", "env": "OPENCODE_API_KEY", "test": "given_the_pinned_opencode_when_a_turn_writes_a_file"},
    "codex": {"key": "CODEX_OAUTH_JSON_B64", "env": "CODEX_OAUTH_JSON_B64", "test": "given_the_pinned_codex_when_a_turn_writes_a_file"},
}


def fail(message):
    raise SystemExit(json.dumps({"ok": False, "leg_error": message}))


def vault(key):
    out = subprocess.run(["wf", "secret", "get", key, "--backend", "bitwarden"], capture_output=True, text=True, timeout=120)
    raw = out.stdout.strip()
    if out.returncode != 0 or not raw:
        fail(f"the vault has no value for {key}")
    try:
        d = json.loads(raw)
        raw = d.get("value") or d.get("secret") or raw
    except Exception:
        pass
    return raw


def install(leg, work):
    pin = PINS[leg]
    platform = pin["platforms"]["linux-x64"]
    url = platform.get("url") or f"{pin['release']}/opencode-linux-x64.tar.gz"
    data = urllib.request.urlopen(url, timeout=600).read()
    if hashlib.sha256(data).hexdigest() != platform["sha256"]:
        fail(f"{leg} does not hash to its pin")
    archive = os.path.join(work, leg + ".tgz")
    open(archive, "wb").write(data)
    subprocess.run(["tar", "xzf", archive, "-C", work], check=True)
    binary = os.path.join(work, platform.get("bin", leg))
    bindir = os.path.join(work, "bin")
    os.makedirs(bindir)
    os.chmod(binary, 0o755)
    os.symlink(binary, os.path.join(bindir, leg))
    return bindir


def scrub(text, secrets):
    for s in secrets:
        if s and len(s) >= 8:
            text = text.replace(s, "<secret>")
    return text


def main():
    leg = sys.argv[sys.argv.index("--leg") + 1]
    spec = LEGS[leg]
    work = tempfile.mkdtemp(prefix="real-backend-")
    bindir = install(leg, work)
    secret = vault(spec["key"])
    env = dict(os.environ, RCA_REAL_CLI_DIR=bindir, RCA_REAL_CREDENTIAL_ENV="" if leg == "codex" else spec["env"])
    env[spec["env"]] = secret
    cmd = ["cargo", "test", "--locked", "-p", "rca-service", "--test", "flow", spec["test"], "--", "--ignored", "--nocapture", "--test-threads=1"]
    run = subprocess.run(cmd, cwd=os.path.join(ROOT, "backend", "remote-chat-agent"), env=env, capture_output=True, text=True, timeout=3000)
    tail = scrub(re.sub(r"\x1b\[[0-9;]*m", "", run.stdout[-6000:] + "\n" + run.stderr[-6000:]), [secret])
    print(tail[-8000:])
    out = os.environ.get("REAL_BACKEND_OUT")
    if out:
        open(out, "w").write(json.dumps({"leg": leg, "exit": run.returncode, "version": PINS[leg]["version"]}, indent=2))
    sys.exit(run.returncode)


main()
