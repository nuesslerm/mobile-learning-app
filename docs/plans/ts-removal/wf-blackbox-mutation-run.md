# wf black box: clean and seeded-fault runs of both suites

Run on 2026-10-10, before `tooling/wf-blackbox` was removed, against the release `wf` built from the head of `claude/wf-blackbox-rs`.

## Clean binary

| Suite | Result |
| --- | --- |
| Rust (`cargo test -p wf-blackbox`) | 953 passed, 0 failed, 91 suites, 425 s |
| TypeScript (`vitest run`, 4 workers) | 1186 passed, 0 failed (the 947 titles expand to 1186 cases through `it.each`) |

A first TypeScript run on a copy of the binary in another directory failed 21 browser cases. The browser daemon locates the built device-frame harness relative to the binary, so the copy could not find it. The same files passed 131 of 131 against the binary in `clis/target/release`.

## Seeded faults

One edit to `clis/wf/src`, a release build, then the test files that exercise the edited command in each suite. A title counts as the same when the Rust test and the vitest case differ only by the `it.each` expansion.

| Fault | Rust failures | TypeScript failures | Same titles |
| --- | --- | --- | --- |
| `vm logs`: `--since 5m` maps to `HOUR` | 2 | 2 | yes (the `--since %s` loop is one Rust test, one vitest case per row) |
| `work --check`: `duplicate id` becomes `duplicate ids` | 1 | 1 | yes |
| `auth login`: `authenticated ✓` becomes `✔` | 4 | 6 | yes (the per-tool loop is one Rust test, three vitest cases) |
| `browser scroll`: `Scrolled` becomes `Scroll` | 1 | 1 | yes |
| `vm exec`: accepted start status 202 becomes 200 | 10 | 10 | yes |
| `work`: `outside the repo` becomes `outside of the repo` | 1 | 1 | yes |

Every seeded fault was caught by both suites on the same behaviour, and neither suite caught one the other missed.
