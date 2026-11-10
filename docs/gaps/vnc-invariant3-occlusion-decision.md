---
status: blocked
area: vnc-auth
paths:
  - "clis/cdp/src/attached.rs"
  - "clis/cdp/src/measure_launch.js"
opened: 2026-10-04
last_verified: 2026-10-04
links: []
---
# Launch invariant 3 (inputs visible) needs a human decision on occlusion

## What is broken

On console.anthropic.com six candidate inputs fail invariant 3 on an otherwise correct launch. Four
deployed rounds isolated the cause: the page's own hero content (`section.relative isolate flex`, ...) is
painted over closed-menu inputs. Commit `1e7d12b` treats occlusion as out of scope and reports
`occludedCount` separately.

## Why blocked

Narrowing a rule a human wrote is not a unit's call. Options: (1) occlusion out of scope (as
implemented), (2) in scope (red on most login pages), (3) a seventh, separate occlusion invariant.

## What unblocks it

The owner picks 1, 2 or 3. The check never fails a login: `check_launch_invariants`
(`clis/cdp/src/attached.rs`) returns a report and currently has no caller (see
`docs/gaps/vnc-launch-url-skips-invariants.md`), so no login depends on the answer.
