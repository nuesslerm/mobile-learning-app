---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-terminal/src/**"
opened: 2026-11-16
last_verified: 2026-11-16
links:
  - "docs/plans/rca-parity/plan.md"
---
# The Orca terminal stream is a replay of rca's byte tail, not the host's screen

## What is broken

`terminal.subscribe` is served over rca's held-open `terminal.attach` (the phone's binary frames, credit, resume, input, resize, the JSON stream and
the refusals). It differs from the pinned host in these places:

- The snapshot replays rca's retained 256 KiB output tail with terminal query sequences stripped, and the queries are not replayed afterwards. It is
  not a serialized screen, so it carries no `alternateScreen`, `terminalOwner`, `lastTitle`, `oscLinks`, `cwd`, kitty flags or `scrollbackRows`, and
  the stream sends no Metadata frames. `truncated` means the tail no longer starts at offset 0. A full-screen program can leave a wrong screen.
- `displayMode` is always `auto` and the layout `seq` is omitted (the host treats `undefined` as fail-open).
- There is no desktop input lock, no fit-override events and no wait for a renderer mount.
- A stream token is counter-based and bound to the device, not a random UUID.
- A resize is announced only for the phone's own resizes.

## Why blocked

rca keeps bytes by offset and no screen model, no desktop renderer and no title, link or cwd tracking, so none of these can be produced from its
terminal contract.

## What unblocks it

A screen model in the rca terminal group (a serialized snapshot with title, cwd and alternate-screen state), then a mapper change that sends it in
place of the replay. The input lock, fit overrides and mount wait have no counterpart until rca has a desktop.
