# clis/agent-config-sync: the last hand-written system call

Date: 2026-10-09. Plan: `docs/plans/rust-services/standard-crates.md` (phase 4). Branch `claude/rca-serde-errors` (PR #339 stack).

## Scope

The survey of this crate found one piece of infrastructure written by hand: an `extern "C"` declaration of `getuid`, used only to name the user id in the "checkout is not writable" message. It now calls `nix::unistd::getuid`, the crate `wf` already uses for its system calls (feature `user` only). Everything else the crate does is its own product logic (the contract and the renderers) over `serde_json`, `regex` and `toml`.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| `extern "C"` declarations in `clis/` | 1 | 0 |
| Direct dependencies | 3 | 4 (`nix`, already in the lockfile through `wf`) |

## Tests, by tier

- Unit and parity (`cargo test -p agent-config-sync`): 9 parity tests against the generated agent config, passing. `cargo clippy --all-targets -- -D warnings` clean.
- The "not writable by uid" branch is not covered by a test: it needs a checkout owned by another user.

## Deliberate behaviour changes

- None; the message carries the same user id.

## Deviations from the instructions

- None.

## Known gaps

- `fsx.rs` maps raw OS error numbers to Node's errno names by hand so a text match written for the TypeScript implementation still holds; `std::io::ErrorKind` does not carry the names, and the matcher is the parity contract, so it stays.

## 2026-10-09: its command line on clap (standard-crates plan, phase 4c)

`cli.rs` read its mode with `args.iter().any(|a| a == "mcp-guard")` and `"--check"`, ignoring every other word. It now parses `[sync] [--check]` and `mcp-guard` with `clap` (the builder API, help and version off; `wf agents` and the standalone binary share the function). Anything else is clap's error on stderr with exit 2 (before, an unknown word was ignored and the sync ran); a `mcp-guard` hook given a stray argument now fails loud, as the hook's own failure policy already wants. Tests: two unit tests in `cli.rs` (the three shapes, a refused shape named by clap), the nine parity tests and the `agents-caveat` black-box file unchanged and green, and `pnpm run sync:check` (`wf agents sync --check`) passes. Dependency added: `clap`, default features off. Deviation: none. Known gap: none.
