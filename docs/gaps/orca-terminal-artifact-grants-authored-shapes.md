---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-workspace/src/artifacts.rs"
  - "clis/rca-exec/src/adapters/artifacts.rs"
  - "docs/plans/rca-parity/coverage/orca-decisions.json"
opened: 2026-11-21
last_verified: 2026-11-21
links:
  - "docs/plans/rca-parity/plan.md"
  - "docs/gaps/t3-mcp-secret-guest-telemetry-no-success-shape.md"
---
# Orca terminal-artifact rows are served from authored shapes, behind a grant that is off by default

## What is built

`files.resolveTerminalPath`, `files.readTerminalArtifact`, `files.readTerminalArtifactPreview` and `files.writeTerminalArtifact` are served (O3c).
The executor has three actions (`artifact_stat`, `artifact_read`, `artifact_write`), the agent API three methods of `files.edit`
(`artifact.stat`, `artifact.read`, `artifact.write`), and the mapper keeps a grant table per device (ten minutes after last use, 256 at most).
A grant binds the canonical path, the file's identity and a content fingerprint; a read or save after someone else rewrote the file is refused
as `terminal_file_grant_stale`. Flow tests: `clis/rca-exec/tests/flow/artifacts.rs`, `clis/orca-serve/map-workspace/tests/flow.rs`; chain test:
`clis/orca-serve/tests/integration/terminal_artifacts.rs` (real service and executor).

## Decision taken by the agent (the owner's sign-off was pending)

The sign-off for terminal-artifact grants outside the workspace was open, so the agent took the safer option: the rows are served, and nothing
outside the executor's root is reachable unless the executor's owner names a directory in `RCA_EXEC_ARTIFACT_ALLOW` (colon separated, empty by
default). With it empty a path outside the root is refused as "not granted on this host". One switch undoes the grant: empty that variable.
The owner can overrule either way (widen it, or move the rows back to not-offered in `orca-decisions.json`).

The workspace is a boundary too: below a prefix (a directory workspace or a worktree) the service sends the prefix as `within`, and the executor
then serves only that directory plus the allowed ones, so a sibling worktree or the rest of the root is refused as outside. A file longer than the
16 MiB fingerprint cap is never written through a grant (`artifact_write` refuses it as invalid).

## What is not proven

- No recording holds a granted file: the recorded crawl sends a made-up grant id, which the host refuses (those replays now match the refusals).
  The success shapes are authored from `references/orca/src/main/runtime/terminal/artifacts`, so the four rows carry a PROVISIONAL `unrecordable`
  reason and need a recording against a real host with a real terminal tap.
- The terminal's own cwd (`terminal` handle) is not known to the mapper; only the `cwd` the phone sends is used. A `~/` path answers absent
  (the executor's home is not the service's to expand).
- A workspace that is an executor's own root has no absolute path known to the service, so an absolute path there is granted rather than opened as a
  workspace file.
- Grants die with their TTL, not when a device is unpaired. `files.statTerminalArtifact` and `files.readTerminalArtifactChunk` are desktop-only
  and not served.
- The fingerprint is a change detector (FNV-1a over the first 16 MiB), not a defence against a writer who can also match it.
