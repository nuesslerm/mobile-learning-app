---
title: Package shapes — the table of package types that `wf check --layout` enforces
date: 2026-11-16
category: architecture
module: clis/pipeline
problem_type: architecture_pattern
component: development_workflow
severity: low
tags: [architecture, package-types, layout, generated]
applies_when:
  - "adding a package, or choosing its package type"
  - "changing what a package type requires, allows or forbids"
related_components: [tooling]
---

# Package shapes

Every package opts in with `[package.metadata.arch] type = "..."` in `Cargo.toml` or an `arch` key in `package.json`. `wf check --layout` reads `clis/pipeline/data/package-types.json` and checks each package's placement and dependency edges against its type. A pass proves shape and edges only, not behaviour. The reasoning behind the types is in `hexagonal-architecture-by-package-type.md`.

The table below is generated from the manifest: edit the manifest, then run `wf check --layout --write`. `wf check --layout` fails on drift, in lefthook (pre-commit) and in the Architecture workflow.

<!-- BEGIN generated:shape-table -->
| Type | Ecosystem | Pure | Rings (may import) | Required | At the root | Forbidden | May depend on | May dev-depend on |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `rust-core` | rust | yes | domain; application -> domain; testing -> domain, application | `lib.rs`, `domain or application` | `lib.rs`, `testing.rs` | `api`, `infrastructure`, `main.rs` | `rust-core`, `rust-contract`, `rust-group` | `rust-testkit` |
| `rust-flat-core` | rust | yes | domain | `lib.rs` | `*.rs`, `testing` | none | `rust-core`, `rust-flat-core`, `rust-contract` | `rust-testkit` |
| `rust-rs` | rust | no | api; infrastructure | `lib.rs`, `main.rs` | `lib.rs`, `main.rs`, `compose.rs`, `ports.rs` | `domain`, `application` | `rust-core`, `rust-contract`, `rust-group`, `rust-flat-adapter`, `rust-single-hexagon`, `library` | `rust-testkit` |
| `rust-contract` | rust | yes | none | `lib.rs` | `*.rs`, `testing` | none | `rust-contract` | `rust-testkit` |
| `rust-flat-adapter` | rust | no | none | `lib.rs` | `*.rs` | none | `rust-contract`, `rust-core` | `rust-testkit` |
| `rust-group` | rust | yes | none | `lib.rs` | `*.rs` | none | `rust-contract`, `rust-core` | `rust-testkit` |
| `library` | rust | no | none | `lib.rs` | `*` | none | `rust-contract`, `library` | `rust-testkit` |
| `cli-crate` | rust | no | none | `lib.rs or main.rs` | `*` | none | `cli-crate`, `library`, `rust-contract`, `rust-core`, `rust-group`, `rust-flat-adapter`, `rust-single-hexagon` | `rust-testkit` |
| `rust-single-hexagon` | rust | no | domain; adapters -> domain; bin -> domain, adapters | `lib.rs`, `domain or domain.rs`, `adapters`, `bin` | `lib.rs` | none | `rust-contract`, `library` | `rust-testkit` |
| `rust-testkit` | rust | no | none | `lib.rs` | `*.rs`, `bin` | none | `rust-contract`, `rust-testkit` | none |
| `hexagonal-feature` | ts | no | domain; data -> domain; state -> domain; ui -> domain, state; di -> domain, data, state; routes -> di, ui; testing -> domain, data, state, ui, di, routes | `index.ts or index.tsx`, `domain`, `state`, `ui`, `di`, `routes` | `index.ts`, `index.tsx`, `__tests__`, `__mocks__`, `project.json` | none | none | none |
| `presentation-feature` | ts | no | state; ui -> state; routes -> ui | `index.ts or index.tsx`, `ui`, `routes` | `index.ts`, `index.tsx`, `__tests__`, `__mocks__`, `project.json` | `domain`, `data`, `di` | none | none |
| `hexagonal-library` | ts | no | ports; core -> ports; adapters -> core, ports; testing -> core, ports, adapters | `index.ts`, `core` | `index.ts` | none | `hexagonal-library` | none |
<!-- END generated:shape-table -->
