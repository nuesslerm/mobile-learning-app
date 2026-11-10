# libraries/hook-runtime and scripts/guards: the approve guard's secret rule

Date: 2026-10-09. Plan: `docs/plans/rust-services/standard-crates.md` (the goal's verification phase). Decision log: "the approve guard stops refusing a fetch beside other commands, and the authenticated probes on the VM". Branch `claude/guard-secret-fetch-narrow` (PR #342), landed on `dev`. Existing package, one data file changed (`src/deny.json`); the script it runs lives at `scripts/guards/block-secret-exposure.mjs`.

## Scope

The condition script behind the secret rules of `deny.json` refused any Bash call in which a secret fetch (`wf secrets get|env`, `bws secret get|list`, `security find-generic-password`) and any dump word or file redirect appeared anywhere in the same command string, so a fetch next to a `head` of another file, or `curl ... | head` of a response, was refused and an agent stopped instead of splitting the call. On the owner's instruction it now splits the command into list items and pipeline stages (outside quotes, substitutions, groups and backticks) and refuses only when the fetched value itself reaches a leak shape: the fetch pipeline continuing into a dump word, `tee`, `xargs` or a network tool; the fetch, or an `echo`/`printf` of a captured variable, redirected to a real file; a dump word or `tee` handed the value by a here-string, process substitution or captured variable (a network tool by a here-string); after `eval "$(wf secrets env)"`, an environment listing or a secret-named variable printed into one of those sinks. A command the scanner cannot parse, or any scanner failure, takes the old coarse rule. The reasons of rules 3 to 6, 8 and 9 say what is still refused.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| A fetch beside `head README.md` in the same call | refused | allowed |
| `TOK=$(...); curl -H "Bearer $TOK" url \| head -c 200` | refused (a dump word anywhere) | allowed |
| `wf secrets get K > f`, `wf secrets get K \| head`, `... \| curl -d @-` | refused | refused |
| `T=$(...); printf %s "$T" \| base64`, `cat <(wf secrets get K)`, `echo "$T" > f` | refused only through the coarse rule | refused by shape |
| Guard unit tests | 11 | 47 |

## Tests, by tier

- Unit (`scripts/guards/block-secret-exposure.test.mjs`): 47 tests, written first: the nine that describe the newly allowed shapes were red on the old script (the new leak-shape tests already passed through its coarse rule and pin that they still do). `libraries/hook-runtime` 140 of 140.
- Integration (`tooling/wf-blackbox` `secrets-guard-coverage.integration.test.ts`, 4 tests): the script's subcommand list, the deny list and the script's behaviour per value-revealing subcommand stay in step (a redirect is refused, an unrelated dump beside a discarded fetch is not).
- E2E: the guard ran on the real Bash calls of the authenticated VM probes that it had blocked (`TOK=$(...)` handed to a probe through its environment, with other commands in the same call), and those calls were allowed.

## Deliberate behaviour changes

- A dump word beside an unrelated fetch is no longer refused. Every shape that has leaked a value (the `BWS_DEPLOY_SECRETS_EDIT_ACCESS_TOKEN` redirect, the piped dump, the piped network tool) still is.

## Deviations from the instructions

- The guard is a tripwire for the known shapes, not a sandbox: a value printed deliberately, or moved through a construct the scanner does not model (a `while read` loop, a `{ ...; }` group), is out of its reach, so "never print, echo, redirect or write a secret value" still holds on its own (documented in `docs/solutions/security/wf-secrets-default-backend-and-redaction-pitfalls.md`).

## Known gaps

- `bws secret edit|create` are in the deny list but not sources of the script, as before; they were not part of this change.
