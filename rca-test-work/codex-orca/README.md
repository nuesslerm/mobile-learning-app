# Loopback health server

Requires Python 3; uses only the standard library. Run commands from the repository root.

Start the server (Ctrl-C stops it):

```sh
python3 -B rca-test-work/codex-orca/server.py --port 8000
```

The server binds only to IPv4 loopback (`127.0.0.1`). `GET /healthz` returns
HTTP 200 with `Content-Type: application/json` and `{"status": "ok"}`.
Unknown paths return HTTP 404.

Try requests in another terminal:

```sh
curl --fail http://127.0.0.1:8000/healthz
curl --include http://127.0.0.1:8000/unknown
```

Run the integration tests:

```sh
python3 -B -m unittest discover -s rca-test-work/codex-orca -v
```

Each test starts a loopback server on an OS-assigned ephemeral port, makes a
real HTTP request, and shuts the server down. Tests check the JSON health
response, unknown-path status, and loopback binding.
