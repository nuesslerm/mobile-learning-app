import http.client
import json
import threading
import unittest

from server import create_server


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.server = create_server(port=0)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.stop_server)

    def stop_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        self.assertFalse(self.thread.is_alive())

    def request(self, path):
        connection = http.client.HTTPConnection(*self.server.server_address, timeout=5)
        try:
            connection.request("GET", path)
            response = connection.getresponse()
            return response.status, response.getheader("Content-Type"), response.read()
        finally:
            connection.close()

    def test_healthz_returns_json(self):
        """Given a running server, when healthz is requested, then return JSON."""
        status, content_type, body = self.request("/healthz")
        self.assertEqual(status, 200)
        self.assertEqual(content_type, "application/json")
        self.assertEqual(json.loads(body), {"status": "ok"})

    def test_unknown_path_returns_404(self):
        """Given a running server, when an unknown path is requested, then return 404."""
        status, _, _ = self.request("/unknown")
        self.assertEqual(status, 404)

    def test_server_binds_to_loopback_on_ephemeral_port(self):
        """Given port zero, when serving a real request, then use loopback and an assigned port."""
        host, port = self.server.server_address
        self.assertEqual(host, "127.0.0.1")
        self.assertGreater(port, 0)
        self.assertEqual(self.request("/healthz")[0], 200)


if __name__ == "__main__":
    unittest.main()
