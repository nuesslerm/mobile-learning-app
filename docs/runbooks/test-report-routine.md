---
title: test-report routine — the hourly Claude job that turns run bundles into report artifacts
verifies: Every CI test run published since the last report is on a report page with its images, recordings and metrics.
tier: routine
schedule: hourly (Claude Code Remote Routine trig_01WG8t9iahjso6FVxG9EB9Zv, one persistent session with the repository)
---

# test-report routine

The procedure lives in one place, the `test-report` skill (`.agents/skills/test-report/SKILL.md`). The Routine only runs it,
hourly, so a run's report follows within the hour; anyone can run the same skill on request ("make a test report").

The Routine fires into one persistent session created with the repository as its source, so every firing has a checkout
(the fresh-session runs before it had none and could not read the state file). Its stored prompt:

```text
You are the test-report routine of nuesslerm/expo-hello-world-app-2, firing hourly into this session. Earlier turns are
history, not state. Run `git fetch origin dev`, read the test-report skill as it is on dev
(`git show origin/dev:.agents/skills/test-report/SKILL.md`) and follow it. With no new runs, reply with one line and commit
nothing.
```

Keep this file and the stored prompt the same; `update_trigger` carries a change over. Report commits carry `[skip ci]`, so
they start no Test or Deploy run.
