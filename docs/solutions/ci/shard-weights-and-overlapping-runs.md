---
title: "Deterministic shard weights and overlapping runs"
date: 2026-11-19
category: ci
module: ci-pipeline
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - "A flow or integration shard is red and the fix should re-run it without waiting for the other shards or an older run"
  - "Changing how tests are dealt to the hosted shards, or the concurrency of ci-hosted.yml"
tags: [ci, shards, concurrency, nx-cache]
---

## The split

`clis/pipeline` deals the flow and integration tests to the `HOSTED_SHARDS` machines by unit key, not round-robin. A unit is a package, a crate (`crate-<crate>`) or, for integration and e2e groups, `crate-<crate>/<group>`; its weight is in `clis/pipeline/data/shard-weights.json` (a default per stage, an entry for each heavy unit). The units are dealt longest first to the lightest shard (`assign` in `test_plan.rs`), so the same commit always puts the same unit on the same shard and a shard's Nx cache key and CI timing row mean the same thing from run to run. Change the weights when a unit's measured time moves; the tests in `shard_tests` pin the dealing.

## Overlapping runs

`ci-hosted.yml` groups runs per commit (`ci-hosted-<sha>`), so a push never waits for an older run and a red shard is fixed and re-run at once; no run is cancelled, and every run is measured to its end. Per-job groups still serialise what must not overlap (the deploy per app). The `head` job in `pipeline.yml` gates the deploy and the production EAS publish on the commit still being the head of `dev`, so a slow older run never puts older code over newer.

## The e2e jobs are per commit too

`e2e-tests.yml` used one concurrency group per platform for every run. GitHub keeps one running and one pending job per group and cancels the older pending one, so a newer push silently cancelled the browser and Android jobs of the run before it (3a110f34, no job red). The group now carries the commit, so overlapping runs each finish their own e2e jobs.

## The e2e jobs do not wait for the integration tests

`browser-e2e-tests` runs against a web export built on its own runner and `android-e2e-tests` against its own backends, so neither reads the deploy or the production EAS Update, which both wait for every stage including the slowest integration shard (about 800 s). They now need `plan`, `flow-tests`, `typecheck-lint-unit-tests`, `clis` and `cli-e2e-tests`; Android also needs `eas-update-e2e`, the job that publishes the branch `e2e` update and hands its pointer over, split out of `eas-update` (which keeps the production publish gated on the tests).

## Integration shards queue per crate set

The concurrency group of an integration shard is `hosted-integration-<key>`, the key being `wf ci plan shard-keys` of the crates that shard runs. Two commits that plan the same crates on a shard share a group: the later one waits for the earlier one, and a third commit replaces the second's pending job (owner, 19 Nov: desired). A different set of crates has a different key and starts at once. The key follows the weighted split of that commit's plan, so a plan that moves a crate to another shard changes the key.
