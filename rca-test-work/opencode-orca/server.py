"""Tiny loopback-only HTTP server: GET /healthz -> JSON, any other path -> 404."""

from __future__ import annotations

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

HOST = "127.0.0.1"
DEFAULT_PORT = 8788
HEALTHZ_PATH = "/healthz"


class HealthHandler(BaseHTTPRequestHandler):
    server_version = "opencode-orca-health/1.0"

    def do_GET(self) -> None:
        path = self.path.split("?", 1)[0]
        if path == HEALTHZ_PATH:
            self._send_json(200, {"status": "ok"})
        else:
            self._send_json(404, {"error": "not_found"})

    def _send_json(self, status: int, payload: dict[str, Any]) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: Any) -> None:
        return


def create_server(port: int = DEFAULT_PORT) -> ThreadingHTTPServer:
    """Bind a threaded server to loopback; port=0 lets the OS pick a free port."""
    return ThreadingHTTPServer((HOST, port), HealthHandler)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Loopback health server.")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    args = parser.parse_args(argv)

    server = create_server(args.port)
    host, port = server.server_address[:2]
    print(f"listening on http://{host}:{port} (GET {HEALTHZ_PATH})", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
