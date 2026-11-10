---
id: device-lab
title: device-lab — a browser render you can trust for native
status: queued
created: 2026-10-04
branch: claude/device-lab
criteria:
  # ---- Phase 0: spec with the API breakdown, clean render container, ledger, landed on dev.
  - id: p0.spec-breakdown
    phase: 0
    description: 'docs/plans/device-lab/spec.md carries the breakdown table between its markers: at least 40 rows, each with a tier T1-T4, a priority P1-P3 and a status todo|done|residual.'
    command: |-
      node -e "
      const s = require('fs').readFileSync('docs/plans/device-lab/spec.md', 'utf8');
      const t = (s.split('<!-- breakdown:start -->')[1] || '').split('<!-- breakdown:end -->')[0] || '';
      const rows = t.split('\n').filter((l) => /^\| [A-Z]\d+ \|/.test(l)).map((l) => l.split('|').map((c) => c.trim()));
      const bad = rows.filter((r) => !/^T[1-4]$/.test(r[5]) || !/^P[1-3]$/.test(r[6]) || !/^(todo|done|residual)$/.test(r[7]));
      if (rows.length < 40 || bad.length) { console.error(rows.length + ' rows; malformed: ' + bad.map((r) => r[1]).join(',')); process.exit(1); }
      "
  - id: p0.render-container
    phase: 0
    description: 'The harness shell renders a clean screen by default: no option bar (DevicePicker removed), a screen-only container (.mcp-screen) with the bezel opt-in via frame=bezel, and the shell typechecks.'
    command: |-
      ! test -e clis/multi-browser/harness-shell/ui/src/DevicePicker.tsx \
        && grep -q 'mcp-screen' clis/multi-browser/harness-shell/ui/src/DeviceFrame.tsx \
        && grep -q "frame" clis/multi-browser/harness-shell/data/src/params.ts \
        && pnpm --config.verify-deps-before-run=false --filter @cli/multi-browser/harness-shell run typecheck
  - id: p0.progress-ledger
    phase: 0
    description: 'docs/plans/device-lab/progress.md exists and records phase 0 with evidence.'
    command: |-
      grep -q '^## P0' docs/plans/device-lab/progress.md
  - id: p0.on-dev
    phase: 0
    description: 'Phase 0 is landed: this goal file exists on origin/dev.'
    command: |-
      git fetch -q origin dev && git cat-file -e origin/dev:docs/goals/2026-10-04-device-lab.md

  # ---- Phase 1: contract and units derived from the breakdown.
  - id: p1.contract
    phase: 1
    description: 'docs/plans/device-lab/contract.md pins the seams every unit builds against: the wf browser driver commands (name, args, result), the lab module API and its gate/sentinel, and the static guard rules.'
    command: |-
      f=docs/plans/device-lab/contract.md
      test -s "$f" && grep -qi 'driver commands' "$f" && grep -qi 'lab module' "$f" && grep -qi 'static guard' "$f" && grep -q '__device_lab_v1__' "$f"
  - id: p1.units
    phase: 1
    description: 'docs/plans/device-lab/units.jsonl exists and passes the plan checker, `wf offload plan --check`: no parse errors, cycles or missing gates, and 1-3 files per unit.'
    command: |-
      test -s docs/plans/device-lab/units.jsonl && wf offload plan --check device-lab

  # ---- Phase 2: lab export gate. Lab code ships only when EXPO_PUBLIC_DEVICE_LAB=1.
  - id: p2.lab-flag
    phase: 2
    description: 'export:web sets EXPO_PUBLIC_DEVICE_LAB=1 and the root layout reaches the lab module only through a gated require.'
    command: |-
      grep -q 'EXPO_PUBLIC_DEVICE_LAB=' frontend/mla-tamagui/package.json \
        && grep -q "EXPO_PUBLIC_DEVICE_LAB === '1'" frontend/mla-tamagui/app/_layout.tsx
  - id: p2.prod-clean
    phase: 2
    description: 'A flagless web export (the production configuration) contains no lab code: the sentinel __device_lab_v1__ is absent.'
    command: |-
      rm -rf /tmp/device-lab-prod && cd frontend/mla-tamagui \
        && EXPO_PUBLIC_DEVICE_LAB= npx expo export --platform web --output-dir /tmp/device-lab-prod > /tmp/device-lab-prod.log 2>&1 \
        && ! grep -rqs '__device_lab_v1__' /tmp/device-lab-prod
  - id: p2.lab-sentinel
    phase: 2
    description: 'A flagged web export contains the lab module (sentinel present), proving the gate is wired rather than dead.'
    command: |-
      rm -rf /tmp/device-lab-lab && cd frontend/mla-tamagui \
        && EXPO_PUBLIC_DEVICE_LAB=1 npx expo export --platform web --output-dir /tmp/device-lab-lab > /tmp/device-lab-lab.log 2>&1 \
        && grep -rqs '__device_lab_v1__' /tmp/device-lab-lab

  # ---- Phase 3: Chromium emulation (tier T2) through wf browser.
  - id: p3.t2-commands
    phase: 3
    description: 'wf browser exposes the T2 driver commands the contract names: permission, camera-inject, upload, clipboard, opened-urls, and for human input swipe, trace, press and video-check.'
    command: |-
      f=clis/multi-browser/src/entrypoints/cli.ts
      for c in permission camera-inject upload clipboard opened-urls swipe trace press video-check; do grep -q "command('$c')" "$f" || { echo "missing: $c"; exit 1; }; done
  - id: p3.t2-e2e
    phase: 3
    description: 'The device-lab e2e suite drives every T2 row headlessly against a real Chromium (device metrics, safe area, fonts, permission deny/grant, fake camera, file upload, clipboard, external URLs, live network through Node, touch taps and human strokes, soft-keyboard typing at the caret, the input log and video-check) and passes.'
    command: |-
      cd clis/multi-browser && pnpm --config.verify-deps-before-run=false exec vitest run tests/e2e/deviceLab.e2e.test.ts
  - id: p3.t2-rows
    phase: 3
    description: 'No P1/P2 row with tier T2 in the spec breakdown is still todo.'
    command: |-
      node -e "
      const s = require('fs').readFileSync('docs/plans/device-lab/spec.md', 'utf8');
      const t = (s.split('<!-- breakdown:start -->')[1] || '').split('<!-- breakdown:end -->')[0] || '';
      const rows = t.split('\n').filter((l) => /^\| [A-Z]\d+ \|/.test(l)).map((l) => l.split('|').map((c) => c.trim()));
      const bad = rows.filter((r) => r[5] === 'T2' && /^P[12]$/.test(r[6]) && r[7] === 'todo');
      if (bad.length) { console.error(bad.map((r) => r[1] + ' ' + r[2]).join('\n')); process.exit(1); }
      "

  # ---- Phase 4: acceptance gates (spec §7), each run by an agent as a person would and recorded.
  - id: p4.gate-voice-edit
    phase: 4
    description: 'G1 (spec §7): the voice-edit problems are solved by speech alone, every tap a touch tap shown as a dot in the video. An agent ran docs/runbooks/device-lab-voice-edit.md on the lab export under the human input model and recorded it. The result record passes, its sha is on this history, the runbook is unchanged since the run, and progress.md ticks G1 with the published recording.'
    command: |-
      node -e "
      const { execSync } = require('child_process');
      const fs = require('fs');
      const name = 'device-lab-voice-edit';
      const f = 'docs/runbooks/.results/' + name + '.json';
      if (!fs.existsSync(f)) { console.error(name + ': no result record'); process.exit(1); }
      const r = JSON.parse(fs.readFileSync(f, 'utf8'));
      if (r.verdict !== 'pass') { console.error(name + ': verdict ' + r.verdict); process.exit(1); }
      try { execSync('git merge-base --is-ancestor ' + r.sha + ' HEAD'); } catch { console.error(name + ': ' + r.sha + ' is not on this history'); process.exit(1); }
      try { execSync('git diff --quiet ' + r.sha + ' HEAD -- docs/runbooks/' + name + '.md'); } catch { console.error(name + ': the runbook changed after the run'); process.exit(1); }
      " && grep -Eq '^- \[x\] G1 .*https://objectstorage' docs/plans/device-lab/progress.md
  - id: p4.gate-tamil-writing
    phase: 4
    description: 'G2 (spec §7): Tamil letters traced on the lesson pad score at least 80 % on the line, and a large letter and three words written over the template are marked LOOKS RIGHT by the live marker. An agent ran docs/runbooks/device-lab-tamil-writing.md on the lab export under the human input model and recorded it. The result record passes, its sha is on this history, the runbook is unchanged since the run, and progress.md ticks G2 with the published recording.'
    command: |-
      node -e "
      const { execSync } = require('child_process');
      const fs = require('fs');
      const name = 'device-lab-tamil-writing';
      const f = 'docs/runbooks/.results/' + name + '.json';
      if (!fs.existsSync(f)) { console.error(name + ': no result record'); process.exit(1); }
      const r = JSON.parse(fs.readFileSync(f, 'utf8'));
      if (r.verdict !== 'pass') { console.error(name + ': verdict ' + r.verdict); process.exit(1); }
      try { execSync('git merge-base --is-ancestor ' + r.sha + ' HEAD'); } catch { console.error(name + ': ' + r.sha + ' is not on this history'); process.exit(1); }
      try { execSync('git diff --quiet ' + r.sha + ' HEAD -- docs/runbooks/' + name + '.md'); } catch { console.error(name + ': the runbook changed after the run'); process.exit(1); }
      " && grep -Eq '^- \[x\] G2 .*https://objectstorage' docs/plans/device-lab/progress.md
  - id: p4.gate-codemirror-typing
    phase: 4
    description: 'G3 (spec §7): two solutions are typed into CodeMirror key by key through the phone keyboard and toolbar, with no paste or bulk insert, and Run Local passes every test. An agent ran docs/runbooks/device-lab-codemirror-typing.md on the lab export under the human input model and recorded it. The result record passes, its sha is on this history, the runbook is unchanged since the run, and progress.md ticks G3 with the published recording.'
    command: |-
      node -e "
      const { execSync } = require('child_process');
      const fs = require('fs');
      const name = 'device-lab-codemirror-typing';
      const f = 'docs/runbooks/.results/' + name + '.json';
      if (!fs.existsSync(f)) { console.error(name + ': no result record'); process.exit(1); }
      const r = JSON.parse(fs.readFileSync(f, 'utf8'));
      if (r.verdict !== 'pass') { console.error(name + ': verdict ' + r.verdict); process.exit(1); }
      try { execSync('git merge-base --is-ancestor ' + r.sha + ' HEAD'); } catch { console.error(name + ': ' + r.sha + ' is not on this history'); process.exit(1); }
      try { execSync('git diff --quiet ' + r.sha + ' HEAD -- docs/runbooks/' + name + '.md'); } catch { console.error(name + ': the runbook changed after the run'); process.exit(1); }
      " && grep -Eq '^- \[x\] G3 .*https://objectstorage' docs/plans/device-lab/progress.md

  # ---- Phase 5: in-app lab mocks (tier T3).
  - id: p5.t3-rows
    phase: 5
    description: 'No P1/P2 row with tier T3 is still todo, and the app flow tests are green.'
    command: |-
      node -e "
      const s = require('fs').readFileSync('docs/plans/device-lab/spec.md', 'utf8');
      const t = (s.split('<!-- breakdown:start -->')[1] || '').split('<!-- breakdown:end -->')[0] || '';
      const rows = t.split('\n').filter((l) => /^\| [A-Z]\d+ \|/.test(l)).map((l) => l.split('|').map((c) => c.trim()));
      const bad = rows.filter((r) => r[5] === 'T3' && /^P[12]$/.test(r[6]) && r[7] === 'todo');
      if (bad.length) { console.error(bad.map((r) => r[1] + ' ' + r[2]).join('\n')); process.exit(1); }
      " && pnpm --config.verify-deps-before-run=false --filter mla-tamagui run test:flow

  # ---- Phase 6: static guards (tier T4).
  - id: p6.t4-rows
    phase: 6
    description: 'No P1/P2 row with tier T4 is still todo; the ESLint plugin tests and the app lint are green.'
    command: |-
      node -e "
      const s = require('fs').readFileSync('docs/plans/device-lab/spec.md', 'utf8');
      const t = (s.split('<!-- breakdown:start -->')[1] || '').split('<!-- breakdown:end -->')[0] || '';
      const rows = t.split('\n').filter((l) => /^\| [A-Z]\d+ \|/.test(l)).map((l) => l.split('|').map((c) => c.trim()));
      const bad = rows.filter((r) => r[5] === 'T4' && /^P[12]$/.test(r[6]) && r[7] === 'todo');
      if (bad.length) { console.error(bad.map((r) => r[1] + ' ' + r[2]).join('\n')); process.exit(1); }
      " && pnpm --config.verify-deps-before-run=false --filter @lib/eslint-plugin-mobile-learn run test \
        && pnpm --config.verify-deps-before-run=false --filter mla-tamagui run lint

  # ---- Phase 7: done.
  - id: p7.all-rows
    phase: 7
    description: 'Every P1/P2 row in the breakdown is done or residual.'
    command: |-
      node -e "
      const s = require('fs').readFileSync('docs/plans/device-lab/spec.md', 'utf8');
      const t = (s.split('<!-- breakdown:start -->')[1] || '').split('<!-- breakdown:end -->')[0] || '';
      const rows = t.split('\n').filter((l) => /^\| [A-Z]\d+ \|/.test(l)).map((l) => l.split('|').map((c) => c.trim()));
      const bad = rows.filter((r) => /^P[12]$/.test(r[6]) && r[7] === 'todo');
      if (bad.length) { console.error(bad.map((r) => r[1] + ' ' + r[2]).join('\n')); process.exit(1); }
      "
  - id: p7.app-green
    phase: 7
    description: 'The app typechecks, its architecture check passes and its flow tests are green on the final tree.'
    command: |-
      pnpm --config.verify-deps-before-run=false --filter mla-tamagui run typecheck \
        && pnpm --config.verify-deps-before-run=false --filter mla-tamagui run arch \
        && pnpm --config.verify-deps-before-run=false --filter mla-tamagui run test:flow
  - id: p7.gates-fresh
    phase: 7
    description: 'The last passing run of each gate postdates every change to the code it exercises: no commit after the recorded sha touches that gate''s feature, the browser driver or the root layout.'
    command: |-
      node -e "
      const { execSync } = require('child_process');
      const fs = require('fs');
      const driver = 'clis/multi-browser/src clis/multi-browser/harness-shell frontend/libraries/gesture-geometry frontend/mla-tamagui/app/_layout.tsx';
      const gates = {
        'device-lab-voice-edit': 'frontend/mla-tamagui/features/leetcode frontend/libraries/voice-edit-dsl clis/harness/src',
        'device-lab-tamil-writing': 'frontend/mla-tamagui/features/tamil-writing',
        'device-lab-codemirror-typing': 'frontend/mla-tamagui/features/leetcode/code-editor frontend/mla-tamagui/features/leetcode/ui',
      };
      const stale = [];
      for (const [name, paths] of Object.entries(gates)) {
        const f = 'docs/runbooks/.results/' + name + '.json';
        const r = fs.existsSync(f) ? JSON.parse(fs.readFileSync(f, 'utf8')) : null;
        if (!r || r.verdict !== 'pass') { stale.push(name + ' (no passing record)'); continue; }
        try { execSync('git diff --quiet ' + r.sha + ' HEAD -- ' + paths + ' ' + driver); } catch { stale.push(name + ' (code changed since ' + r.sha.slice(0, 8) + ')'); }
      }
      if (stale.length) { console.error('re-run: ' + stale.join(', ')); process.exit(1); }
      "
  - id: p7.ledger-complete
    phase: 7
    description: 'progress.md has no unticked box left for phases 0-7.'
    command: |-
      ! grep -q '^- \[ \]' docs/plans/device-lab/progress.md
---

# device-lab — a browser render you can trust for native

Spec: [`docs/plans/device-lab/spec.md`](../plans/device-lab/spec.md). Ledger: [`docs/plans/device-lab/progress.md`](../plans/device-lab/progress.md). Tracking PR: [#306](https://github.com/nuesslerm/expo-hello-world-app-2/pull/306).

## Why this goal exists

The app is checked almost entirely in a browser (flow tests, tier-2 screenshots, Playwright e2e), but it ships to iOS and Android. Wherever react-native-web or the browser diverges from native, a green browser run proves nothing about the phone. Two examples are already in the tree: the voice upload takes a different `FormData` branch on native (`voiceEditTransport.ts:96-106`), and every device frame renders the app with zero safe-area insets. This goal drives the spec's breakdown to completion: each divergent API gets a faithful browser mock (Chromium emulation first, an in-app lab mock second) or a static guard, and an agent can drive all of it headlessly.

## How it runs

Fully autonomous, no review gates (owner decision 2026-10-04). The goal runner (`wf goal`) went in the greenfield P6 slimming, so a criterion is met when its `command` exits 0, run by hand. This lists every unmet criterion:

```bash
python3 -c "import re,subprocess,sys,yaml; m=re.match(r'---\n(.*?)\n---\n', open('docs/goals/2026-10-04-device-lab.md').read(), re.S); bad=[c['id'] for c in yaml.safe_load(m.group(1))['criteria'] if subprocess.run(['bash','-c',c['command']], capture_output=True).returncode]; print('unmet:', ' '.join(bad) or 'none'); sys.exit(1 if bad else 0)"
```

Each iteration pulls `dev`, implements the first unmet criterion, commits through the lefthook pre-commit hook (`wf check --staged`), and lands on `dev`. The goal is done when that command exits 0.

The phase 4 criteria are the owner's acceptance gates (spec §7). Each is a runbook an agent runs the way a person would, through the headless browser SDK, recorded on video with every tap shown as a dot. Land the commit under test on `dev` first. Then drive the runbook through `wf browser`, record it with `--video` and check it with `video-check`. Write the run to `docs/runbooks/.results/<gate>.json`, publish its evidence as one page, and tick the gate in `progress.md` with the link. A failing gate is a bug to fix, never a threshold to relax. `p7.gates-fresh` re-requires a gate whenever later code it exercises changes.
