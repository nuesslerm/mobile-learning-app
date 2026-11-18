#!/usr/bin/env python3
"""Does an agent CLI send its own file and shell calls to the host (Agent Client Protocol), or run them itself?

One leg per run (claude, opencode, codex-adapter, codex-cli). The leg installs its pinned agent (every download is checked against a pin before it runs), takes its
one credential from the vault in memory, runs the agent with that credential alone, and prints a JSON report of the method names the agent sent: never a credential,
never reply text. The ACP client offers fs.readTextFile, fs.writeTextFile and terminal, plays the host with a virtual workspace, and records what the agent asked for.
"""
import base64, hashlib, json, os, re, subprocess, sys, tempfile, threading, time, urllib.request

PINS = {
    "codex-acp": {"url": "https://registry.npmjs.org/@zed-industries/codex-acp-linux-x64/-/codex-acp-linux-x64-0.16.0.tgz",
                  "integrity": "sha512-xs5zZBLpJuciEbZNx6ZSNL0qCa9h3i/zWpj40sp6QtF+L4Ow/7qzHdBzboGhHdcz1jrLedfZeRFDA2Elj8TLMA==", "bin": "package/bin/codex-acp"},
    "claude-acp": {"url": "https://registry.npmjs.org/@agentclientprotocol/claude-agent-acp/-/claude-agent-acp-0.108.0.tgz",
                   "integrity": "sha512-Y0HI3CB0DfhpWItCn2RE3xgpen001V2U/bHm3G6ZVKXsElF5N4uYyjYlmgNzZBKcBtZK0JqiE96NMaeVXtgJjw=="},
}
LEGS = {  # the vault key each leg reads, and the variable or file its agent reads it as
    "claude": "CLAUDE_CODE_OAUTH_TOKEN",
    "opencode": "OPENCODE_GO_API_KEY",
    "codex-adapter": "CODEX_OAUTH_JSON_B64",
    "codex-cli": "CODEX_OAUTH_JSON_B64",
}
PROMPT = "Create a file named hello.txt in the current directory containing the word hi. Then run the shell command: echo done. Reply with the command's output."
KEEP_ENV = ("PATH", "HOME", "LANG", "TZ", "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY", "https_proxy", "http_proxy", "no_proxy", "SSL_CERT_FILE", "SSL_CERT_DIR", "CURL_CA_BUNDLE")


def vault(key):
    out = subprocess.run(["wf", "secret", "get", key, "--backend", "bitwarden"], capture_output=True, text=True, timeout=120)
    raw = out.stdout.strip()
    if out.returncode != 0 or not raw:
        raise SystemExit(json.dumps({"ok": False, "leg_error": f"the vault has no value for {key}"}))
    try:
        d = json.loads(raw)
        raw = d.get("value") or d.get("secret") or raw
    except Exception:
        pass
    return raw


def fetch_checked(url, integrity, dest):
    data = urllib.request.urlopen(url, timeout=300).read()
    algo, want = integrity.split("-", 1)
    got = base64.b64encode(hashlib.new(algo, data).digest()).decode()
    if got != want:
        raise SystemExit(json.dumps({"ok": False, "leg_error": f"{url} does not hash to its pin"}))
    open(dest, "wb").write(data)


def install(work, name):
    pin = PINS[name]
    tgz = os.path.join(work, name + ".tgz")
    fetch_checked(pin["url"], pin["integrity"], tgz)
    subprocess.run(["tar", "xzf", tgz, "-C", work], check=True)
    return os.path.join(work, "package")


def scrub(text, secrets):
    for s in secrets:
        if s and len(s) >= 8:
            text = text.replace(s, "<secret>")
    return text


def acp_run(cmd, env, cwd, secrets, session_wait):
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, cwd=cwd, text=True, bufsize=1)
    seen, virtual, terminals, pending, lock, nid = [], {}, {}, {}, threading.Lock(), [0]

    def send(o):
        proc.stdin.write(json.dumps(o) + "\n")
        proc.stdin.flush()

    def call(method, params, wait=60):
        with lock:
            nid[0] += 1
            i = nid[0]
        ev = threading.Event()
        pending[i] = [ev, None]
        send({"jsonrpc": "2.0", "id": i, "method": method, "params": params})
        ev.wait(wait)
        return pending[i][1]

    def answer(msg):
        m, p, i = msg["method"], msg.get("params", {}), msg["id"]
        seen.append(("agent->client", m))
        if m == "fs/read_text_file":
            send({"jsonrpc": "2.0", "id": i, "result": {"content": virtual.get(p["path"], "")}})
        elif m == "fs/write_text_file":
            virtual[p["path"]] = p.get("content", "")
            send({"jsonrpc": "2.0", "id": i, "result": None})
        elif m == "terminal/create":
            tid = f"t{len(terminals) + 1}"
            terminals[tid] = p.get("command")
            send({"jsonrpc": "2.0", "id": i, "result": {"terminalId": tid}})
        elif m == "terminal/output":
            send({"jsonrpc": "2.0", "id": i, "result": {"output": "done\n", "truncated": False, "exitStatus": {"exitCode": 0}}})
        elif m == "terminal/wait_for_exit":
            send({"jsonrpc": "2.0", "id": i, "result": {"exitCode": 0}})
        elif m in ("terminal/release", "terminal/kill"):
            send({"jsonrpc": "2.0", "id": i, "result": None})
        elif m == "session/request_permission":
            opts = p.get("options", [])
            pick = next((o for o in opts if "allow" in str(o.get("kind", "")).lower()), opts[0] if opts else None)
            send({"jsonrpc": "2.0", "id": i, "result": {"outcome": {"outcome": "selected", "optionId": pick["optionId"]} if pick else {"outcome": "cancelled"}}})
        else:
            send({"jsonrpc": "2.0", "id": i, "error": {"code": -32601, "message": "method not found"}})

    def reader():
        for line in proc.stdout:
            try:
                msg = json.loads(line)
            except Exception:
                continue
            if "method" in msg and "id" in msg:
                answer(msg)
            elif "method" in msg:
                if msg["method"] == "session/update":
                    u = msg["params"].get("update", {})
                    seen.append(("update", f"{u.get('sessionUpdate')}:{u.get('kind', '')}"))
                else:
                    seen.append(("notification", msg["method"]))
            elif "id" in msg and msg["id"] in pending:
                pending[msg["id"]][1] = msg
                pending[msg["id"]][0].set()

    threading.Thread(target=reader, daemon=True).start()
    report = {}
    init = call("initialize", {"protocolVersion": 1, "clientCapabilities": {"fs": {"readTextFile": True, "writeTextFile": True}, "terminal": True}, "clientInfo": {"name": "acp-probe", "version": "0"}})
    res = (init or {}).get("result") or {}
    report["initialize"] = "ok" if res else "no answer"
    report["agentCapabilities"] = sorted((res.get("agentCapabilities") or {}).keys())
    report["authMethods"] = [a.get("id") for a in res.get("authMethods", [])]
    for a in res.get("authMethods", []):
        if a.get("id") in ("chatgpt", "opencode-login"):
            r = call("authenticate", {"methodId": a["id"]})
            report["authenticate"] = "ok" if r and "result" in r else "refused"
            break
    s = call("session/new", {"cwd": cwd, "mcpServers": []}, wait=session_wait)
    sid = ((s or {}).get("result") or {}).get("sessionId")
    report["session/new"] = "ok" if sid else ("no answer in %ds" % session_wait if s is None else "error")
    if sid:
        r = call("session/prompt", {"sessionId": sid, "prompt": [{"type": "text", "text": PROMPT}]}, wait=240)
        report["prompt"] = ((r or {}).get("result") or {}).get("stopReason") or ("error" if r else "no answer")
    time.sleep(1)
    counts = {}
    for d, m in seen:
        counts[f"{d} {m}"] = counts.get(f"{d} {m}", 0) + 1
    report["messages"] = counts
    report["client_fs_writes"] = len(virtual)
    report["client_terminals"] = len(terminals)
    report["agent_used_its_own_tools"] = sorted(os.listdir(cwd))
    proc.stdin.close()
    try:
        proc.wait(5)
    except Exception:
        proc.kill()
    err = scrub(proc.stderr.read(), secrets)
    report["stderr_errors"] = [re.sub(r"\x1b\[[0-9;]*m", "", l)[:300] for l in err.splitlines() if re.search(r"ERROR|error:|denied|refus|unknown variant|failed", l)][-5:]
    return report


def main():
    leg = sys.argv[sys.argv.index("--leg") + 1]
    work = tempfile.mkdtemp(prefix="acp-probe-")
    cwd = tempfile.mkdtemp(prefix="acp-cwd-")
    secret = vault(LEGS[leg])
    secrets = [secret]
    env = {k: v for k, v in os.environ.items() if k in KEEP_ENV}
    home = tempfile.mkdtemp(prefix="acp-home-")
    env["HOME"] = home
    report = {"leg": leg}
    try:
        run_leg(leg, work, cwd, env, secret, secrets, report)
    except SystemExit:
        raise
    except Exception as e:  # a failing leg reports itself, scrubbed, instead of dying silently
        import traceback
        report["leg_error"] = scrub("".join(traceback.format_exception_only(type(e), e)).strip()[:600], secrets)
        report["trace_tail"] = scrub("".join(traceback.format_tb(e.__traceback__)[-2:])[:600], secrets)
    text = scrub(json.dumps(report, indent=2), secrets)
    print(text)
    out = os.environ.get("ACP_PROBE_OUT")
    if out:
        open(out, "w").write(text)


def run_leg(leg, work, cwd, env, secret, secrets, report):
    if leg.startswith("codex"):
        codex_home = tempfile.mkdtemp(prefix="codex-home-", dir="/dev/shm" if os.path.isdir("/dev/shm") else None)
        os.chmod(codex_home, 0o700)
        login = base64.b64decode(secret)
        with open(os.path.join(codex_home, "auth.json"), "wb") as f:
            f.write(login)
        os.chmod(os.path.join(codex_home, "auth.json"), 0o600)
        env["CODEX_HOME"] = codex_home
        secrets.append(login.decode(errors="ignore"))
    if leg == "codex-adapter":
        pkg = install(work, "codex-acp")
        binary = os.path.join(pkg, "bin", "codex-acp")
        os.chmod(binary, 0o755)
        report.update(acp_run([binary], env, cwd, secrets, 90))
    elif leg == "claude":
        install(work, "claude-acp")  # the tarball is checked against its pin and stays at work/claude-acp.tgz
        proj = os.path.join(work, "proj")
        os.makedirs(proj)
        open(os.path.join(proj, "package.json"), "w").write('{"name":"acp-probe","private":true}')
        npm = subprocess.run(["npm", "install", "--ignore-scripts", "--no-audit", "--no-fund", "--prefix", proj, os.path.join(work, "claude-acp.tgz")], capture_output=True, text=True, timeout=600)
        if npm.returncode != 0:
            raise RuntimeError("npm install of the adapter failed: " + npm.stderr[-300:])
        pkg = os.path.join(proj, "node_modules", "@agentclientprotocol", "claude-agent-acp")
        meta = json.load(open(os.path.join(pkg, "package.json")))
        bin_rel = meta["bin"] if isinstance(meta["bin"], str) else list(meta["bin"].values())[0]
        env["CLAUDE_CODE_OAUTH_TOKEN"] = secret
        report.update(acp_run(["node", os.path.join(pkg, bin_rel)], env, cwd, secrets, 90))
    elif leg == "opencode":
        pins = json.load(open(os.path.join(os.path.dirname(__file__), "..", "mimic-drivers", "provision", "real-agents-pin.json")))["opencode"]
        url = f"{pins['release']}/opencode-linux-x64.tar.gz"
        data = urllib.request.urlopen(url, timeout=600).read()
        if hashlib.sha256(data).hexdigest() != pins["platforms"]["linux-x64"]["sha256"]:
            raise SystemExit(json.dumps({"ok": False, "leg_error": "opencode does not hash to its pin"}))
        open(os.path.join(work, "oc.tgz"), "wb").write(data)
        subprocess.run(["tar", "xzf", os.path.join(work, "oc.tgz"), "-C", work], check=True)
        env["OPENCODE_API_KEY"] = secret
        env["OPENCODE_CONFIG_CONTENT"] = json.dumps({"model": "opencode-go/deepseek-v4.1-flash"})
        report.update(acp_run([os.path.join(work, "opencode"), "acp"], env, cwd, secrets, 90))
    elif leg == "codex-cli":
        pins = json.load(open(os.path.join(os.path.dirname(__file__), "..", "mimic-drivers", "provision", "real-agents-pin.json")))["codex"]
        p = pins["platforms"]["linux-x64"]
        data = urllib.request.urlopen(p["url"], timeout=600).read()
        if hashlib.sha256(data).hexdigest() != p["sha256"]:
            raise SystemExit(json.dumps({"ok": False, "leg_error": "codex does not hash to its pin"}))
        open(os.path.join(work, "codex.tgz"), "wb").write(data)
        subprocess.run(["tar", "xzf", os.path.join(work, "codex.tgz"), "-C", work], check=True)
        binary = os.path.join(work, p["bin"])
        v = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=60, env=env)
        r = subprocess.run([binary, "exec", "--skip-git-repo-check", "-s", "read-only", "reply with OK"], capture_output=True, text=True, timeout=240, env=env, cwd=cwd)
        tail = scrub(re.sub(r"\x1b\[[0-9;]*m", "", r.stderr + r.stdout), secrets)
        report.update({"version": v.stdout.strip(), "exit": r.returncode, "answered_ok": "OK" in r.stdout,
                       "errors": [l[:300] for l in tail.splitlines() if re.search(r"ERROR|error|unknown variant|denied", l)][-5:]})


main()
