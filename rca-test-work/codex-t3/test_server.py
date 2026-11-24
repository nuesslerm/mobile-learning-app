import http.client
import json
import threading
import unittest

from server import create_server


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.server = create_server(0)
        self.addCleanup(self.server.server_close)
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()
        self.addCleanup(self.stop_server)

    def stop_server(self):
        self.server.shutdown()
        self.thread.join(timeout=5)
        self.assertFalse(self.thread.is_alive())

    def request(self, path):
        connection = http.client.HTTPConnection(*self.server.server_address, timeout=5)
        self.addCleanup(connection.close)
        connection.request("GET", path)
        response = connection.getresponse()
        return response, response.read()

    def test_healthz_returns_json_over_loopback(self):
        """Given an ephemeral server, when GET /healthz arrives, then return JSON."""
        self.assertEqual(self.server.server_address[0], "127.0.0.1")
        self.assertGreater(self.server.server_address[1], 0)
        response, body = self.request("/healthz")
        self.assertEqual(response.status, 200)
        self.assertEqual(response.getheader("Content-Type"), "application/json")
        self.assertEqual(int(response.getheader("Content-Length")), len(body))
        self.assertEqual(json.loads(body), {"status": "ok"})

    def test_other_paths_return_404(self):
        """Given a running server, when other targets arrive, then return 404."""
        for path in ("/", "/missing", "/healthz/", "/healthz?probe=1"):
            with self.subTest(path=path):
                response, _ = self.request(path)
                self.assertEqual(response.status, 404)

    def test_two_servers_can_bind_ephemeral_ports(self):
        """Given one bound server, when another binds port zero, then both work."""
        with create_server(0) as second:
            self.assertNotEqual(second.server_address, self.server.server_address)
            response, _ = self.request("/healthz")
            self.assertEqual(response.status, 200)


if __name__ == "__main__":
    unittest.main()
