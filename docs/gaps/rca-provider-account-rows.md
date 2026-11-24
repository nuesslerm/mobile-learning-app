---
status: open
area: rca-parity
paths:
  - "clis/rca-exec/src/adapters/provider_account.rs"
  - "libraries/agent-api/exec-proc/**"
  - "backend/remote-chat-agent/group-accounts/src/**"
  - "libraries/agent-api/agent-api-accounts/**"
  - "clis/orca-serve/map-accounts/src/**"
  - "clis/t3-serve/core/src/usecases/prepare.rs"
  - "clis/t3-serve/core/src/usecases/triage.rs"
opened: 2026-11-20
last_verified: 2026-11-20
links:
  - "docs/plans/rca-parity/plan.md"
  - "docs/plans/rca-offer-more/plan.md"
  - "docs/gaps/orca-github-project-rows.md"
---
# The provider-account rows and the T3 worktree launch rest on authored doubles and name what they do not do

## What is broken

Issue #83 O2 serves the providers' own accounts at the workspace host (`cli_update`, `codex_app_server_request`, `claude_account_request`), the Orca rows over
them, the T3 launch on a new worktree with its preparation, and the T3 triage bundle. None of it has a recording of the real provider behind it.

1. **Every answer of the three executor actions in the tests is an authored double.** The scripted `codex app-server` (the shell script in
   `clis/rca-exec/tests/flow/provider_account.rs` and the `app_server` answers of the exec-testkit scripted CLI), the `update` commands of the scripted `claude` and
   `opencode`, and the local HTTP server that stands for Claude's usage service were written from Orca's source (`account/read`, `account/rateLimits/read`, the
   `/api/oauth/usage` request and its `five_hour` and `seven_day` windows), never recorded from a real Codex or Claude binary (none is in the sandbox). The method
   name the executor sends to spend a reset credit, `account/rateLimitResetCredit/consume`, is **not verified against a Codex binary**: Orca spends a credit through
   chatgpt.com's backend, not the app-server, so the name is this repository's guess and the scripted app-server is the only thing that answers it. The accepted
   spellings of the outcome (`reset`, `nothing_to_reset`, `no_credit`, `already_redeemed`) are Orca's backend codes. `codex` has no update command of its own and is
   refused; `claude update` and `opencode upgrade` are written from the CLIs' documentation and were not run.
2. **Orca's spend row does not compare `expectedScope`.** rca keeps no account or offer revision, so `accounts.consumeCodexResetCredit` checks the scope's shape and
   its account and relies on the idempotency key (the key goes to the account) to keep one credit from being spent twice. The recorded host answered only validation
   errors for it (every recorded key is no uuid), so the success shape (`{outcome, scope, snapshot}`) is written from Orca's source. A WSL target is refused.
3. **The Orca host sign-in rows have one recorded success shape.** `hostSignInStart` answers `{flowId, provider, revision: 1, phase: "starting"}` as recorded; the
   waiting, verifying, signed-in, failed and cancelled statuses, the revision counting and the 20 s wait are written from `host-account-sign-in-flows.ts`. The Claude
   rows `beginClaudeSignIn`, `finishClaudeSignIn` and `cancelClaudeSignIn` are the `orca` CLI's rows (neither the phone's nor the desktop's) and open `claude auth login`
   into a managed config directory rca does not have: they are **not registered** (the table test holds every served row to a phone or desktop row), and the host
   sign-in rows (`desktop: true` in the scan) are the way a Claude login is started from a client. The limits `resetDescription` is always null (the host formats a local time string).
4. **The T3 worktree launch is narrower than the pin's.** A launch on a new worktree needs its first message (the worktree is prepared for the run it starts; a launch
   without one is refused as a launch error). `startFromOrigin` is accepted and not acted on (no fetch of the remote base). The setup script is the one `worktree.create`
   starts; a failed preparation removes the worktree it made (the branch stays) and forgets it on the thread, so a retry makes it again. A server restart makes a
   preparing run's preparation again. The queue of messages held behind a failed preparation is not released by the retry.
5. **`server.prepareTriage` writes a bundle of its own.** The prompt is the pin's (`Read <triage-playbook.md path> and follow it. `); the playbook and the log file are
   this repository's text. The log is the rca service's own `application::log` events kept in memory (the newest 2000, lost on a restart), not the T3 install's logs,
   and no third-party source is cloned (owner decision 7).
6. **`server.updateProvider` and the T3 provider credit row keep their old behaviour for the update.** `provider.update` exists at the service, but no T3 success shape
   for an update was recorded, so `server.updateProvider` still answers its declared error. `provider.consumeResetCredit` of T3 maps the service's outcome (`reset` is a
   reset, any other outcome is a `UsageLimitSourceError`).

## What would close it

A real Codex and a real Claude signed in at the executor in a hosted run (R11 and R16), recorded for `account/rateLimits/read`, the credit spend and the usage read;
the owner's Codex login and a credit for the spend; a desktop Orca client for the Claude sign-in rows (O5); a recording of a T3 launch on a worktree and of
`server.updateProvider`.
