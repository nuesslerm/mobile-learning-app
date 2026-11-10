---
status: blocked
area: tamil-writing
paths:
  - "frontend/mla-tamagui/features/tamil-writing/**"
opened: 2026-10-04
last_verified: 2026-10-04
links: []
---
# 123 of the 251 Tamil letters have no stroke guide

## What is broken

128 letters have a guide (31 base letters from the UPenn animations, 97 composed); the rest fall back to
free tracing, which grades placement and size but never stroke order or direction. Missing: every
-ii/-u/-uu form plus டி and லி (56, single ligatures), every -ai form (18), ஞ and ந rows (11), and the
grantha letters and the rest of the grid (38).

## Why blocked

A ligature has no seam to compose along, ை has no UPenn animation, and ஞ/ந differ between the
handwritten animation and Noto's printed form (composed strokes run 25-45% outside the letter).

## What unblocks it

Animations of those letters, or a Tamil writer recording them. For ை one recorded mark covers all 18.
