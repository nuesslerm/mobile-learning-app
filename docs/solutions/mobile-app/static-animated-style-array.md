---
title: Static plus animated style arrays silently drop static styles
date: 2026-07-13
category: mobile-app
tags: [animated, style, nativewind, cssinterop, reanimated]
problem_type: knowledge
module: mobile-learning-app
---

## Context

A `cssInterop`'d `Animated.View` with `style={[staticObj, animatedStyle]}` drops the static object — layout like `position: 'absolute'` simply vanishes. The failure is silent; a lint rule caught it until 20 Oct 2026, when lint was cut to the rules that catch defects on a device the app runs today, so reviews catch it now.

## Guidance

Put static layout on a plain `View` wrapper and animated props only on the inner `Animated.View`. Never mix static and animated styles in a single array on a cssInterop'd animated component.

Bad:

```tsx
<Animated.View style={[styles.absoluteFill, animatedStyle]} />
```

Good:

```tsx
<View style={styles.absoluteFill}>
  <Animated.View style={animatedStyle} />
</View>
```

## Why this matters

Full-screen layout breaks with no error. `position: absolute` disappears and the component renders as if unpositioned — hard to debug because the animated style itself works.

## When to apply

- Using NativeWind with Reanimated.
- Any `cssInterop`'d `Animated` component.
- Refactoring a component flagged by `no-mixed-animated-style-array`.

## Examples

The lint rule that enforced this was dropped on 20 Oct 2026, when the frontend moved to oxlint with only the most useful rules; the convention stands as review guidance.
