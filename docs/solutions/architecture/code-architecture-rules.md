---
title: "Code architecture rules: ports and adapters per package type, feature slices and the wf crate split"
date: 2026-11-08
category: architecture
module: repo-layout
problem_type: architecture_pattern
component: development_workflow
severity: medium
applies_when:
  - "Adding or moving a feature, a Rust service or a wf crate"
tags: [architecture, hexagonal, wf]
---

# Code architecture rules

Moved verbatim from the always-on `CLAUDE.md` section "Code architecture" (8 Nov 2026), where a short rule now points here.

Ports and adapters, one shape per package type: dependencies point inward to a pure core (`domain/` does no I/O), the core owns its ports, adapters implement them, one composition root wires them. Shapes, rules, checks and targets: `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`. A feature owns its slice in `frontend/mla-tamagui/features/<name>` (in-app Nx project, or workspace package with doubles in its `testing` entry; no `-mocks`/`-impl` siblings), imported only through its barrel, `routes/*` or `testing`; an `app/` route file under a feature prefix is a one-line re-export of its `routes/*` module (layouts and shell screens aside). Rust services are `core`/`rs` crate pairs. `clis/wf` is split by concept into crates (`wf-kit` at the bottom; `pipeline` for `wf test`, `wf check` and `wf ci`; `deploy` for `wf ship` and `wf vm`; `secrets`; `offload` for `wf offload`; `harness-core`, `harness-flow`, `harness-browser`, `harness-android` for the runners and their goldens; `wf-bench`, `knowledge`, `agent-config-sync` for their verbs; `wf` is only the router, the help table, `log` and `build-sha`: the one composition root; a crate owns its ports and ships their doubles behind its `testing` feature; plan `docs/plans/wf-crate-split/plan.md`, the CLI restructure `docs/audits/2026-09-02-token-diet-decision-log.md`). Checks: `pnpm run arch:services` (Rust services), each frontend package's `lint` (dependency-cruiser presets from `frontend/feature-kit`, with a known-violations baseline that only shrinks).
