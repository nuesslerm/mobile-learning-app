---
title: pnpm `allowBuilds` is deny-by-default — every build script is opt-in
date: 2026-07-13
category: workflow
tags: [pnpm, supply-chain, security, postinstall, allowbuilds, onlyBuiltDependencies, workflow]
problem_type: knowledge
module: workflow
---

## Context

`pnpm-workspace.yaml` declares an `allowBuilds` map where **every entry is `false` except the ones we deliberately run**. The two `true` entries today are `bufferutil` and `utf-8-validate`, the native add-ons `ws` loads when they are built. Everything else — `@ast-grep/cli`, `@shopify/react-native-skia`, `esbuild`, `lefthook`, `nx` — is explicitly denied; an entry for a package the lockfile no longer installs is deleted rather than left behind. pnpm's deny-by-default model (introduced in pnpm 9.x as a successor to the older `onlyBuiltDependencies` allow-list) refuses to run a package's `install`/`postinstall` script unless it appears here with `true`. The narrower `onlyBuiltDependencies` list further down (`pnpm-workspace.yaml` lines 57-58) is the v8-compat form pnpm still honors.

## Guidance

- **Never rely on a postinstall running by default.** If a dependency needs a build script (e.g. a native module compile), the entry must appear in `allowBuilds:` with `true` and in `onlyBuiltDependencies:` with `- '<name>'`. Both are required to keep behavior stable across pnpm versions and across the `pnpm install --ignore-scripts` escape hatch.
- When adding a new native dep, set it to `false` first, run `pnpm install`, see what breaks (a missing binary, a wrong arch fallback, a runtime error about a missing `.node` file), then consciously flip to `true` and explain *why* in a PR comment. "It needs the postinstall to build" is the expected trigger, not a blocker.
- When auditing the repo, the list of `true`s is the explicit supply-chain surface — every one runs arbitrary code on every developer's machine and on every CI install. Treat each `true` as a code-review artifact, not a config detail.
- Pair every `allowBuilds: true` entry with the upstream package's CVE feed (`pnpm audit`) and a pinned version in `pnpm-workspace.yaml`'s catalog where appropriate.

## Why this matters

A postinstall script is arbitrary code execution with the privileges of whoever runs `pnpm install`. The default-deny posture means a malicious dependency's `postinstall` is *not* run on this machine — pnpm warns and skips — which converts a whole class of supply-chain attack into a loud build-time error instead of a quiet compromise. The handful of `true`s are deliberate, small, and reviewed every time they change.

## When to apply

- Adding any package whose install scripts do native compilation, fetch binaries, or otherwise run code.
- Reviewing a PR that touches `pnpm-workspace.yaml`.
- Investigating a `ERR_PNPM_*` "ignored build scripts" warning that surfaces after `pnpm install`.
- Auditing supply-chain exposure (`pnpm run audit` + this list).

## Examples

- Authoritative source: `pnpm-workspace.yaml` (the `allowBuilds:` and `onlyBuiltDependencies:` blocks).
- Companion controls in the same file: `minimumReleaseAge: 1440` (minutes, not a duration string — `"1440 minutes"` resolves to `NaN` in pnpm and crashes every install), `blockExoticSubdeps: true`, `strict-ssl`.
- The CVE response flow and the broader checklist this rule belongs to: `docs/archive/README.md`, `docs/archive/README.md`, `docs/archive/README.md`.