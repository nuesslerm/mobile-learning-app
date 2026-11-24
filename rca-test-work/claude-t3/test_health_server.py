import json
import threading
import unittest
import urllib.error
import urllib.request

from health_server import HOST, make_server


class HealthServerTest(unittest.TestCase):
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

    def get(self, path):
        url = f"http://{HOST}:{self.port}{path}"
        try:
            return urllib.request.urlopen(url, timeout=5)
        except urllib.error.HTTPError as err:
            return err

    def test_given_ephemeral_port_when_bound_then_loopback_and_nonzero(self):
        self.assertEqual(self.server.server_address[0], "127.0.0.1")
        self.assertGreater(self.port, 0)

    def test_given_server_when_get_healthz_then_200_json_ok(self):
        res = self.get("/healthz")
        self.assertEqual(res.status, 200)
        self.assertEqual(res.headers["Content-Type"], "application/json")
        self.assertEqual(json.loads(res.read()), {"status": "ok"})

    def test_given_query_string_when_get_healthz_then_200(self):
        self.assertEqual(self.get("/healthz?verbose=1").status, 200)

    def test_given_unknown_path_when_get_then_404_json(self):
        res = self.get("/nope")
        self.assertEqual(res.code, 404)
        self.assertEqual(res.headers["Content-Type"], "application/json")
        self.assertEqual(json.loads(res.read()), {"error": "not found"})

    def test_given_root_when_get_then_404(self):
        self.assertEqual(self.get("/").code, 404)


if __name__ == "__main__":
    unittest.main()
