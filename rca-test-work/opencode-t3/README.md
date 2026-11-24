# opencode-t3 health server

Tiny Python stdlib HTTP server bound to loopback (`127.0.0.1`).
`GET /healthz` returns JSON `{"status": "ok"}`; every other path returns
`404` with JSON `{"error": "not_found"}`.

## Run

```sh
python3 rca-test-work/opencode-t3/server.py --port 8787
```

```sh
curl -i http://127.0.0.1:8787/healthz
curl -i http://127.0.0.1:8787/anything-else
```

## Test

Real HTTP requests over a socket, server bound to an ephemeral port (`port=0`):

```sh
python3 -m unittest discover -s rca-test-work/opencode-t3 -v
```

or

```sh
python3 rca-test-work/opencode-t3/test_server.py -v
```

## Files

| File | Purpose |
| --- | --- |
| `server.py` | The server; `create_server(port=0)` binds an ephemeral loopback port |
| `test_server.py` | `unittest` suite driving the server over real HTTP |
