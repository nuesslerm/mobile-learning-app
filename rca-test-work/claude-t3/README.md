# claude-t3 health server

Loopback-only HTTP health server built on the Python standard library (no dependencies).

## Endpoints

- `GET /healthz` returns `200` with `{"status": "ok"}` (`application/json`).
- Any other path returns `404` with `{"error": "not found"}`.

## Run

    python3 health_server.py   # listens on http://127.0.0.1:8080

The server binds to `127.0.0.1` only. `make_server(port=0)` picks an ephemeral port.

## Test

    python3 -m unittest -v

The tests start the server on a real ephemeral loopback port and make real HTTP requests.
