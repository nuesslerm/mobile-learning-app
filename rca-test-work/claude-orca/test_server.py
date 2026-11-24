import json
import threading
import unittest
import urllib.error
import urllib.request

from server import HOST, make_server


class ServerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = make_server(0)
        cls.port = cls.server.server_address[1]
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def url(self, path):
        return f"http://{HOST}:{self.port}{path}"

    def test_binds_loopback_on_ephemeral_port(self):
        # Given a server started with port 0
        # Then it is bound to loopback on a kernel-assigned port
        self.assertEqual(self.server.server_address[0], "127.0.0.1")
        self.assertGreater(self.port, 0)

    def test_healthz_returns_json(self):
        # Given a running server
        # When GET /healthz
        with urllib.request.urlopen(self.url("/healthz")) as res:
            body = json.loads(res.read())
            # Then 200 with a JSON body
            self.assertEqual(res.status, 200)
            self.assertEqual(res.headers["Content-Type"], "application/json")
        self.assertEqual(body, {"status": "ok"})

    def test_unknown_path_returns_404(self):
        # Given a running server
        # When GET an unknown path
        with self.assertRaises(urllib.error.HTTPError) as ctx:
            urllib.request.urlopen(self.url("/nope"))
        # Then 404 with a JSON error body
        self.assertEqual(ctx.exception.code, 404)
        with ctx.exception:
            self.assertEqual(json.loads(ctx.exception.read()), {"error": "not found"})


if __name__ == "__main__":
    unittest.main()
