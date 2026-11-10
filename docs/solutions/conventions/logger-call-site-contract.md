---
title: The TypeScript and Rust loggers share one call-site contract, checked black box
date: 2026-10-25
category: conventions
tags: [logging, telemetry, call-site, contract, babel, rust, black-box-test]
problem_type: convention
module: clis/telemetry, frontend/libraries/logging
---

## Context

Both loggers infer where a line was logged instead of taking it by hand: the TypeScript one through a Babel plugin
(`frontend/libraries/logging/src/babel/injectLoggerSource.ts`), the Rust one through the `site!` and `record!` macros of
`clis/telemetry`. Left alone they drifted: the plugin named an anonymous callback `<anonymous>`, and Rust cannot tell a closure from
an `async fn` body at all (both are `{{closure}}` in `type_name`).

## The rule

`clis/telemetry/contract/callsite.json` is the contract, and the Rust package owns it. A record carries:

- `file`: the source path relative to the implementation's `root` (the cargo workspace for Rust, so `telemetry/src/x.rs`; the repo for
  TypeScript).
- `line`: the 1-based line of the logging call.
- `fn`: the innermost enclosing **named** function. A method reports its own name; a closure, callback or async block reports the
  function around it; a call outside any named function reports null. (`fnPath`, the full lexical chain, is TypeScript-only.)

## How it is checked

A fixture source marks each logging call with a trailing `// @site <case> fn=<name>` comment and logs the case id as the message.
`callsite-check` (`telemetry::callsite::check`, behind the `callsite` feature) derives the expected file, line and function from the
markers and compares them with the records the implementation emitted, and it fails a case that is missing, duplicated or unmarked.
Adding a case is writing a marker.

- Rust: `clis/telemetry/tests/flow.rs` with `tests/fixtures/callsite.rs`, through `record!` and the console line.
- TypeScript: `frontend/libraries/logging/tests/integration/callsiteContract.integration.test.ts` compiles
  `tests/fixtures/callsite.ts` with the real plugin and pipes the injected context to the Rust binary
  (`cargo run -p telemetry --features callsite --bin callsite-check`, or `CALLSITE_CHECK_BIN`).

The TypeScript test failed on the callback case with the old plugin line and passes with
`fn = first non-anonymous name`, which is how the two were brought into line.

## Not yet done

Services log through `tracing`, whose events carry file and line but no function, so their records have no `fn`. Moving a call to
`record!` (or a `tracing` wrapper that adds `code.function`) is how a caller gets it.
