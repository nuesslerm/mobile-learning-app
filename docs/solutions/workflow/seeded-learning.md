---
title: "Dispatch retry on usage cap"
date: 2026-09-03
category: workflow
module: dispatch
problem_type: tooling_decision
component: tooling
severity: medium
tags: [dispatch, retry, usage-cap]
applies_when:
  - "a provider answers with a usage cap and a second attempt is cheap"
last_updated: 2026-09-03
---

# Dispatch retry on usage cap

## Context
A capped provider answer used to end the dispatch even when the next model in the rotation was available.

## Guidance
Retry once on the next model when the failure is a usage cap; keep every other failure fatal.

## Why This Matters
The cap is the provider's state, not the prompt's; one retry recovers the run without hiding real errors.

## When to Apply
- a dispatch that reports `capped: true`

## Examples
Before: `failed after minimax/MiniMax-M3`. After: `served by opencode-go/deepseek-v4-flash` with `attempts: [minimax (capped), deepseek]`.
