# claude-orca test server

Small Python standard-library HTTP server bound to loopback (`127.0.0.1`).

- `GET /healthz` returns `200` with `{"status": "ok"}`
- any other path returns `404` with `{"error": "not found"}`

## Run

```sh
python3 server.py 8080
curl -i http://127.0.0.1:8080/healthz
```

## Test

Tests make real HTTP requests against a server on an ephemeral port.

```sh
cd rca-test-work/claude-orca
python3 -m unittest -v
```
