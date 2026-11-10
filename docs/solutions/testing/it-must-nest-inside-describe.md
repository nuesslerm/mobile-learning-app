---
title: it() and test() must nest inside describe() — no top-level tests
date: 2026-07-13
category: testing
tags: [testing, vitest, describe, it, nesting, convention-lint, structural-rule]
problem_type: convention
module: tests (review convention)
---

## Context

Every `it(...)` / `test(...)` in this repo's test files lives inside a
`describe(...)` block. This is a review convention, not a lint rule: the
repository lints with oxlint (`.oxlintrc.json`, `frontend/.oxlintrc.json`),
which has no `no-restricted-syntax`, and the one ESLint selector that checked
it (a single test file by then) went with ESLint on
20 Oct 2026, when the owner cut lint to the rules that catch defects.

ile.

## Guidance

- **Shape**:
  ```ts
  // ✅ correct
  describe('given an empty registry', () => {
    it('when a slot is created, then it appears in the registry list', () => { … });
  });

  // ❌ top-level test
  it('when a slot is created, then it appears in the registry list', () => { … });
  ```
- **Nested `describe` is allowed for state stacking**:
  ```ts
  describe('given a session', () => {
    describe('given the session has expired', () => {
      it('when a protected route is requested, then a 401 is returned', () => { … });
    });
  });
  ```
  The selector matches only an `it`/`test` call that is a top-level statement of the file, so a call nested at any depth inside a `describe` block passes.
- **Why the rule exists**: top-level tests lose the "given" preamble that the
  Given/When/Then convention expects. A top-level `it('when X, then Y')` reads as a
  sentence with a missing subject. Forcing the `describe` wrapper makes the
  precondition explicit and groups related cases under one suite-output header.

## Why this matters

A top-level `it` is structurally valid vitest but operationally slippery: it runs
in the global suite, has no `describe` header in the output, and breaks the file's
visual rhythm. The Given/When/Then convention is only legible when every test is
wrapped in a `describe` whose name states the starting state — without that, the
"given" half of every test name becomes implicit and varies wildly between files.
Reviews hold the line; a top-level `it` is a review comment, not a lint error.

## When to apply

- Refactoring a test that grew a top-level `it` because someone added a "one-off sanity check" at the bottom of a file.
- Reviewing a PR that touches a test file: if a `describe` is wrapping nothing, push back.
