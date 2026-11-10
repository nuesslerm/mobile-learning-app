---
title: useEffect restrictions
date: 2026-07-13
category: mobile-app
tags: [useEffect, hooks, review, react, mobile-app]
problem_type: knowledge
module: mobile-learning-app
---

## Context

`useEffect` is restricted in this codebase, by review: no lint rule checks it since 20 Oct 2026, when lint was cut to the rules that catch defects. Cleanup-returning effects are fine; setState-only and no-dependency effects are not.

## Guidance

- Effects that return a cleanup function are auto-allowed.
- Effects that only call `setState` should be derived during render instead.
- Effects with no dependency array are banned (every-render effects).
- Any other effect carries a one-line comment saying why it is an effect.

Bad:

```tsx
useEffect(() => {
  setDerivedValue(compute(props));
}, [props]);
```

Good:

```tsx
const derivedValue = useMemo(() => compute(props), [props]);
```

## Why this matters

Most effects in the old code bypassed the ban, masking derived state or over-rendering. The rule forces state to be derived during render and keeps side effects explicit and justified.

## When to apply

- Adding an effect to a screen or hook.
- Reviewing a PR flagged by `restrict-use-effect`.
- Migrating from `React.useEffect` to the allowed patterns.

## Examples

The lint rule that enforced this was dropped on 20 Oct 2026, when the frontend moved to oxlint with only the most useful rules; the convention stands as review guidance.
