---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-agent/src/vault.rs"
  - "libraries/agent-api/exec-fs/src/**"
opened: 2026-11-20
last_verified: 2026-11-20
links:
  - "docs/plans/rca-parity/plan.md"
---
# Orca `aiVault.listSessions` fields the scan cannot fill

Issue #83, O3. The rows `aiVault.listSessions` and `aiVault.prepareSessionResume` are served over `sessions.list` and the executor's `session_scan` (`libraries/agent-api/exec-fs`).

- The recorded host has no CLI sessions, so the only recorded session list is empty. The row shape of a non-empty list comes from the pinned source (`session-list-result-validation.ts`), not from a recording.
- The line formats read from `~/.claude/projects/*/*.jsonl` and `~/.codex/sessions/**/rollout-*.jsonl` are written from the CLIs' own files; no pinned CLI version was run against `session_scan` (the real-CLI jobs verify them).
- Filled with a stand-in: `totalTokens` is 0 (no token sum is read); `previewMessages` holds the first and last prompt only; `titleKind` and `accountHome` are absent; `executionHostPlatform` is absent; `id` is `<agent>:<sessionId>`.
- Only claude and codex sessions are listed; the host's other CLIs keep their sessions elsewhere and rca runs only those two (and opencode, whose store is not scanned).
- `prepareSessionResume` answers `{useRealCodexHome: false}` after checking the params, as the recorded host does when no account home is selected. It copies nothing and does not check that the file exists (the recorded call with a made-up path succeeds). The chat-fork form (`sourceChat`) is refused as `Invalid input`.
- Proof is flow tests, the executor fixture and the service chain test; no recorded replay of a non-empty list exists.
