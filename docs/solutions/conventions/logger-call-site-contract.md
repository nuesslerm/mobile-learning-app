---
title: The Rust logger call-site contract
date: 2026-10-25
category: conventions
tags: [logging, telemetry, call-site, contract, rust, black-box-test]
problem_type: convention
module: clis/telemetry
---

## Context

The Rust one-shot logger infers the call site through the `site!` and
`record!` macros. The app's TypeScript logger uses structured events and resolves
exception stacks with source maps. See [Logging](logging.md).

## The rule

`clis/telemetry/contract/callsite.json` is the contract, and the Rust package owns it. A record carries:

- `file`: the source path relative to the cargo workspace, such as `telemetry/src/x.rs`.
- `line`: the 1-based line of the logging call.
- `fn`: the innermost enclosing **named** function. A method reports its own name; a closure, callback or async block reports the
  function around it; a call outside any named function reports null.

## How it is checked

A fixture source marks each logging call with a trailing `// @site <case> fn=<name>` comment and logs the case id as the message.
`callsite-check` (`telemetry::callsite::check`, behind the `callsite` feature) derives the expected file, line and function from the
markers and compares them with the records the implementation emitted, and it fails a case that is missing, duplicated or unmarked.
Adding a case is writing a marker.

`clis/telemetry/tests/flow.rs` runs `tests/fixtures/callsite.rs` through
`record!` and checks the resulting console lines.

## Not yet done

Services log through `tracing`, whose events carry file and line but no function, so their records have no `fn`. Moving a call to
`record!` (or a `tracing` wrapper that adds `code.function`) is how a caller gets it.
