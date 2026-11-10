---
title: "wf secret sync-env redacts stdout by default — an agent printed live secrets into a session transcript by running it bare"
date: 2026-07-30
category: security
module: clis/secrets
problem_type: security_issue
component: tooling
severity: high
symptoms:
  - "Running `node wf-bundle.cjs secrets sync-env --for <app> --backend bitwarden --base .env.defaults` with no --out prints every resolved secret value for that app in plaintext to stdout"
  - "An interactive/agent shell capturing that stdout (a Bash tool, a terminal multiplexer log, a session transcript) now holds live credentials in cleartext"
  - "The command's own description ('print/write a .env') gives no indication that the print path dumps real secret values, not a masked preview"
root_cause: config_error
resolution_type: code_fix
related_components:
  - infra/oracle-cloud/vm-ops-api
  - backend/remote-chat-agent
  - backend/leetcode-backend
  - backend/leetcode-auth-cookie-capture
tags: [security, secrets, sync-env, redaction, stdout, wf-cli, agent-safety]
---

# wf secret sync-env redacts stdout by default — an agent printed live secrets into a session transcript by running it bare

## Problem

While debugging why a first-time app deploy failed on an Oracle Cloud VM, an agent
session ran `node wf-bundle.cjs secrets sync-env --for remote-chat-agent --backend
bitwarden --base .env.defaults` directly over SSH, with no `--out` flag and no shell
redirection, to see what the command would do. `sync-env` resolved the full
`remote-chat-agent` secret group and printed every value — `TS_VM_AUTH_KEY`,
`GITHUB_TOKEN`, `CHAT_AGENT_API_SECRET`, `TELEGRAM_BOT_TOKEN`, `OPENCODE_GO_API_KEY`,
`MINIMAX_API_KEY`, `KIMI_API_KEY` — in plaintext to stdout, which the agent's shell
tool captured into the running session transcript.

## Symptoms

Running `sync-env` (or the `wf secret sync-env` alias) with no `--out` argument
prints `ENV_VAR="real-secret-value"` lines for every resolved key to stdout. There is
no warning, no confirmation prompt, and nothing in the command's own `--help` output
that flags this as more dangerous than, say, `wf secret list` (which already masks
values by design).

## What Didn't Work

- **Assuming `sync-env`'s default behavior mirrored `list`'s.** `wf secret list`
  already shows masked values (`list_statuses`, behind `cmd_list` in
  `clis/secrets/src/secrets/mod.rs`, fills a `masked` field per entry with `mask()`) —
  it was reasonable to assume `sync-env` followed the same convention. It didn't:
  `sync-env`'s CLI handler (then TypeScript; today `cmd_sync_env` in
  `clis/secrets/src/secrets/mod.rs`, with the resolve step in
  `clis/secrets/src/secrets/sync_env.rs`) had exactly two paths — write real values
  to `--out <file>`, or print real values to stdout — no masked/preview mode
  existed at all.
- **Treating "just checking a command" as risk-free.** Unlike `wf secret get <KEY>`
  (single value, and its entire purpose is command substitution —
  `TOKEN=$(wf secret get X)`, used throughout this repo's own deploy scripts and
  this session), `sync-env --for <app>` resolves an entire app's secret *set* in one
  call. A single accidental invocation exposes every credential that app depends on,
  not one.

## Solution

Audited every `wf secret *` subcommand for whether its default output exposes real
secret values, and whether anything in this repo actually depends on capturing that
output for automation:

| Command | Default stdout behavior | Change |
|---|---|---|
| `list` | Already masked (`s.masked`, `mask()` in `clis/secrets/src/secrets/util.rs`) | None — already safe |
| `get <KEY>` | Prints the real single value — by design, since `$(wf secret get X)` command substitution is the primary, repo-wide usage pattern | None — narrowing this would break real scripts |
| `env` | Prints all real values — by design, documented as "eval-safe" (`eval "$(wf secret env)"` is the only way to use it) | None — the eval contract requires real values on stdout |
| `sync-env` | Printed all real values for a whole app group when `--out` was omitted, with **no real caller in this repo relying on that path** | **Redact by default; require `--out` or an explicit `--reveal` flag for real values** |

`sync-env` was the one outlier: a `grep -rn "sync-env"` across every `deploy.sh`,
`cloud-init.sh.tftpl`, and the vm-ops-api first-time-provisioning script
(today built in `backend/vm-ops-api/core/src/application/scripts/redeploy.rs`)
showed every single real call site
already used either `--out <file>` or shell redirection (`>> .env`) — nothing reads
its bare stdout programmatically. That made the fix safe: redacting the no-`--out`
path can't break any existing automation, only the ad hoc "let me see what this
does" case that caused the incident.

Extracted the existing `mask()` helper from the `list` command into one shared
helper (today `mask()` in `clis/secrets/src/secrets/util.rs`), then changed the
`sync-env` CLI handler (then TypeScript; today `cmd_sync_env` in
`clis/secrets/src/secrets/mod.rs`) from:

```ts
if (opts.out) {
  writeFileSync(opts.out, envContent);
  process.stdout.write(`Wrote ${result.resolved.length} secret(s)... to ${opts.out}\n`);
} else {
  process.stdout.write(envContent);
}
```

to:

```ts
if (opts.out) {
  writeFileSync(opts.out, envContent);
  process.stdout.write(`Wrote ${result.resolved.length} secret(s)... to ${opts.out}\n`);
} else if (opts.reveal) {
  process.stdout.write(envContent);
} else {
  const redacted = { ...result, resolved: result.resolved.map(({ envVar, value }) => ({ envVar, value: mask(value) })) };
  const redactedContent = basePath
    ? composeDotEnv(parseDotenv(readFileSync(basePath, 'utf8')), redacted).content
    : formatDotEnv(redacted);
  process.stderr.write('No --out given -- printing a REDACTED preview ...\n');
  process.stdout.write(redactedContent);
}
```

`--reveal` is the explicit, self-documenting, grep-able opt-out for the rare
legitimate case (a human at a real terminal who genuinely wants to eyeball a
resolved value) — `writeFileSync`'s `--out` path is untouched and remains the way
real values reach disk.

Then migrated every real call site from the old `: > .env; sync-env ... >> .env`
shell-redirection pattern to the equivalent, safer `sync-env ... --out .env`
(`writeFileSync` overwrites in one step, same effect as truncate-then-append, but
the value now never touches the parent shell's stdout stream at all):
`infra/oracle-cloud/vm-ops-api/deploy.sh`, the `remote-chat-agent` and
`opencode-telegram-bot` deploy scripts (both since removed: `remote-chat-agent`
was rewritten in Rust and the Telegram bot was deleted),
`backend/leetcode-auth-cookie-capture/scripts/deploy.sh`,
`backend/leetcode-backend/scripts/deploy.sh`, and the sibling-container
first-time-provisioning script (today built by `app_redeploy_sibling_args` in
`backend/vm-ops-api/core/src/application/scripts/redeploy.rs`).

## Why This Works

`--out <file>` was already implemented as a direct `writeFileSync` from inside the
Node process — the secret value is never written to the process's stdout file
descriptor at all, so no supervising shell, pipe, log collector, or agent tool
wrapper can observe it in transit. The old `>> .env` shell-redirection pattern
achieved the same *end state* (a file with real values) but did so by letting the
value pass through stdout first and relying on the parent shell to redirect it away
— safe when a human or a script deliberately adds `>>`, but silently unsafe the
moment someone runs the bare command to "just check" something, exactly as happened
here. Making the no-`--out` default redact removes that ambient risk without
touching the one path (`--out`) that was always the genuinely safe one.

## Prevention

- **When adding a new `wf secret` subcommand (or any CLI command that touches
  secret values), ask explicitly: does anything in this repo rely on capturing this
  command's raw stdout for automation?** If not — and especially if the command can
  resolve *multiple* secrets in one call — default to a redacted/masked preview and
  require an explicit flag (`--out`, `--reveal`, or similar) for real values. `get`
  and `env` are correctly exempted here because their real, load-bearing usage
  pattern (`$(wf secret get X)`, `eval "$(wf secret env)"`) *requires* real values on stdout; a command doesn't earn that exemption just by
  resembling those two.
- **Before running any secrets-related command interactively "just to check," default
  to redirecting or piping through something that can't leak** (`> /dev/null`,
  `| head -c 0`, or simply not running it bare) rather than trusting the command's
  default behavior to be safe.
- **Grep for the unsafe pattern when reviewing a PR that touches a `deploy.sh` or
  provisioning script**: `grep -rn "sync-env" --include='*.sh'` and confirm every hit
  has `--out` or `--reveal` — a bare `sync-env --for <app>` with neither is now a
  code-review flag, not just a runtime redaction.
- Added one shared masking implementation, today `mask()` in
  `clis/secrets/src/secrets/util.rs` — reuse it rather than reimplementing ad hoc
  truncation/asterisk logic in a new command.
- **Regression coverage now exists at the CLI-process level, not just the
  use-case level.** The `sync-env` flow tests in
  `clis/secrets/tests/flow/secrets.rs` spawn the real, built `secrets` binary (the thin `wf secret` binary, via
  `run_wf` in `clis/wf-testkit/src/run.rs`) with a fake secret value
  served by a stub `bws` on `PATH` (the Bitwarden backend's CLI), so no real
  macOS Keychain or Bitwarden access happens. The tests assert all three
  `sync-env` branches: no flags → stdout redacted, `--reveal` → stdout has
  the real value, `--out <file>` → the file has the real value and stdout
  has none of it. Any future regression that reintroduces an
  unredacted-by-default `sync-env` will fail this test immediately, rather
  than waiting to be caught by manual inspection during a debugging session
  (which is exactly how the original incident happened). `get`'s equivalent
  coverage lives in the same file, against the same stub, so it also covers
  the "resolves to nothing" path (a key absent from the vault exits 1 with
  `not found`) that the old TypeScript tests could only reach with a fake
  backend in a unit test.

## Related

- `docs/solutions/security/keychain-secrets-not-env.md` — the broader `.env`/secrets
  architecture this fix operates within (registry, backends, no-soft-failures
  contract). This doc's fix doesn't change any of that; it only changes what
  `sync-env` shows on stdout when `--out` is omitted.
