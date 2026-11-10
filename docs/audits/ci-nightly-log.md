# Nightly audit log

One entry per night, appended by the scheduled task "nightly audit" (procedure: `docs/runbooks/nightly-audit.md`; before 7 Nov 2026 "ci nightly cost review"). Each entry: the date, the numbers judged, the fixes landed (commit and the number each should move), the earlier fixes checked (measured number, kept or reverted) and what needs the owner (also recorded in issue #365).

## 2026-11-06 (set up by hand)

- The per-push `budget` job stopped failing on cost (`enforce: false`): the last runs measured 3601 CPU-seconds (run 42286286271) and 1889 (run 42293798628, after the pnpm single-saver rule, with two jobs still on the old `wf`), against a limit of 1000.
- Fixes already landed that the first nights must check: `e0474576` (browser e2e hash on the pinned Playwright: browser e2e 26 s and Android e2e 20 s replayed on run 42293798628) and `59a61f33` (only `flow-tests` packs the pnpm store: 3 minutes per other job saved).
- Open work: issues #359 and #360.
