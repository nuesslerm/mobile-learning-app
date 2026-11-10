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

`wf check --unused-deps` (a word scan of each crate's own `.rs` files) lists `deploy: libc`, `deploy: sha2`, `harness-android: rca-core`, `knowledge: base64`, `t3-store: t3-contracts`, `wf: cdp`, `wf: harness-core`, `wf: nix` and `wf: rca-core`. Each one widens a crate's blast radius (`wf check --blast-radius`) and its build for nothing.

`wf check --unused-deps` and the blast-radius gate now cover every Cargo workspace of the repo, not only `clis`. The gate fails on a declared workspace crate no source names unless `clis/pipeline/data/blast-radius.json` lists `<crate> -> <dependency>` under `allowed_unused` with a reason: eight backend entries are listed there today (`rca-service`, `rca-service-core`, `reporting-service-rs`, `leetcode-auth-cookie-capture-rs`) and each goes with its removal.

## Why blocked

Not blocked. The scan is a candidate list: a dependency used only through a macro, a feature or a registry patch needs a look before it goes.

## What unblocks it

Check each candidate (and each `allowed_unused` entry) (`cargo tree -i <dep>`, a build without it), remove the real ones, and lower the affected crates' budgets in `clis/pipeline/data/blast-radius.json`.
