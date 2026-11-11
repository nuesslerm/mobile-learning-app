#!/bin/bash
# e2e verification for `wf` itself as it actually runs inside agent-shell:
# the real deployed binary, not a sandbox build, in a plain and a login shell
# (see docs/archive/README.md). Re-run after every
# wf-touching agent-shell redeploy, by hand (`e2e-wf-smoke`, on PATH) or via
# `wf vm exec` with `e2e-wf-smoke` as the script.
#
# Usage: ./e2e-wf-smoke.sh
#   No arguments, no cloning, no network beyond what `wf` itself needs --
#   this only exercises the binary already baked into THIS image.
set -uo pipefail

FAILURES=0
fail() {
  echo "[e2e-wf] FAIL: $1" >&2
  FAILURES=$((FAILURES + 1))
}
pass() {
  echo "[e2e-wf] pass: $1"
}

echo "[e2e-wf] --- checking wf --help prints usage ---"
HELP_OUTPUT="$(wf --help 2>&1)"
if echo "$HELP_OUTPUT" | grep -q -- 'Usage: wf'; then
  pass "wf --help prints usage"
else
  fail "wf --help did not print usage -- got:
$HELP_OUTPUT"
fi

echo "[e2e-wf] --- checking a login shell's wf --help emits no git-detection noise ---"
LOGIN_STDERR="$(bash -lc 'wf --help 2>&1 >/dev/null' 2>&1)"
if echo "$LOGIN_STDERR" | grep -q 'source=git'; then
  fail "login-shell wf --help logged git detection -- got:
$LOGIN_STDERR"
else
  pass "login-shell wf --help is quiet on stderr"
fi

echo "[e2e-wf] --- summary ---"
if [ "$FAILURES" -eq 0 ]; then
  echo "[e2e-wf] SUCCESS: all checks passed"
  exit 0
else
  echo "[e2e-wf] FAILURE: $FAILURES check(s) failed" >&2
  exit 1
fi
