# Nightly audit log

One entry per night, appended by the nightly audit session (the "Nightly audit clock (daily)" tick into `session_01MdgrcdJGE5MkZS29xApR9c`, in `nuesslerm/mobile-learning-app`) (procedure: `docs/runbooks/nightly-audit.md`; before 7 Nov 2026 "ci nightly cost review"). Each entry: the date, the numbers judged, the fixes landed (commit and the number each should move), the earlier fixes checked (measured number, kept or reverted) and what needs the owner (also recorded in the findings inbox issue, `#79`, once the owner session creates it).

## 2026-11-06 (set up by hand)

- The per-push `budget` job stopped failing on cost (`enforce: false`): the last runs measured 3601 CPU-seconds (run 42286286271) and 1889 (run 42293798628, after the pnpm single-saver rule, with two jobs still on the old `wf`), against a limit of 1000.
- Fixes already landed that the first nights must check: `e0474576` (browser e2e hash on the pinned Playwright: browser e2e 26 s and Android e2e 20 s replayed on run 42293798628) and `59a61f33` (only `flow-tests` packs the pnpm store: 3 minutes per other job saved).
- Open work: issues #359 and #360 of the former private repository.

## 2026-11-14

- Verdict: run 43404231153 (scheduled 2026-11-13 09:47 UTC, about 19 h old), `ok: false`, 299 findings, 90 hard. Largest: `should-have-hit` on `crate-t3-serve:cargo-integration` (484 s, 419 s), `crate-pipeline:cargo-contract` (395 s), `crate-deploy:cargo-flow` (339 s), `crate-orca-serve:cargo-integration` (304 s): the Nx cache held a result and did not serve it.
- Fixes landed: none. Cause of the cargo cache misses not yet read from the ledger; `wf ci audit` could not run here (no `bws`, no ledger credentials).
- Earlier fixes checked: none listed in the last 7 days.
- Tracker: t3 pinned 0.1.6, latest 0.1.9 (drifted); orca check failed with 403 from api.github.com (no `GITHUB_TOKEN`); run 43403225970 failed in the record legs `record orca-scenarios` and `record orca-recorded`. Nothing landed.

## 2026-11-15

- Verdict: run 43579208549 (scheduled 2026-11-14 09:09 UTC, about 19 h old), `ok: false`, 490 findings, 132 hard: 64 `should-have-hit`, 59 `rejected-write`, 9 `seeded-lost`, 358 `replayed-nothing`. Largest waste: `crate-deploy:cargo-flow` (345 s, 211 s), `crate-t3-serve:cargo-flow` (116 s), `@backend/reporting-service:test:flow` (100 s). Worse than 2026-11-14 (299 findings, 90 hard): the cache misses and rejected writes persist.
- Fixes landed: none. The cause of the cache misses and rejected writes is not yet traced; `wf ci audit` cannot run here (no `bws`, no ledger credentials).
- Earlier fixes checked: none listed in the last 7 days.
- Tracker: t3 now pinned 0.1.9, latest 0.1.9 (current). Newest track-contracts run 43651197829 (2026-11-14 23:02 UTC) failed in the drift report step; the run before it, 43647841263, passed. Orca not checked (no `GITHUB_TOKEN`).
- Inbox issue `#79` not updated: `gh issue view` is blocked in this session (GraphQL); the cache findings need a traced cause first.

## 2026-11-16

- Verdict: run 43700244708 (scheduled 2026-11-15 09:38 UTC, about 19 h old), `ok: false`, 281 findings, 67 hard (all `should-have-hit`), 214 `replayed-nothing`. Better than 2026-11-15 (490 findings, 132 hard): `rejected-write` (59) and `seeded-lost` (9) are gone. Largest waste: `crate-pipeline:cargo-contract` (478 s), `crate-deploy:cargo-flow` (258 s, 239 s).
- Fixes landed: none. The cause of the remaining `should-have-hit` misses on the cargo stages is not yet traced; `wf ci audit` cannot run here (no `bws`, no ledger credentials). The audit run 43708903254 (2026-11-15 11:26 UTC) failed.
- Earlier fixes checked: none listed in the last 7 days.
- Tracker: t3 pinned 0.1.9, latest 0.1.9 (current). The last two track-contracts runs failed: 43699839667 (2026-11-15 09:33 UTC, step "Check the pins") and 43651197829. Orca not checked (no `GITHUB_TOKEN`). Nothing landed.
- Inbox issue `#79` not updated: `gh issue` is blocked in this session (GraphQL).

## 2026-11-17

- Verdict: run 43844387127 (scheduled 2026-11-16 09:57 UTC, about 19 h old), `ok: false`, 422 findings, 62 hard (all `should-have-hit`), 360 `replayed-nothing`. Hard findings are flat against 2026-11-16 (67); total findings rose from 281. Largest waste: `crate-t3-serve:cargo-integration` (500 s), `crate-pipeline:cargo-contract` (364 s, 363 s).
- Fixes landed: none. The cause of the `should-have-hit` misses on the cargo stages is still not traced and now spans four nights; `wf ci audit` cannot run here (no `bws`, no ledger credentials).
- Earlier fixes checked: none listed in the last 7 days.
- Tracker: t3 is behind: pinned 0.1.9, latest 0.1.10 (drifted). The last three track-contracts runs all failed in the step "Check the pins" (43843558576 on 2026-11-16 09:51 UTC, 43699839667, 43651197829), so no record or drift report exists for 0.1.10. Orca not checked (no `GITHUB_TOKEN`). Nothing landed.
- Not recorded in inbox issue `#79` (`gh issue` is blocked in this session): the audit cache misses (four nights) and the failing "Check the pins" step (three runs) need an owner or a session with the vault.

## 2026-11-18

- Verdict: run 44068892681 (scheduled 2026-11-17 10:52 UTC, about 18 h old), `ok: false`, 258 findings, 35 hard (all `should-have-hit`), 223 `replayed-nothing`. Hard findings fell from 62 (2026-11-17), and total findings from 422. Largest waste moved to the offload crate: `crate-offload:cargo-e2e-parity_features` (818 s), `crate-offload:cargo-e2e-parity_capabilities` (703 s), `crate-offload:cargo-e2e-parity` (376 s).
- Fixes landed: none. The cause of the `should-have-hit` misses on the cargo stages is still not traced (five nights); `wf ci audit` cannot run here (no `bws`, no ledger credentials).
- Earlier fixes checked: none listed in the last 7 days.
- Tracker: t3 still behind: pinned 0.1.9, latest 0.1.10 (drifted). The verb is now `wf mimic track` (`wf parity` no longer exists). The last four track-contracts runs failed in the step "Check the pins" (44068213532 on 2026-11-17 10:48 UTC, 43843558576, 43699839667, 43651197829), so no record or drift report exists for 0.1.10. Orca not checked (no `GITHUB_TOKEN`). Nothing landed.
- Not recorded in inbox issue `#79` (`gh issue` is blocked in this session). Owner items: the failing "Check the pins" step (four runs) and the cache misses.
- Update for the runbooks: `docs/runbooks/contract-tracker.md` still names `wf parity track`.
