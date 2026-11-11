---
title: test-report routine - the Claude clock that turns run bundles into report artifacts
verifies: Every CI test run published since the last report is on a report page with its images, recordings and metrics.
tier: routine
schedule: every 6 h (cron `43 */6 * * *`, "Test report clock (every 6 h)", fires into the `<reports-session>`)
---

# test-report routine

The procedure lives in one place, the `test-report` skill (`.agents/skills/test-report/SKILL.md`). The clock only fires it every 6 hours (bursts of run bundles follow pushes, so 6 h is enough); anyone can run the same skill on request ("make a test report").

The clock fires into one persistent session, `<reports-session>`, created by the owner with `nuesslerm/mobile-learning-app` as its source, so every firing has a checkout. Its tick prompt:

```text
Scheduled tick: the test report is due. Run `git fetch origin dev`, read the test-report skill as it is on dev
(`git show origin/dev:.agents/skills/test-report/SKILL.md`) and follow it. With no new runs, reply with one line and commit nothing.
```

Keep this file and the stored prompt the same. Report commits carry `[skip ci]`, so they start no CI run. The session needs push access to `dev` under the repository ruleset (unconfirmed until the first trial tick).
