"""run_leg.py with a fake `wf` (the vault) and a fake `cargo` (the flow test): the record it writes for each outcome.
Run: python3 -m unittest tooling/real-backends/test_run_leg.py
"""
import contextlib, io, json, os, stat, sys, tempfile, unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_leg

SECRET = "fake-credential-value-0001"


def script(path, body):
    with open(path, "w") as f:
        f.write("#!/bin/sh\n" + body)
    os.chmod(path, os.stat(path).st_mode | stat.S_IEXEC)


class RunLeg(unittest.TestCase):
    def run_leg(self, leg, vault_has_key, cargo_body):
        work = tempfile.mkdtemp()
        bin_dir = os.path.join(work, "bin")
        os.makedirs(bin_dir)
        script(os.path.join(bin_dir, "wf"), f'echo {SECRET}\n' if vault_has_key else "exit 1\n")
        script(os.path.join(bin_dir, "cargo"), cargo_body)
        out = os.path.join(work, "out.json")
        env = {"PATH": bin_dir + ":" + os.environ["PATH"], "REAL_BACKEND_OUT": out}
        installs = []
        saved = (dict(os.environ), run_leg.install, sys.argv)
        os.environ.clear()
        os.environ.update(env)
        run_leg.install = lambda leg, w: installs.append(leg) or w
        sys.argv = ["run_leg.py", "--leg", leg]
        stdout = io.StringIO()
        try:
            with contextlib.redirect_stdout(stdout):
                with self.assertRaises(SystemExit) as raised:
                    run_leg.main()
        finally:
            os.environ.clear()
            os.environ.update(saved[0])
            run_leg.install, sys.argv = saved[1], saved[2]
        record = json.load(open(out)) if os.path.exists(out) else None
        return record, raised.exception.code, installs, stdout.getvalue()

    def test_given_the_install_mode_when_a_cli_is_named_then_only_the_pinned_cli_is_installed_and_its_directory_printed_with_no_vault_read(self):
        # Given an install that records what it was asked for, and a `wf` that would fail the run if the vault were asked
        work = tempfile.mkdtemp()
        bin_dir = os.path.join(work, "bin")
        os.makedirs(bin_dir)
        script(os.path.join(bin_dir, "wf"), "exit 9\n")
        installs = []
        saved = (dict(os.environ), run_leg.install, sys.argv)
        os.environ["PATH"] = bin_dir + ":" + os.environ["PATH"]
        run_leg.install = lambda leg, into: installs.append((leg, into)) or os.path.join(into, "bin")
        sys.argv = ["run_leg.py", "--install", "codex", "--into", os.path.join(work, "cli")]
        stdout = io.StringIO()
        try:
            # When it runs
            with contextlib.redirect_stdout(stdout):
                run_leg.main()
        finally:
            os.environ.clear()
            os.environ.update(saved[0])
            run_leg.install, sys.argv = saved[1], saved[2]
        # Then the pinned codex was installed into the directory and its bin directory printed
        self.assertEqual(installs, [("codex", os.path.join(work, "cli"))])
        self.assertEqual(stdout.getvalue().strip(), os.path.join(work, "cli", "bin"))

    def test_given_a_passing_turn_when_the_leg_runs_then_the_record_merges_the_methods_and_final_state_and_no_value(self):
        # Given a vault with the key and a test that writes its result and passes
        result = json.dumps({"schema": "x", "driver": "codex", "methods": [{"method": "turn.start", "ok": True}], "final_state": "completed", "replied": True, "wrote_file": True})
        record, code, installs, out = self.run_leg("codex", True, f"echo '{result}' > \"$RCA_REAL_RESULT_OUT\"\necho the credential is {SECRET}\nexit 0\n")
        # Then the record says ok with the methods and the final state, the output is scrubbed of the value, and nothing carries it
        self.assertEqual((code, record["leg"], record["ok"], record["final_state"]), (0, "codex", True, "completed"))
        self.assertEqual(record["methods"], [{"method": "turn.start", "ok": True}])
        self.assertNotIn(SECRET, json.dumps(record) + out)

    def test_given_a_failing_turn_when_the_leg_runs_then_the_record_is_not_ok_with_the_exit_code_and_what_the_test_wrote(self):
        result = json.dumps({"methods": [{"method": "turn.start", "ok": True}], "final_state": "failed"})
        record, code, _, _ = self.run_leg("claude", True, f"echo '{result}' > \"$RCA_REAL_RESULT_OUT\"\nexit 101\n")
        self.assertEqual((code, record["ok"], record["exit"], record["final_state"]), (101, False, 101, "failed"))

    def test_given_a_vault_without_the_key_when_the_leg_runs_then_it_writes_the_missing_key_and_installs_and_tests_nothing(self):
        # Given a vault that holds no value for the Claude token
        record, code, installs, _ = self.run_leg("claude", False, "touch \"$0.ran\"\nexit 0\n")
        # Then the record names the leg and the key, nothing was installed, and the test never ran
        self.assertEqual((record["leg"], record["ok"], record["missing_key"]), ("claude", False, "CLAUDE_CODE_OAUTH_TOKEN"))
        self.assertEqual(installs, [])
        self.assertIn("CLAUDE_CODE_OAUTH_TOKEN", record["leg_error"])
        self.assertNotEqual(code, 0)


if __name__ == "__main__":
    unittest.main()
