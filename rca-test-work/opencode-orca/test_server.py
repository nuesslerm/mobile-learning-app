"""Real HTTP tests for the loopback health server (Python stdlib only)."""

from __future__ import annotations

import json
import socket
import sys
import threading
import unittest
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from server import HOST, create_server  # noqa: E402


class HealthServerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.server = create_server(port=0)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.host = str(cls.server.server_address[0])
        cls.port = int(cls.server.server_address[1])

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)

    def get(self, path: str) -> tuple[int, str, bytes]:
        url = f"http://{self.host}:{self.port}{path}"
        try:
            with urllib.request.urlopen(url, timeout=5) as response:
                return response.status, response.headers.get("Content-Type", ""), response.read()
        except urllib.error.HTTPError as error:
            with error:
                return error.code, error.headers.get("Content-Type", ""), error.read()

    def test_server_binds_to_loopback(self) -> None:
        self.assertEqual(self.host, HOST)
        self.assertEqual(self.host, "127.0.0.1")

    def test_server_bound_to_ephemeral_port(self) -> None:
        self.assertGreater(self.port, 0)

    def test_server_is_reachable_on_that_port(self) -> None:
        with socket.create_connection((self.host, self.port), timeout=5):
            pass

    def test_healthz_returns_ok_json(self) -> None:
        status, content_type, body = self.get("/healthz")
        self.assertEqual(status, 200)
        self.assertEqual(content_type, "application/json")
        self.assertEqual(json.loads(body), {"status": "ok"})

    def test_healthz_ignores_query_string(self) -> None:
        status, _, body = self.get("/healthz?probe=1")
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body), {"status": "ok"})

    def test_unknown_path_returns_404(self) -> None:
        status, content_type, body = self.get("/nope")
        self.assertEqual(status, 404)
        self.assertEqual(content_type, "application/json")
        self.assertEqual(json.loads(body), {"error": "not_found"})

    def test_root_path_returns_404(self) -> None:
        status, _, body = self.get("/")
        self.assertEqual(status, 404)
        self.assertEqual(json.loads(body), {"error": "not_found"})


if __name__ == "__main__":
    unittest.main()
