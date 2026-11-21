# The T3 chain runs the real agent CLIs only through `wf mimic exec`, and the vault holds no credential for them yet

`wf mimic scan` and the `wf-mimic` driver resolve real CLIs (`T3_CHAIN_CLIS`, `MIMIC_REAL_CLI_DIR`), and the rca backends run them in `real-backends.yml`. The T3 integration chain (`clis/t3-serve/tests/integration`) runs the scripted CLIs by default and the real ones when it is started through the launcher.

## What exists

- `wf mimic exec --clis real[:claude,codex,opencode] [--cli-dir <dir>] [--scan <dirs>] -- <command...>` (`clis/mimic/src/exec_verb.rs`) resolves the chosen CLIs' credentials in memory (the vault, or the caller's environment without one), starts the command with the default-deny child environment plus the resolved values, then scans `--scan` and `$WF_TEST_OUTPUT_DIR` for the exact values before anything is published. Flow tests with fake CLIs and a fake vault (the caller's environment): `clis/mimic/tests/flow/real_clis.rs`.
- The rig (`clis/t3-serve/tests/integration/rig.rs`) and the `real_agent_turn` test read only environment variables (`T3_CHAIN_REAL_CLIS`, `MIMIC_REAL_CLI_DIR`, `T3_CHAIN_CLI_HOME`, `T3_CHAIN_EXECUTOR_ENV` and the variables that one names). `t3-serve` depends on neither `wf-mimic` nor `secrets`, so the image closure is as before. Its parse of that environment is flow-tested in `rig.rs`.
- Run it: `wf mimic exec --clis real:claude --cli-dir <dir of claude> -- cargo test -p t3-serve --test integration real_agent_turn`.

## Why it was not in the crate

Resolving from inside the test linked `wf-mimic` (a versioned library, with `secrets` behind it) into `t3-serve`'s cargo closure. The image then had to copy `clis/mimic`, which `no_image_copies_the_source_of_a_versioned_library_except_the_one_that_builds_it` forbids, and every secrets change rebuilt the T3 image.

## Not done, and why

- **One credential per CLI in the vault.** A person with the accounts must store `CLAUDE_CODE_OAUTH_TOKEN` (a Claude Code OAuth token, `claude setup-token`), `CODEX_OAUTH_JSON_B64` (the base64 of a Codex `auth.json`, `wf secret login codex`) and `OPENCODE_GO_API_KEY` (an opencode Go key). The registry already names all three; no value exists to fetch, and none is fabricated.
- **The pinned CLI binaries on the host that runs it.** `claude` 2.1.327, `codex` 0.170.0 and `opencode` come from the `real_agents` provision block of `wf mimic capture record`; the launcher takes their directory as `--cli-dir`.
- **The real run itself.** No real-CLI run has passed `real_agent_turn` (a real turn may not produce the executor's `run_command` event). Only the scripted run is verified.
- **The CI wiring.** The integration job's scope in `job-scopes.json` needs the three keys and a `wf mimic exec` step; that waits on the vault values.
- **Codex write-back.** `exec` reports a rotated login (`codex_login_rotated`, a warning) and does not write it to the vault; `wf secret rotate-login codex` does.
- **Isolation inside the chain.** One executor process runs every real CLI, so each can read the others' environment variables there.
