# opencode-orca health server

Tiny Python standard-library HTTP server bound to loopback (`127.0.0.1`).
`GET /healthz` returns JSON `{"status": "ok"}`; every other path returns
`404` with JSON `{"error": "not_found"}`.

No third-party dependencies: only `http.server`, `json` and friends from the
standard library.

## Run

```sh
python3 rca-test-work/opencode-orca/server.py --port 8788
```

The server prints the address it bound to, then serves until interrupted:

```sh
listening on http://127.0.0.1:8788 (GET /healthz)
```

Probe it:

```sh
curl -i http://127.0.0.1:8788/healthz
curl -i http://127.0.0.1:8788/anything-else
```

## Test

Real HTTP requests over a socket, server bound to an ephemeral port (`port=0`):

```sh
python3 -m unittest discover -s rca-test-work/opencode-orca -v
```

or

```sh
python3 rca-test-work/opencode-orca/test_server.py -v
```

## Files

| File | Purpose |
| --- | --- |
| `server.py` | The server; `create_server(port=0)` binds an ephemeral loopback port |
| `test_server.py` | `unittest` suite driving the server over real HTTP |
