---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-state/src/**"
  - "clis/orca-serve/map-agent/src/session.rs"
opened: 2026-11-20
last_verified: 2026-11-20
links:
  - "docs/plans/rca-parity/plan.md"
  - "docs/plans/rca-offer-more/plan.md"
---
# `settings.getAgentLaunch` stays not offered: its answer is the launch of 35 terminal agents, which rca does not run

## What is broken

`settings.getAgentLaunch` answers the pinned host's projection of the launch settings of every TUI agent (`projectAgentLaunchSettings`): the
permission flags each of 35 terminal agents is started with, their command overrides, environment names and the combined `permissionMode`
(`yolo`, `manual`, `mixed`, `auto`). The recorded host gave only the fresh-host answer.

## What is served instead

The part of launch settings rca can follow is served and followed: `settings.mutateNativeChatSessionOptions` keeps a default model per agent
and `nativeChatPermissionMode` (kept by `settings.update`) is the permission of a new turn (`ask` and `auto` ask for approval, `accept-edits`
and `full-access` map to rca's modes). A turn takes the model of the send, then the thread's, then the saved default.

## Why blocked

rca starts three structured agents through a driver with a model and a `runtime_mode`; it starts no terminal agent with flags. An answer
built from the recorded defaults would say `permissionMode: yolo` and list flags no rca turn follows, and an answer that varies with
`nativeChatPermissionMode` would be a mapping from three agents to the summary of 35 that no recording shows.

## What unblocks it

A recording of the host after `settings.mutateAgentLaunch` changes (permission, command, arguments) for the three agents rca runs, and a
decision on whether rca should pass per-agent extra arguments and environment to the CLIs it spawns. Then the projection can be computed from
settings rca keeps and followed at launch.
