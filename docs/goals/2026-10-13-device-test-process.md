---
id: device-test-process
title: device-test-process — a slim shared Android device, and a VM-run test process the gh-runner drives per commit
status: queued
created: 2026-10-13
branch: claude/device-lab
criteria:
  - id: a1.device-healthy
    description: 'The redroid the newest CI Android job brought up finished booting: the device entry of `wf android list-devices` its warm-up published (out/device-features.json) reports boot.completed and boot.activityService true. The emulator runs only in CI, so this reads CI evidence, never a live device.'
    command: |-
      f=$(curl -fsS "$(wf test results --query "test=warm-app platform=android limit=1" --json | jq -er '.runs[0].bundle')" | jq -er '.files[] | select(.path == "out/device-features.json") | .url') \
        && curl -fsS "$f" | jq -e '.boot.completed == true and .boot.activityService == true'
  - id: a2.rollback-real
    description: 'A failed slim set is rolled back by stopping the container and clearing the state before starting it again (never by deleting the file in a live container); the flow tests prove the printed script.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- android::the_lab_scripts 2>&1 | grep -q '^test result: ok. 1 passed'
  - id: a3.slim-survives-restart
    description: 'The slim set provably survives a restart: in the newest CI Android job, the device carried the proof marker written after a restart that booted, and most optional packages were disabled (read from the device health its warm-up published).'
    command: |-
      f=$(curl -fsS "$(wf test results --query "test=warm-app platform=android limit=1" --json | jq -er '.runs[0].bundle')" | jq -er '.files[] | select(.path == "out/device-features.json") | .url') \
        && curl -fsS "$f" | jq -e '.boot.completed == true and .restartProven == true and .disabledTotal >= 40'
  - id: b1.lease
    description: 'The device lease (FIFO, TTL, heartbeat, refusal naming the holder) has flow tests, and mutating `wf android` verbs take it.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- android::the_device_lease 2>&1 | grep -q '^test result: ok. 1 passed'
  - id: b2.queue
    description: '`wf test` (plan, device) has flow tests driven by a fake harness: selection by changed package, one run per runbook unit, run flags passed through, timeouts that keep the report, JSON shapes. The VM queue it replaced is gone (owner, 15 Oct: the Actions runner sequences device runs).'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- pipeline::ci_plans pipeline::focused_e2e_tests 2>&1 | grep -q '^test result: ok. 2 passed'
  - id: b3.manifest
    description: 'The e2e tests (e2e/*.lua) declare platform and against in their header, and `wf test plan --all` lists at least four with a script, platform and target.'
    command: |-
      wf test plan --all --json | jq -e '(.tests | length) >= 4 and (.tests | all(.script != null and .platform != null and .against != null))'
  - id: b4.workflow
    description: 'A GitHub workflow on the vm runner runs the focused e2e tests a push reaches (`wf test run --changed`) as Deploy''s last job and posts an e2e-tests status.'
    command: |-
      test -f .github/workflows/e2e-tests.yml \
        && grep -q 'wf test run' .github/workflows/e2e-tests.yml \
        && grep -q 'e2e-tests' .github/workflows/e2e-tests.yml \
        && grep -q 'runs-on: \[self-hosted, vm\]' .github/workflows/e2e-tests.yml
  - id: b5.real-job
    description: 'A job enqueued from a sandbox was run by the worker on the VM against the device and passed, with a recording that was read back.'
    command: |-
      jq -e '.verdict == "pass" and .ranOn == "vm" and (.recording | length) > 0' docs/runbooks/.results/device-test-process-e2e.json
  - id: b6.two-sandboxes
    description: 'Two sandboxes contended for the device: the second waited or was refused naming the holder, and ran after release.'
    command: |-
      jq -e '.verdict == "pass" and .refusedNamedHolder == true and .ranAfterRelease == true' docs/runbooks/.results/device-test-process-contention.json
  - id: b7.lanes-verdict
    description: 'The second-display parallel-lane experiment has a recorded verdict with numbers.'
    command: |-
      grep -q '^- Parallel lanes:' docs/plans/device-test-process/progress.md
  - id: b8.metro-flow
    description: 'Flow step 4: Expo Go on the device opened the running dev Metro at the pushed commit, with a screenshot and clean ReactNativeJS warnings.'
    command: |-
      jq -e '.verdict == "pass" and .against == "metro" and .warnings == 0' docs/runbooks/.results/device-lab-metro-android.json
  - id: c1.android-bench
    description: 'The Android suite measures a real cold start (not am start -W) and has committed initial and current baselines taken on the repaired slim device.'
    command: |-
      jq -e '.initial.runs >= 3 and (.initial.metrics | has("coldStartMs") and has("pssBytes") and has("apkBytes")) and .initial.env.coldStartMarker != null' clis/app-bench/baselines/android.json \
        && wf bench compare --suite android
  - id: c2.web-bench
    description: 'The web suites are trustworthy (zero-executed startup fails, missing harness UI is a clear error) and their baselines were re-recorded at the final tree.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- bench::repo_bench_ 2>&1 | grep -q '^test result: ok. 5 passed' \
        && wf bench compare --suite all
  - id: d1.gates-fresh
    description: 'Device-lab gates G1 to G3 record a pass on a commit at or after the last change under frontend/mla-tamagui/features and app, on web and on Android.'
    command: |-
      last=$(git log -1 --format=%H -- frontend/mla-tamagui/features frontend/mla-tamagui/app) \
        && for g in device-lab-voice-edit device-lab-tamil-writing device-lab-codemirror-typing device-lab-codemirror-typing-android; do \
             s=$(jq -r '.sha' docs/runbooks/.results/$g.json); \
             [ "$(jq -r '.verdict' docs/runbooks/.results/$g.json)" = pass ] && git merge-base --is-ancestor "$last" "$s" || exit 1; \
           done
  - id: d2.eas-fresh
    description: 'The EAS Update on branch dev (runtime exposdk:57.0.0) was published at or after the last change under frontend/mla-tamagui.'
    command: |-
      git merge-base --is-ancestor "$(git log -1 --format=%H -- frontend/mla-tamagui)" "$(cut -d: -f2 frontend/.claude/.eas-published-sha)" \
        && grep -q "exposdk:57.0.0" frontend/mla-tamagui/app.config.js
  - id: d3.docs
    description: 'The audit, decision log, ledger and report document the slim device, the boot-breaking packages, the lease and queue, and the numbers.'
    command: |-
      grep -q "^## Android" docs/audits/2026-10-06-app-bench-baseline.md \
        && grep -q "slim" docs/audits/2026-09-02-token-diet-decision-log.md \
        && test -f docs/audits/2026-10-06-app-bench-report.md
  - id: z.ledger-complete
    description: 'Every box in the ledger is ticked, and no Blocker line is left open.'
    command: |-
      ! grep -q '^- \[ \]' docs/plans/device-test-process/progress.md && ! grep -q '^- Blocker:' docs/plans/device-test-process/progress.md
  - id: z.pushed
    description: 'Everything is pushed: HEAD is on origin/dev and claude/device-lab is at HEAD.'
    command: |-
      git fetch -q origin dev claude/device-lab && git merge-base --is-ancestor HEAD origin/dev \
        && [ "$(git rev-parse origin/claude/device-lab)" = "$(git rev-parse HEAD)" ]
---

# device-test-process

Plan and ledger: `docs/plans/device-test-process/{plan.md,progress.md}`; the approved working plan is also at `/root/.claude/plans/declarative-knitting-hummingbird.md`.

Hard rule from the owner: the agent does everything itself and never hands a step to the user. When something is stuck or cannot be recovered it is recreated (the redroid container and volume, then the VM through Terraform after checking tfstate); vault secrets and tfstate history are never destroyed.
