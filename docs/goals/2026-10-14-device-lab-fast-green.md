---
id: device-lab-fast-green
title: device-lab fast and green — every gate green on Android and in the browser, Android under 5 min, records true, debugger and goldens
status: queued
created: 2026-10-14
branch: claude/device-lab
plan: docs/plans/device-test-process/plan-2026-10-14.md
ledger: docs/plans/device-test-process/progress-2026-10-14.md
criteria:
  - id: p0.down-is-clean
    description: '`wf android stop-device` stops Android with time to write its state before the container is removed, and the flow tests prove it.'
    command: |-
      wf android stop-device --dry-run | grep -q 'docker stop -t 30' \
        && cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- android::the_lab_scripts 2>&1 | grep -q '^test result: ok. 1 passed'
  - id: p0.one-device
    description: 'One device: the default and only lane is rdl, booted at 480x1040@213 (a Galaxy S25 in dp, 360 x 780) and healthy. The device is up only while an Android e2e job runs (p4), so when it is down the check reads that job: its health step gates the cold start, and the newest cold start (warm-app) of the last two days passed.'
    command: |-
      wf android list-devices > /tmp/p0-devices.json 2>/dev/null; \
      if jq -e '.result.devices | length > 0' /tmp/p0-devices.json >/dev/null; then \
        jq -e '.result.devices[0] | .boot.completed == true and .boot.activityService == true and .restartProven == true and .disabledTotal >= 40' /tmp/p0-devices.json; \
      else \
        jq -e '.ok == true and .result.devices == []' /tmp/p0-devices.json >/dev/null \
          && grep -A6 'name: The device is healthy' .github/workflows/e2e-tests.yml | grep -q 'restartProven == true and .disabledTotal >= 40' \
          && wf test results --query "test=warm-app platform=android since=2d limit=1" --json | jq -e '.runs[0].verdict == "pass"'; \
      fi
  - id: p1.timing-in-results
    description: 'Every published e2e run carries a timing table (wall seconds, and calls and seconds per device verb), flow tested; the newest run of every G1, G2 and G3 unit has one.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- pipeline:: 2>&1 | grep -q '^test result: ok. 4 passed' \
        && wf test results --query "test=editor-typing,tamil-writing,voice-edit platform=android newest units" --json | jq -e '.runs | length >= 3 and all(.missing != true and .wallS > 0 and (.byVerb | length) > 0)' >/dev/null
  - id: p2.android-green
    description: 'The newest published run of every G1, G2 and G3 Android unit passes on the 480x1040@213 screen, at a commit at or after the last app change, with zero JS warnings and recordings.'
    command: |-
      last=$(git log -1 --format=%H -- frontend/mla-tamagui/features frontend/mla-tamagui/app) \
        && shas=$(wf test results --query "test=editor-typing,tamil-writing,voice-edit platform=android newest units" --json | jq -er '.runs | if length >= 3 and all(.missing != true and .verdict == "pass" and .screen == "480x1040@213" and .warnings == 0 and .recordings > 0) then .[].sha else error("not green") end') \
        && for s in $shas; do git merge-base --is-ancestor "$last" "$s" || exit 1; done
  - id: p2.harness-hygiene
    description: 'wf android run reuses the running Expo Go and force-stops it with --cold-start; wf test run starts it cold once per job, after a failed unit and after a Metro move (owner, 16 Oct); the dead UI server restart and paced strokes are flow tested; the Metro wait uses the deploy rule.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- android::a_runbook_drives_the_screen_through_the_on_device_ui_server harness:: 2>&1 | grep -qE '^test result: ok\. [1-9][0-9]* passed; 0 failed' \
        && grep -q 'force-stop' clis/wf-harness/src/run.rs \
        && grep -q 'cold-start' clis/wf/src/e2e_run.rs \
        && grep -q 'affected' clis/wf/src/e2e_tests.rs
  - id: p3.android-fast
    description: 'Every G1, G2 and G3 Android unit finishes in under 60 seconds (owner, 15 Oct; it was 5 minutes per gate) with the same gates.'
    command: |-
      wf test results --query "test=editor-typing,tamil-writing,voice-edit platform=android newest units" --json | jq -e '.runs | length >= 3 and all(.missing != true and .durationS < 60)' >/dev/null
  - id: p4.device-only-while-testing
    description: 'The post-deploy CI job brings the device up before the queue and always takes it down after, so it holds no memory between runs.'
    command: |-
      grep -q 'wf android start-device' .github/workflows/e2e-tests.yml \
        && grep -q 'wf android stop-device' .github/workflows/e2e-tests.yml \
        && grep -B4 'wf android stop-device' .github/workflows/e2e-tests.yml | grep -q 'if: always()'
  - id: p5.published
    description: 'Every recording of the newest G1, G2 and G3 Android runs was read back from the bucket, those runs are on a test report page, and each browser result links its evidence page with its recordings read back.'
    command: |-
      wf test results --query "test=editor-typing,tamil-writing,voice-edit platform=android newest units" --json > /tmp/p5-results.json \
        && jq -e '.runs | length >= 3 and all(.missing != true and .readBack == true)' /tmp/p5-results.json >/dev/null \
        && newest=$(jq -r '[.runs[].publishedAt] | max' /tmp/p5-results.json) \
        && git fetch -q origin dev && git show origin/dev:docs/runbooks/.results/test-report-state.json | jq -e --arg n "$newest" '(.lastReport | startswith("https://claude.ai/")) and .lastPublishedAt >= $n' >/dev/null \
        && for g in device-lab-codemirror-typing device-lab-tamil-writing device-lab-voice-edit; do \
             jq -e '((.evidence.page // .page) | startswith("https://claude.ai/artifact/")) and .recordingsReadBack == true' docs/runbooks/.results/$g.json >/dev/null || exit 1; \
           done
  - id: p6.browser-green
    description: 'G1, G2 and G3 pass in the browser at or after the last app change, every current unit, read from the published runs; G1 ran on the web export CI builds (since 22 Oct; the dev Metro is retired), whose app calls the deployed remote-chat-agent through the backend hosts baked into it.'
    command: |-
      last=$(git log -1 --format=%H -- frontend/mla-tamagui/features frontend/mla-tamagui/app) \
        && wf test results --query "test=editor-typing,tamil-writing,voice-edit platform=browser newest units" --json > /tmp/p6-results.json \
        && jq -e '(.runs | length) >= 3 and (.runs | all(.missing != true and .verdict == "pass"))' /tmp/p6-results.json >/dev/null \
        && for s in $(jq -r '.runs[].sha' /tmp/p6-results.json | sort -u); do git merge-base --is-ancestor "$last" "$s" || exit 1; done \
        && jq -e '[.runs[] | select(.test == "voice-edit")] | length > 0 and all(.args.base | startswith("http://127.0.0.1:"))' /tmp/p6-results.json >/dev/null
  - id: p7.records-true
    description: 'No record claims the deployed planner is broken, the new gaps are recorded (shared Metro restarts, closed with the Metro on 23 Oct; VM memory saturation), and dev carries the work.'
    command: |-
      ! grep -rqiE 'deployed planner (still|is still) (broken|times out)|deployed one still times out' \
          docs/plans/device-test-process docs/audits/2026-10-06-app-bench-report.md docs/audits/2026-10-06-app-bench-baseline.md docs/runbooks/.results \
        && test -f docs/archive/gaps/shared-metro-restarts-on-app-push.md && grep -q 'vm-memory-saturation' docs/known-gaps.md \
        && git fetch -q origin dev && git merge-base --is-ancestor HEAD origin/dev
  - id: p8.agent-proof
    description: 'The agent proof benchmark ran (an agent writes one Lua script from wf browser run --api for the four-step task) and the wait budget was set from recorded timings.'
    command: |-
      jq -e '(.arms | length) >= 2 and all(.arms[]; .wallS > 0 and (.passed | type) == "boolean") and .waitBudgetMs > 0' docs/runbooks/.results/agent-proof.json \
        && grep -A3 'fn wait_budget_ms' clis/wf-harness/src/device/mod.rs | sed -E 's/([0-9])_([0-9])/\1\2/g' | grep -q "unwrap_or($(jq -r .waitBudgetMs docs/runbooks/.results/agent-proof.json))"
  - id: p9.debugger
    description: 'Pause, step, resume and eval in the browser debugger, hits joined to input actions, flow tests stoppable at a line; hit-record tests pass. SUPERSEDED on 22 Oct (Metro retirement M3): the Android half (--debug through Expo Go''s Hermes runtime with a debugStatus block in the report) was deleted with the dev Metro, so this checks the browser half only.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- browser::an_agent_debugs 2>&1 | grep -q '^test result: ok. 1 passed' \
        && wf browser --help | grep -q 'debug-resume' && wf browser --help | grep -q 'debug-step' && wf browser --help | grep -q 'debug-eval' \
        && pnpm --filter mla-tamagui run test:flow:debug
  - id: p10.goldens
    description: 'Golden images with sidecar JSON for the flow, browser and Android harnesses, --update-goldens, mismatch diffs, a gallery per harness.'
    command: |-
      wf android test --api | grep -q 'device:assert_screenshot' \
        && cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- harness::goldens harness::a_flow_file_with_update_goldens 2>&1 | grep -q '^test result: ok. 2 passed' \
        && pnpm --filter mla-tamagui run test:goldens \
        && for h in browser android; do ls docs/runbooks/goldens/$h/*.png >/dev/null 2>&1 && ls docs/runbooks/goldens/$h/*.json >/dev/null 2>&1 || exit 1; done
  - id: z.ledger-complete
    description: 'Every box in the ledger is ticked and no Blocker is open.'
    command: |-
      ! grep -q '^- \[ \]' docs/plans/device-test-process/progress-2026-10-14.md && ! grep -q '^- Blocker:' docs/plans/device-test-process/progress-2026-10-14.md
  - id: z.pushed-and-clean
    description: 'claude/device-lab is at HEAD, HEAD is on dev, nothing is left running in the sandbox.'
    command: |-
      git fetch -q origin dev claude/device-lab \
        && [ "$(git rev-parse origin/claude/device-lab)" = "$(git rev-parse HEAD)" ] \
        && git merge-base --is-ancestor HEAD origin/dev \
        && ! ps -eo args | grep -qE '[s]taticServer.mjs|[r]ca-service|[o]pencode serve|[s]tt/server.py'
---

# device-lab fast and green

The plan is `docs/plans/device-test-process/plan-2026-10-14.md` (phases 0 to 12, owner's items A0 to A5 and B6 to B13 kept with their numbers). The ledger is `docs/plans/device-test-process/progress-2026-10-14.md`. One device only; the second device, parallel runs and sleep/wake were dropped by the owner.
