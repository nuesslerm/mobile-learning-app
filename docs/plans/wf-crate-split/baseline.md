# wf crate split: build baseline (W0a)

Measured on the unchanged tree, commit `cde8a6d6e` (clis tree `d540dfd0e822d57028f6cfba67144b606dd31a0a`), 23 Oct 2026, for the
stop rule of `plan.md` section 7. Compare W2 against these rows; n=1 per cell unless two samples are listed.

Method: rustc/cargo 1.97.0, `--locked --offline`, `CARGO_INCREMENTAL=0`, `WF_BUILD_SHA` set to the tree hash as `install-wf` does.
The sandbox shows 4 CPUs and 16 GB with no swap; every build ran under `taskset -c 0,1` (2 CPUs, as CI's 2 OCPUs), the sampler on
CPUs 2 and 3. Peak RSS is the sum over the process tree sampled every 0.2 s (as the 2026-10-12 decision-log entry did, so a lower bound).

| Build | Command | Wall | Peak RSS (tree) | Output |
| --- | --- | --- | --- | --- |
| Deploy, cold | `cargo build --profile deploy -p wf` in a fresh scratch `CARGO_TARGET_DIR` | **205.2 s** (user 337 s) | 1130 MB | `deploy/wf` 15,973,528 B (15.2 MiB); target dir 533 MB |
| Deploy, after `touch clis/wf/src/ci_comment.rs` | same, `CARGO_INCREMENTAL=0` | **47.0 s** (user 72 s) | 1130 MB | `deploy/wf` 15,973,528 B |
| Deploy, incremental as `install-wf` sets it | `CARGO_PROFILE_DEPLOY_INCREMENTAL=true`, `CARGO_INCREMENTAL` unset; first build, then after `touch` twice | 57.1 s, then 3.3 s and 3.1 s | 1128 MB, then 723 and 766 MB | `deploy/wf` 15,970,496 B |
| Debug, after `touch clis/wf/src/ci_comment.rs` | `cargo build -p wf` in `clis/target` | **22.8 s** (also 24.5 s) | 1788 MB | `target/debug/wf` 257,963,144 B (246.0 MiB) |
| Debug, same touch | `cargo test --no-run -p wf --test flow` | **23.8 s** (also 25.1 s) | 1728 MB | `deps/flow-061b6ee200373db3` 113,973,288 B (108.7 MiB) |
| Debug, after `touch clis/wf/tests/flow/ci_comment.rs` | `cargo test --no-run -p wf --test flow` | 15.1 s | 970 MB | same flow binary, relinked |

Free disk on `/` was 3.22 GB before and after (3,224,328 KB, then 3,223,476 KB); the scratch target dir is deleted; `clis/target` is 4.5 GB.

## Reading the numbers

- Cold deploy is 205 s, not the 181 s of the 2026-10-12 entry (the tree grew since). Stop rule at 25 percent over this baseline:
  **256 s**. Over the old 181 s it would be 226 s, only 21 s above today's tree; use the measured baseline.
- The absolute 29 s incremental limit is already exceeded by the unchanged tree: 47.0 s, because with `CARGO_INCREMENTAL=0` touching
  one file recompiles the whole `wf` crate (16 codegen units, opt-level z) and relinks. A relative rule is 47.0 s x 1.25 = **59 s**.
  This needs a ruling before W2; the 29 s figure came from another run of the same command and may have been a faster moment.
- `install-wf` does not use `CARGO_INCREMENTAL=0`: its action sets `CARGO_PROFILE_DEPLOY_INCREMENTAL=true`. A touch without an
  edit is then a best case (3 s: rustc reuses everything and relinks); a real edit sits between that and 47 s. CI's true figure for a
  `ci_comment.rs` edit was not measured here (no source change in this unit).
- Touching a `src` file does not recompile or relink the flow test binary (its mtime stayed put); `test --no-run` after the touch
  costs the same as the bin build. Only a change under `tests/` recompiles the flow crate (15.1 s).
- The two debug samples of each row are the same command; the first followed the deploy runs' touch of `ci_comment.rs`, so both
  recompiled `wf`.
- Not measured: the musl target `install-wf` builds with (not installed here; the 2026-10-12 entry has 2m08s cold, 17 s wf-only),
  and the loaded VM (3.5 min for the old incremental build there).

## Split tree (validation, 24 Oct 2026)

Measured on the working tree after the split (commit `cde8a6d6e` plus every unit of the split, uncommitted; `clis` = `wf` and 13 new crates plus the
six existing ones), same toolchain, same method: `--locked --offline`, `CARGO_INCREMENTAL=0`, `WF_BUILD_SHA` set to the tree hash of
`HEAD:clis` (a value the build only embeds), every build under `taskset -c 0,1`, the sampler (peak RSS summed over the process tree every
0.2 s) on CPUs 2 and 3, the deploy builds in a fresh scratch `CARGO_TARGET_DIR` that is deleted afterwards. Cold n=1, every other cell two
samples. A `touch` marks the file changed without editing it, as in the baseline above.

| Build | Command | Wall | Peak RSS (tree) | Output |
| --- | --- | --- | --- | --- |
| Deploy, cold | `cargo build --profile deploy -p wf` in a fresh scratch target dir | **191.6 s** (user 330.7 s); baseline 205.2 s | 908 MB (baseline 1130) | `deploy/wf` 16,981,928 B (16.2 MiB; baseline 15,973,528, +6.3%); target dir 592 MB (baseline 533) |
| Deploy, after `touch clis/pipeline/src/ci_comment.rs` (the leaf) | same | **8.7 s**, 8.7 s (user 12.8 s); baseline 47.0 s | 537 MB | same binary size |
| Deploy, after `touch clis/wf-harness/src/run.rs` (the hottest crate) | same | **24.7 s**, 25.1 s (user 39 s) | 914 MB | same |
| Deploy, after `touch clis/wf-kit/src/args.rs` (every crate rebuilds) | same | **43.6 s**, 45.6 s (user 77 s) | 917 MB | same |
| Debug, after `touch clis/pipeline/src/ci_comment.rs` | `cargo build -p wf` in `clis/target` | **4.3 s**, 4.2 s; baseline 22.8 s | 930 MB (baseline 1788) | `target/debug/wf` 122,192,744 B (116.5 MiB) |
| Debug, same touch | `cargo test --no-run -p pipeline --test flow` | **1.8 s**, 1.8 s | 530 MB | flow binary 20,233,336 B (19.3 MiB) |
| Debug, same touch | `cargo test --no-run -p wf --test flow` (the baseline's command) | **4.9 s**; baseline 23.8 s | 1078 MB | flow binary 51,868,192 B (49.5 MiB; baseline 108.7 MiB) |
| Debug, after `touch clis/pipeline/tests/flow/ci_comment.rs` | `cargo test --no-run -p pipeline --test flow` | 1.8 s (baseline row of `wf`: 15.1 s) | 501 MB | same flow binary, relinked |
| Debug, after `touch clis/wf-harness/src/run.rs` | `cargo build -p wf`, then `cargo test --no-run -p wf-harness --test flow` | 10.5 s, then 11.1 s | 898, 741 MB | flow binary 55,980,352 B (53.4 MiB) |
| Debug, after `touch clis/wf-kit/src/args.rs` | `cargo build -p wf`, then `cargo test --no-run --workspace` | 16.0 s, then 29.6 s | 901, 1165 MB | every test binary of the workspace rebuilt |

Sizes in `clis/target/debug`: the 12 flow test binaries of the wf crates add up to 402.5 MiB (largest `wf-harness` 53.4, `wf` 49.5,
`wf-bench` 40.8 MiB; the single flow binary of the baseline was 108.7 MiB) and the 11 thin binaries to 504.4 MiB (`wf-harness` 71.0,
`wf-bench` 62.7, `wf-login` 62.1, `wf-plan` 50.7 MiB, `wf-secrets` 19.5 MiB). The debug rows are not like for like with the baseline:
this tree sets `[profile.dev] debug = "line-tables-only"` in `clis/Cargo.toml` (the baseline had full debuginfo, 246 MiB for `wf`).
Free disk on `/` ended at 2,449,004 KB (the baseline's 3.2 GB; `clis/target` 4,987 MB after deleting 1.3 GB of superseded test and thin
binaries of earlier hashes, which the shared debug dir had accumulated across the validation runs).

Not measured (as in the baseline): the musl target `install-wf` builds with, the persistent incremental deploy build of CI
(`CARGO_PROFILE_DEPLOY_INCREMENTAL=true`), and the loaded VM.

### Ruling against the stop rule

- Cold deploy build at most 25 percent above 205.2 s, that is 256.5 s: measured 191.6 s, 6.6 percent below the baseline. Passes.
- Incremental rebuild of a leaf edit not above the 47.0 s baseline: measured 8.7 s after touching `ci_comment.rs`, 81 percent below.
  Passes (the plan's original absolute limit of 29 s is also met). The worst case, an edit to `wf-kit` that rebuilds every crate, is
  43.6 s and 45.6 s, still under 47.0 s.

The stop rule does not halt the split. The cost that went up is disk: the deploy binary is 6.3 percent larger and the target dir 11 percent
larger, and each of the 12 crates with tests adds a flow binary and a thin binary to the debug dir.
