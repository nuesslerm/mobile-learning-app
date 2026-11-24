---
status: open
area: architecture
paths:
  - "clis/deploy/Cargo.toml"
  - "clis/harness-android/Cargo.toml"
  - "clis/knowledge/Cargo.toml"
  - "clis/wf/Cargo.toml"
opened: 2026-11-08
last_verified: 2026-11-08
links: []
---
# Nine declared dependencies of the clis crates are never named in their sources

## What is broken

`wf check --unused-deps` (a word scan of each crate's own `.rs` files) lists `deploy: libc`, `deploy: sha2`, `harness-android: rca-core`, `knowledge: base64`, `t3-store: t3-contracts`, `wf: cdp`, `wf: harness-core`, `wf: nix` and `wf: rca-core`. Each one reruns the crate's builds and tests whenever the dependency changes, for nothing.

`wf check --unused-deps` covers every Cargo workspace of the repo, and `wf check --unused-workspace-deps` (part of `wf check --budget`) fails on a declared workspace crate no source names; there is no allowance list (the blast-radius budget and its `allowed_unused` entries were retired on 15 Nov 2026).

## Why blocked

Not blocked. The scan is a candidate list: a dependency used only through a macro, a feature or a registry patch needs a look before it goes.

## What unblocks it

Check each candidate (`cargo tree -i <dep>`, a build without it) and remove the real ones.
