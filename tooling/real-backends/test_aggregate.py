"""aggregate.py over the records of fake legs: the bundle lists the three backends with their methods, and a missing or failed one fails the run.
Run: python3 -m unittest tooling/real-backends/test_aggregate.py
"""
import json, os, sys, tempfile, unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import aggregate

PASS = {"ok": True, "methods": [{"method": "thread.create", "ok": True}, {"method": "turn.start", "ok": True}], "final_state": "completed"}


def put(directory, leg, record):
    sub = os.path.join(directory, f"real-backend-{leg}")
    os.makedirs(sub)
    with open(os.path.join(sub, f"real-backend-{leg}.json"), "w") as f:
        json.dump(dict(record, leg=leg), f)


class Aggregate(unittest.TestCase):
    def test_given_three_passing_legs_when_the_bundle_is_built_then_it_lists_each_backend_with_its_answered_methods(self):
        d = tempfile.mkdtemp()
        for leg in aggregate.LEGS:
            put(d, leg, PASS)
        bundle = aggregate.build(d)
        self.assertTrue(bundle["ok"])
        self.assertEqual([b["leg"] for b in bundle["backends"]], ["claude", "codex", "opencode"])
        self.assertTrue(all(b["methods"] == ["thread.create", "turn.start"] and b["final_state"] == "completed" for b in bundle["backends"]))

    def test_given_a_leg_that_never_uploaded_when_the_bundle_is_built_then_it_is_listed_missing_and_the_run_fails(self):
        d = tempfile.mkdtemp()
        put(d, "claude", PASS)
        put(d, "opencode", PASS)
        bundle = aggregate.build(d)
        self.assertEqual((bundle["ok"], bundle["missing"]), (False, ["codex"]))
        self.assertIn("| codex | missing |", aggregate.table(bundle))
        sys.argv = ["aggregate.py", "--dir", d, "--out", os.path.join(d, "bundle.json")]
        with self.assertRaises(SystemExit) as raised:
            aggregate.main()
        self.assertEqual(raised.exception.code, 1)
        with open(os.path.join(d, "bundle.json")) as f:
            self.assertEqual(json.load(f)["missing"], ["codex"])

    def test_given_a_leg_with_a_missing_credential_when_the_bundle_is_built_then_the_row_names_the_key_and_fails(self):
        d = tempfile.mkdtemp()
        put(d, "claude", PASS)
        put(d, "codex", {"ok": False, "missing_key": "CODEX_OAUTH_JSON_B64"})
        put(d, "opencode", PASS)
        bundle = aggregate.build(d)
        self.assertEqual((bundle["ok"], bundle["failed"]), (False, ["codex"]))
        self.assertIn("no value for CODEX_OAUTH_JSON_B64", aggregate.table(bundle))


if __name__ == "__main__":
    unittest.main()
