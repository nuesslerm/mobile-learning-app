---
id: after-device-lab
title: after device-lab — loose ends, event waits instead of sleeps, sandbox-first CI; the emulator only in CI
status: queued
created: 2026-10-20
branch: claude/device-lab
plan: docs/plans/after-device-lab/spec.md
ledger: docs/plans/after-device-lab/progress.md
criteria:
  - id: a0.device-only-in-ci
    description: 'The rule is in the root instructions, and the older goal''s a1 and a3 read the device health the newest CI warm-up published, not a live device, with the same conditions; both pass.'
    command: |-
      grep -q 'emulator runs only in CI' .agents/instructions/root.md \
        && ! grep -A4 -E 'id: a[13]\.' docs/goals/2026-10-13-device-test-process.md | grep -q 'wf android features --json |' \
        && python3 -c "import yaml,subprocess,sys; t=open('docs/goals/2026-10-13-device-test-process.md').read().split('---')[1]; c={x['id']:x['command'] for x in yaml.safe_load(t)['criteria']}; sys.exit(any(subprocess.run(['bash','-c',c[i]]).returncode for i in ('a1.device-healthy','a3.slim-survives-restart')))"
  - id: a1.device-samples
    description: 'Every G1 to G3 Android unit of the newest CI run carries VM samples with the device''s memory and CPU.'
    command: |-
      wf test results --query "test=editor-typing,tamil-writing,voice-edit platform=android newest units" --json > /tmp/a1-results.json \
        && jq -e '.runs | length >= 3' /tmp/a1-results.json >/dev/null \
        && for b in $(jq -r '.runs[].bundle' /tmp/a1-results.json); do \
             curl -fsS "$(curl -fsS "$b" | jq -r '.report.json')" | jq -e '[.vm.samples[]? | select(.device.memMb != null and .device.cpuPct != null)] | length > 0' >/dev/null || exit 1; \
           done
  - id: a2.ci-usage-rows
    description: 'The newest green CI run on dev wrote a usage row for every wf command: its log never says "usage row not recorded".'
    command: |-
      id=$(gh run list --workflow deploy.yml --branch dev --status success --limit 1 --json databaseId --jq '.[0].databaseId') \
        && log=$(gh run view "$id" --log) && [ -n "$log" ] && ! grep -q 'usage row not recorded' <<<"$log"
  - id: a3.flow-warnings
    description: 'The newest CI flow run counted zero warnings, and a flow test that prints a warning fails (the harness gate CI''s flow stage runs, tested).'
    command: |-
      b=$(wf test results --query "test=mla-tamagui platform=flow limit=1" --json | jq -er '.runs[0].bundle') \
        && curl -fsS "$b" | jq -e '.metrics.warnings == 0' >/dev/null \
        && (cd frontend/libraries/flow-test-harness && pnpm exec vitest run --configLoader runner src/__tests__/warningGate.test.ts 2>&1 | grep -q 'Tests  3 passed')
  - id: a4.gaps-rechecked
    description: 'Every open gap carries a re-check dated 20 Oct or later, and no live text names code that is gone.'
    command: |-
      for f in $(grep -l '^status: open' docs/gaps/*.md); do grep -q '^## Re-checked 2026-1[0-2]-[2-3][0-9]' "$f" || exit 1; done \
        && wf check --dead-refs | jq -e '.ok' >/dev/null
  - id: a5.device-reruns
    description: 'A change to the device harness reruns the device tests, and an E2E-Fresh commit trailer runs a stage fresh; flow tested, and a pushed commit used the trailer.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf-plan --test flow -- nx_cache::nx_runs_an_e2e_test_exactly nx_cache::an_e2e_fresh_trailer_runs_the_stage_fresh 2>&1 | grep -q '^test result: ok. 2 passed' \
        && git fetch -q origin dev && git log --format=%B origin/dev | grep -q '^E2E-Fresh: android'
  - id: a6.no-failed-samples
    description: 'No VM sample failed in the newest G1 to G3 Android units.'
    command: |-
      wf test results --query "test=editor-typing,tamil-writing,voice-edit platform=android newest units" --json > /tmp/a6-results.json \
        && jq -e '.runs | length >= 3' /tmp/a6-results.json >/dev/null \
        && for b in $(jq -r '.runs[].bundle' /tmp/a6-results.json); do \
             curl -fsS "$(curl -fsS "$b" | jq -r '.report.json')" | jq -e '.vm.errors == 0' >/dev/null || exit 1; \
           done
  - id: d0.oxlint-only
    description: 'The app and its feature packages lint with oxlint alone: each lint script runs oxlint, no ESLint config or dependency is left in them (the mobile-learn plugin is the oxlint JS plugin), and the dropped rules are gone from the plugin.'
    command: |-
      for p in frontend/mla-tamagui $(ls -d frontend/mla-tamagui/features/*/ | sed 's#/$##'); do \
        [ -f "$p/package.json" ] || continue; \
        jq -e '(.scripts.lint // "oxlint") | test("^oxlint")' "$p/package.json" >/dev/null || { echo "$p lint is not oxlint"; exit 1; }; \
        jq -e '[(.dependencies // {}), (.devDependencies // {}) | keys[] | select(test("eslint") and . != "@lib/eslint-plugin-mobile-learn")] | length == 0' "$p/package.json" >/dev/null || { echo "$p keeps eslint deps"; exit 1; }; \
      done \
        && ! ls frontend/mla-tamagui/eslint.config.mjs frontend/feature-kit/eslint.feature.mjs 2>/dev/null \
        && ! ls libraries/eslint-plugin-mobile-learn/src/rules/require-log-in-unit.ts libraries/eslint-plugin-mobile-learn/src/rules/require-testid-on-element.ts libraries/eslint-plugin-mobile-learn/src/rules/require-testid-on-interactive.ts 2>/dev/null \
        && pnpm --filter mla-tamagui run lint >/dev/null
  - id: d1.kept-rules-fire
    description: 'Each kept rule fires on a planted case under oxlint, tested.'
    command: |-
      pnpm --filter @lib/eslint-plugin-mobile-learn exec vitest run tests/oxlint.test.ts 2>&1 | grep -q 'passed'
  - id: d4.promise-rules
    description: 'The promise rules run type-aware in both oxlint configs, a planted case per rule fires, and the workspace lints clean.'
    command: |-
      grep -q '"typeAware": true' .oxlintrc.json && grep -q '"typeAware": true' frontend/.oxlintrc.json \
        && pnpm --filter @lib/eslint-plugin-mobile-learn exec vitest run tests/oxlint.test.ts 2>&1 | grep -q '2 passed' \
        && pnpm run lint >/dev/null 2>&1
  - id: d4b.useful-rules
    description: 'The lint plugin carries only the rules that earned their place, its tests run on oxlint, and nothing installs typescript-eslint or ESLint.'
    command: |-
      [ "$(ls libraries/eslint-plugin-mobile-learn/src/rules | grep -c '^no-')" = 2 ] \
        && ! git grep -q "@typescript-eslint\|from 'eslint'" -- libraries/eslint-plugin-mobile-learn \
        && ! grep -qE '^  (eslint|@typescript-eslint/[a-z-]+)@' pnpm-lock.yaml \
        && pnpm --filter @lib/eslint-plugin-mobile-learn exec vitest run >/dev/null 2>&1
  - id: d5.ts7
    description: 'tsc is TypeScript 7 in every package, typescript is the 6.0 API package, the workspace typechecks and the architecture gate still cruises the app.'
    command: |-
      pnpm exec tsc --version | grep -q 'Version 7\.' \
        && node -e "process.exit(require('typescript').version.startsWith('6.') ? 0 : 1)" \
        && pnpm -r --no-bail run --if-present typecheck >/dev/null 2>&1 \
        && pnpm --filter mla-tamagui run arch 2>&1 | grep -qE '[1-9][0-9]* modules'
  - id: d6.measured
    description: 'Typecheck, lint and pre-commit gate timing before and after D4 and D5 (n=3) is in the decision log.'
    command: |-
      grep -q 'D6 TypeScript 7 and type-aware lint timing' docs/audits/2026-09-02-token-diet-decision-log.md
  - id: d2.measured
    description: 'The app lint wall time with ESLint and with oxlint (n=3, cold cache) is in the decision log.'
    command: |-
      grep -q 'D2 lint timing (ESLint against oxlint)' docs/audits/2026-09-02-token-diet-decision-log.md
  - id: b0.baseline
    description: 'The per-unit timing baseline of both platforms is committed once and never changed.'
    command: |-
      f=docs/runbooks/.results/e2e-unit-timing-initial.json \
        && jq -e '(.android | length) >= 21 and (.browser | length) >= 21' "$f" >/dev/null \
        && [ "$(git log --format=%H -- "$f" | wc -l)" -eq 1 ]
  - id: b1.wait-verbs
    description: 'The wait verbs the scripts need return on the event on both drivers, flow tested.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf-harness --test flow -- harness::the_wait_verbs_return_on_the_event 2>&1 | grep -qE "^test result: ok\. [1-9][0-9]* passed"
  - id: b2.no-fixed-sleeps
    description: 'No fixed sleep is left in the G1 to G3 scripts but the real-time length of a voice clip.'
    command: |-
      ! grep -n 'sleep(' e2e/editor-typing.lua e2e/tamil-writing.lua \
        && ! grep -n 'sleep(' e2e/voice-edit.lua | grep -v 'sleep(clip_s)'
  - id: b3.faster
    description: 'Every G1 to G3 unit''s median over its newest three CI runs is at or below the baseline, on both platforms.'
    command: |-
      python3 - <<'PY'
      import json, statistics, subprocess, sys
      base = json.load(open("docs/runbooks/.results/e2e-unit-timing-initial.json"))
      for platform, units in base.items():
          q = f"test=editor-typing,tamil-writing,voice-edit platform={platform} since=14d"
          runs = json.loads(subprocess.run(["wf", "test", "results", "--query", q, "--json"], capture_output=True, text=True).stdout)["runs"]
          for key, initial in units.items():
              mine = [r["durationS"] for r in runs if f"{r['test']} {r.get('unit') or ''}".strip() == key and r["verdict"] == "pass"][:3]
              if len(mine) < 3 or statistics.median(mine) > initial:
                  print(platform, key, mine, initial); sys.exit(1)
      PY
  - id: s0.benchmark
    description: 'The EAS Update against Metro benchmark (n=3, in CI) and its decision are logged.'
    command: |-
      grep -q 'S0 benchmark (EAS Update against Metro)' docs/audits/2026-09-02-token-diet-decision-log.md
  - id: s1.crash-handler
    description: 'The app logs app.crash for an uncaught error or rejection, flow tested.'
    command: |-
      grep -rq "'app.crash'" frontend/mla-tamagui/app-shell \
        && pnpm --filter mla-tamagui exec vitest run --config vitest.flow.config.ts --configLoader runner tests/flow/crashHandler.flow.test.tsx >/dev/null 2>&1
  - id: s2.eas-update-job
    description: 'CI has an eas-update job before the Android tests, and the newest run that changed the app published from it.'
    command: |-
      grep -q 'eas-update' .github/workflows/pipeline.yml && actionlint .github/workflows/pipeline.yml
  - id: s3.android-on-update
    description: 'The newest G1 to G3 Android units ran against an EAS Update, not the dev Metro.'
    command: |-
      wf test results --query "test=editor-typing,tamil-writing,voice-edit platform=android newest units" --json \
        | jq -e '.runs | length >= 3 and all(.args.base | test("^exp://u\\.expo\\.dev/"))'
  - id: s4.remote-cache
    description: 'A sandbox pass at a pushed commit replays on CI at that commit (remote Nx cache, clean-tree writes), flow tested.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf-plan --test flow -- nx_cache::a_sandbox_pass_replays_on_ci 2>&1 | grep -qE '^test result: ok\. [1-9][0-9]* passed'
  - id: s5.replay-and-drift
    description: 'The pre-commit gate runs through Nx, CI replays what the sandbox ran, and the drift contract holds.'
    command: |-
      cargo test --manifest-path clis/Cargo.toml --release -p wf-plan --test flow -- nx_cache::the_sandbox_and_the_runner_hash_alike check::the_staged_gate_runs_through_nx 2>&1 | grep -q '^test result: ok. 2 passed'
  - id: h0.spec
    description: 'The harness API spec and its implementation contract exist; the spec names every command of the plan''s table, and the contract every device command (run_flow and assert_true are Lua globals, outside its command table).'
    command: |-
      d=docs/plans/harness-api; test -f "$d/spec.md" && test -f "$d/contract.md" \
        && for c in open_link tap_on swipe scroll_until_visible input_text press_key back extended_wait_until assert_visible assert_not_visible take_screenshot assert_screenshot start_recording stop_recording get_element get_device_state get_pixel get_logs clear_logs set_microphone_input draw_strokes_on run_shell add_network_stub get_network_stub_hits get_debug_hits run_flow assert_true; do \
             grep -qw "$c" "$d/spec.md" || { echo "spec.md misses $c"; exit 1; }; \
             case "$c" in run_flow|assert_true) ;; *) grep -qw "$c" "$d/contract.md" || { echo "contract.md misses $c"; exit 1; };; esac; \
           done
  - id: h1.parity
    description: 'wf browser test --api --json and wf android test --api --json list the same device commands, which are the Lua binder''s, and the parity flow test passes.'
    command: |-
      b=$(wf browser test --api --json | jq -ce '[.commands[].name] | sort | select(length > 0)') \
        && a=$(wf android test --api --json | jq -ce '[.commands[].name] | sort | select(length > 0)') \
        && [ "$b" = "$a" ] \
        && cargo test --manifest-path clis/Cargo.toml --release -p wf --test flow -- surface:: 2>&1 | grep -qE '^test result: ok\. [1-9][0-9]* passed'
  - id: h7.scripts
    description: 'No e2e/*.lua calls an old device method.'
    command: |-
      ! grep -nE 'device:(tap|find|wait|appears|wait_slow|wait_gone|wait_gone_slow|wait_keyboard|wait_recording|wait_pixel|wait_editor_longer|text|editor_text|pixel|keyboard_shown|audio_inject|trace|stroke|press|type_text|open|screenshot|golden|warnings|clear_warnings|shell|stub|unstub|stub_hits|debug|debug_hits|record_start|record_stop|step|span)\(' e2e/*.lua
  - id: h8.ci
    description: 'The newest completed CI run on dev has browser-e2e-tests and android-e2e-tests green.'
    command: |-
      id=$(gh run list --workflow deploy.yml --branch dev --status completed --limit 1 --json databaseId --jq '.[0].databaseId') \
        && gh run view "$id" --json jobs --jq '[.jobs[] | select(.name | test("^(browser|android)-e2e-tests"))]' \
        | jq -e 'all(.conclusion == "success") and any(.name | startswith("browser-e2e-tests")) and any(.name | startswith("android-e2e-tests"))' >/dev/null
---

# After device-lab

The spec is `docs/plans/after-device-lab/spec.md`; the ledger is `docs/plans/after-device-lab/progress.md`. The emulator runs only in CI (owner, 20 Oct): no criterion here may need a device brought up by hand, and every device measurement comes from CI runs.

A criterion that names a test or file that does not exist yet is refined when its item is built; a refinement never asks for less than its description.
