# Gate dependency audit (2026-11-08)

Why a change to one crate under `clis/` pulls other crates into a gate, from `cargo metadata --no-deps --offline` of the `clis` workspace (45 packages) at `dev` 7b79bced5. Reuses what `clis/pipeline/src/blast_radius.rs` and `clis/pipeline/data/blast-radius.json` already measure: the radius is the reverse-dependency closure over normal, build and dev edges. The check covers only the manifests under `clis/` (it reads `clis/**/Cargo.toml`): the `backend/` workspace (rca, vm-ops-api and the other services) and `libraries/agent-api` are not in it, and `arch:services` is their check.

## Reading the data

- **Radius** of X: the crates rebuilt and retested when X changes (X's reverse closure). **Closure** of X: the crates X builds from (forward closure).
- A light crate pulls a heavy one into its gate when it has an edge to a crate with a large closure, because every change below that edge rebuilds the light crate too. The radius of the crate below the edge is what the edge costs.

## The deploy to orca-serve case

There is no cargo path from `deploy` to `orca-serve`. `deploy` has radius 1 (only `wf` depends on it), and `orca-serve` depends on `orca-core`, `orca-crypto`, the nine `orca-map-*` crates and `wf-kit` (`mimic-core` as a dev edge). So a change to `deploy` rebuilds `deploy` and `wf` and nothing of Orca. The way Orca is pulled in is the other direction, through `wf-kit`: its radius is 23, every crate but itself, so a `wf-kit` change rebuilds `orca-serve` and the twelve crates under it. Any observed rebuild of `orca-serve` after a `deploy` change comes from the stage rather than the graph: a stage that runs `cargo test --workspace`-shaped work (a bare clippy or build with no `-p`) builds every member. The Nx cargo tasks name `-p <crate>` and hash the closure of the crate only, so they do not. Not reproducible from the manifests; the sandbox build log of the run is the evidence needed to place it.

## Reverse-dependency closure per crate (radius), largest first

| Crate | Radius | Closure |
| --- | ---: | ---: |
| wf-kit | 22 | 0 |
| cli-http | 20 | 0 |
| wf-testkit | 17 | 0 |
| secrets | 15 | 3 |
| wf-vmops | 13 | 4 |
| blob-cache | 12 | 1 |
| orca-core | 11 | 0 |
| cdp | 10 | 1 |
| wf-bucket | 10 | 6 |
| harness-core | 8 | 8 |
| orca-map-workspace | 6 | 1 |
| harness-android, harness-browser | 5 | 9 |
| mimic-core, t3-contracts | 4 | 0 |
| wf-harness | 4 | 11 |
| harness-flow, t3-core | 3 | 9, 1 |
| offload, orca-map-agent, parity-core, pipeline | 2 | 5, 2, 0, 13 |
| the other 19 crates (deploy, knowledge, the other orca-map-*, parity, t3-http, t3-store, wf-bench, wf-lib, wf-login, agent-config-sync) | 1 | 1 to 14 |

(`wf-kit` shows 22 here and 23 in `blast-radius.json`, which also reads the crates of the separate manifests under `clis/` (`telemetry`, `vnc-auth-listener`) that this workspace metadata does not hold; the JSON is the budget, this table is the metadata count.) The full per-crate numbers, with the test stages each radius reaches, are the output of `wf check --blast-radius`.

## Edges that pull heavy crates into light ones

Measured by removing each edge in turn and summing the shrink of every crate's closure (the larger, the more the edge costs):

| Edge | Closure rows saved if cut | Why it exists | Verdict |
| --- | ---: | --- | --- |
| harness-core -> wf-bucket | 12 | `android_logs.rs` and `test_goldens.rs` read public bucket objects | Listed: move the two readers behind a port that `wf` wires |
| wf-bucket -> blob-cache | 8 | the build cache is one module of wf-bucket | Listed: split the cache out of `wf-bucket` |
| wf-bench -> wf-harness | 7 | reads run reports | Listed: needs `run_report` split out of `wf-harness` |
| deploy -> pipeline | 7 | `verify_tier3` and `build_image` use `pipeline::verify`, `eas`, `image_hash` | Listed: move those modules to a crate both depend on |
| wf-mimic -> wf-bucket | 6 | replay reference storage | Listed |
| pipeline -> wf-harness | 6 | `dead_refs` and `e2e_run` read verb tables and `run_report` | Listed: same split as for `wf-bench` |
| wf-harness -> harness-browser, harness-android | 5 each | the facade re-exports both | Listed: by design, `wf-harness` is the facade |
| harness-{flow,browser,android} -> harness-core | 5, 5, 3 | the shared runner | Kept: the shared core is the point of the split |

Each listed edge needs a code move, not an edge removal, so none is cut here.

## Edges cut now

A declared dependency on a workspace crate whose sources never name it adds its dependant to the radius for nothing. `wf check --unused-deps` finds three (the other candidates are third-party crates or `rca-core`, which is outside `clis/`):

| Edge removed | Evidence |
| --- | --- |
| `t3-store` -> `t3-contracts` | no `t3_contracts` in `t3-serve/store` |
| `wf` -> `cdp` | no `cdp` in `clis/wf/src` or its tests; `wf-login` still carries `cdp` into `wf` |
| `wf` -> `harness-core` | no `harness_core` in `clis/wf`; `harness-flow` still carries it |

`cargo check --tests -p wf -p t3-store` passes without them and `Cargo.lock` loses three lines. Radii do not change (each crate stays reachable another way), so `blast-radius.json` needs no edit. The test that keeps the class from coming back: `a_crate_that_declares_a_workspace_crate_its_sources_never_name_fails_the_check` in `clis/pipeline/tests/flow/blast_radius.rs`; `wf check --blast-radius` (in `wf check --budget` and the pre-commit gate) now fails on any such edge with the crate and the edge named.

## Listed, not cut

- The third-party candidates of `wf check --unused-deps`: `deploy: libc, sha2`, `knowledge: base64`, `wf: nix`. They are not workspace edges, so they do not move a radius; a manifest edit that must be verified with a build of each crate's targets.
- `harness-android: rca-core` and `wf: rca-core`: `rca-core` is outside `clis/`, so the blast-radius check does not see it. Whether the two crates name it through a macro is unchecked.
- The six edges above marked Listed, in order of saving: `harness-core -> wf-bucket`, `wf-bucket -> blob-cache`, `wf-bench`/`pipeline -> wf-harness`, `deploy -> pipeline`.
- `wf-kit`, `cli-http` and `wf-testkit` (radius 22, 20, 17) are leaves every crate legitimately uses; their budgets in `blast-radius.json` already stop growth.
