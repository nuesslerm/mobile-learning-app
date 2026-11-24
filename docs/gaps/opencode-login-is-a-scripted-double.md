---
status: open
area: rca-parity
paths:
  - "clis/rca/tests/flow/login.rs"
  - "libraries/agent-api/exec-proc/src/lib.rs"
  - "backend/remote-chat-agent/cli-driver/src/domain/login.rs"
opened: 2026-11-20
last_verified: 2026-11-20
links:
  - "docs/gaps/exec-testkit-recordings-not-from-real-clis.md"
---
# opencode's login is driven as typed lines; the flow test uses a scripted double, not the real picker

Issue #83, G1. The implementation drives opencode's sign-in by the `paste` method, each answer one `provider.auth.respond` line (the provider, then the API key), never by driving its provider-picker TUI.

## What the test proves

`clis/rca/tests/flow/login.rs` starts `opencode auth login` through `wf rca login` against the real service and `rca-exec` with a scripted opencode that prints an address, asks `Provider:` and `API key:` as plain line prompts, and writes `~/.local/share/opencode/auth.json`. It proves the two answers reach the CLI's input, the credential lands in the executor's home, and no reply carries the key.

## What it does not prove

The scripted prompts are authored, not recorded. Nothing here has run the real `opencode auth login`, so how it behaves on piped stdin is unknown: it may open a TUI picker that needs a terminal, and `opencode auth login <provider>` or a key placed in its auth file by the executor may be the working form. Both are unverified guesses.

## What would close it

Run the pinned opencode through `login_start` at a host with a pty or the per-provider form, record the prompts, and replace the scripted lines with that recording. Needs a person's provider key.
