# Codex T3 loopback health server

Standalone Python 3 standard-library example. No dependencies or installation.
It binds only to IPv4 loopback (`127.0.0.1`).

From the repository root:

```sh
python3 -B rca-test-work/codex-t3/server.py
```

The default port is `0`: the OS selects an available ephemeral port and the
server prints its address. Use `--port 8080` to select a fixed port. Stop with
Ctrl-C; the listening socket closes on exit.

`GET /healthz` returns HTTP 200, `Content-Type: application/json`, and
`{"status": "ok"}`. All other GET targets return HTTP 404, including a trailing
slash or query string. Other HTTP methods use the standard handler's HTTP 501
response. This is a local test example.

Run the integration tests:

```sh
python3 -B -m unittest discover -s rca-test-work/codex-t3 -v
```

Tests bind real loopback sockets on OS-assigned ports, send HTTP requests,
check the JSON response and 404 routes, and verify simultaneous ephemeral
bindings. Each test shuts down its server, joins the serving thread, and
closes sockets. No fixed ports, sleeps, mocks, or third-party packages.

## Starting revision

Verified a clean linked Git worktree of `nuesslerm/mobile-learning-app` on
`t3/7b3a5b58`, neither `dev` nor `main`. After fetching `origin/dev`, both HEAD
and `origin/dev` were `d16bc2c2efdc46634f70a08b2323b6a475c64f5b`.
The test branch was renamed to `dev-codex-t3` before adding these files.
