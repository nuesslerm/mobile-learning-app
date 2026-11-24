---
title: The real-CLI proof of the Codex live session
date: 2026-11-22
category: testing
tags: [testing, codex, app-server, ci, secrets, real-cli]
problem_type: knowledge
module: backend/remote-chat-agent
---

# The real-CLI proof of the Codex live session

Issue #83, section G4. `CodexSessions` (`backend/remote-chat-agent/cli-driver`, the `SessionDriver` of the service) has run only against recordings and authored conversations (`docs/gaps/codex-live-session-not-run-against-the-real-cli.md`). This proof runs it once against the real `codex app-server` with a real model, on every push to `dev`, without touching the owner's own Codex login.

Type, in the words of `verification-ladder.md`: **e2e** (the real CLI and the real model, nothing doubled; the service's own process adapter `Processes` and the driver are the real ones). It is not deployment verification: no deployed service is involved.

## What the test does

`backend/remote-chat-agent/host/tests/real_codex_session.rs` (`#[ignore]`, so no ordinary `cargo test` runs it):

1. Starts `CodexSessions` over `Processes::at(<providers root>)`, so the environment of `codex app-server` is the one production builds (`provider_env`: `CODEX_HOME` is `<root>/codex`, the pinned binary first on `PATH`, nothing else of the caller's).
2. Turn 1 on a new thread: `Reply with the single word ok. Use no tool.` Asserts the neutral events: the session id is announced, the text contains `ok`, the turn ends `Done` (no `Error`). An approval it asks for is declined, so nothing waits for nobody.
3. `close` of the workspace (the server and everything it started end).
4. Turn 2 on the same thread with the announced id as `resume` (a new process, `thread/resume`): asks which word it replied with before. Asserts no new session is announced, the text contains `ok` (the conversation survived), `Done`.
5. Closes the session again. Every turn has a hard limit of 150 seconds; the CI step has 25 minutes.
6. With `RCA_REAL_RESULT_OUT` set it writes the shape of the run (events per turn, never the reply).

Without `CODEX_HOME` pointing at a directory named `codex` that holds an `auth.json`, the test prints `skipped: ...` and passes, so a stray `--ignored` run costs nothing.

## The wiring: `wf mimic exec --clis real:codex-ci`

The test never touches the vault. The launcher does (`clis/mimic/src/exec_verb.rs`, `real_clis.rs`):

- Reads `CODEX_CI_OAUTH_JSON_B64` in memory (`secrets::load_values`), decodes it to `<private home>/providers/codex/auth.json`: directory 0700, file 0600, and `CODEX_HOME` names that directory. The command gets the default-deny child environment (never `BWS_*`, never the login as a variable) and `MIMIC_REAL_CLI_DIR` (the pinned binary).
- `--skip-when-empty`: a missing or blank secret skips the run: exit 0, `{"ran": false, "skipped": "<KEY> holds no value ... wf secret login codex-ci"}`, the message also on stderr, the command never starts. Without the flag it fails with exit 2.
- After the command (whatever its result): when `auth.json` changed, `secrets::rotate_login` writes it to that one vault key with the CI deploy credential, which is what `wf secret rotate-login codex-ci --from <file>` runs: a file that is no ChatGPT login or one of another account is refused, an unchanged refresh token writes nothing. A refused or failed write-back is a failed run (`login_written_back` says `refused` or `failed`, and the warning names `wf secret login codex-ci`), because the vault's refresh token is dead once Codex has used it.
- Scans what the run wrote (`--scan`, `$WF_TEST_OUTPUT_DIR`) and the command's own output for the login's token fields, withholding a hit; the JSON result names files and keys, never a value.
- `codex-ci` is named on its own (`real` is still claude, codex, opencode) and cannot be named with `codex` (both set `CODEX_HOME`).

`wf secret update ... --apply` is not the write-back in CI: it needs `BWS_ADMIN_ACCESS_TOKEN`, which never goes into GitHub (`job-secret-needs.md`). The CI deploy credential reads and writes the CI project, and `rotate-login` is the writer that takes it.

## The CI job

`pipeline.yml` job `codex-session-proof`, in the backend build and deploy workflow's run (`stages: deploy`), after `integration-tests` (it runs when that passed or was skipped):

- Only `github.event_name == 'push'` on `refs/heads/dev` of a repository that is not a fork, at the head of dev (`plan.outputs.is_head`), when the service stage runs; the repository variable `CODEX_CI_PROOF=off` switches it off. No pull request, dispatch or fork path reaches it.
- `concurrency: group: codex-ci-login, cancel-in-progress: false`: one job holds the login at a time across every run; a pending job is replaced by a newer one (it ends cancelled, never started), a running one is never cancelled.
- Keys: `CODEX_CI_OAUTH_JSON_B64` plus the usage pair (`job-scopes.json`, `pipeline.yml:codex-session-proof`, scope `ci-deploy`), read only by the verb in the one step that holds the vault token.
- The pinned binary comes from `tooling/real-backends/run_leg.py --install codex --into <dir>` (the sha256 of `tooling/mimic-drivers/provision/real-agents-pin.json`).
- The model: the repository variable `CODEX_CI_MODEL` (read as `WF_CODEX_CI_MODEL`); empty means the account's default. Set it to the cheapest model the account lists. No model name is in the code.
- Output: the step summary (the skip reason, or exit, write-back outcome and the tail of the log), the artifact `codex-session-proof` (the result shape, 14 days).

## Run it by hand against a stub

The flow tests of the wiring (`clis/mimic/tests/flow/real_clis.rs`, `clis/secrets/tests/flow/secrets.rs`) run with `sh -c` as the command and a `bws` stub, never a real login: the login restored to `providers/codex/auth.json` with 0600/0700, the write-back only when the file changed (one `bws secret edit` of that key, none otherwise), a refused write-back failing the run, no refresh token in any printed line, the run skipped with the reason when the secret is empty. The test itself was run through the whole chain with a scripted `codex` on `PATH` (an authored app-server, not committed):

```sh
CODEX_CI_OAUTH_JSON_B64=<base64 of an auth.json> wf mimic exec --clis real:codex-ci --cli-dir <dir with a codex> --skip-when-empty -- \
  cargo test --locked -p remote-chat-agent-host --test real_codex_session --manifest-path backend/remote-chat-agent/Cargo.toml -- --ignored --nocapture
```

## When it goes red

- `skipped` in the summary: the vault holds no value: `wf secret login codex-ci` (a device-link login in a private `CODEX_HOME`).
- `login_written_back: refused` or `failed`: the refreshed login did not reach the vault, so the vault's one is dead: `wf secret login codex-ci` again.
- The turn fails with an error naming an expired login: the same.
- A request of this client refused by the real server, or an event the driver does not translate: that is the finding this proof exists for; the first bullets of the gap file say what to compare.
