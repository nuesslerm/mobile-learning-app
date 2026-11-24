"""Loopback-only health server using only the Python standard library."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOST = "127.0.0.1"


class HealthHandler(BaseHTTPRequestHandler):
    def _send_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def do_GET(self):
        if self.path.split("?", 1)[0] == "/healthz":
            self._send_json(200, {"status": "ok"})
        else:
            self._send_json(404, {"error": "not found"})

    do_HEAD = do_GET

    def log_message(self, format, *args):
        pass


def make_server(port=0):
    """Bind to loopback; port 0 picks an ephemeral port."""
    return ThreadingHTTPServer((HOST, port), HealthHandler)


if __name__ == "__main__":
    server = make_server(8080)
    print(f"Listening on http://{HOST}:{server.server_address[1]}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
