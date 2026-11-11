/**
 * Captures the nine leetcode visual-judge contract shots
 * (docs/archive/README.md) against the served web export,
 * into .report/leetcode/<shot>.png.
 *
 * Each shot waits for that state's testID before capturing so no shot catches a
 * blank frame. The voice states are driven through the real controls (mic -> stop -> review -> send
 * -> alternative picker -> Run Local), made deterministic by the fixture ports;
 * see features/leetcode/data/fixturePorts.ts.
 *
 * The fixture switch is baked in at EXPORT time, not run time:
 * LeetCodeFixtureOverrides reads process.env.EXPO_PUBLIC_LEETCODE_FIXTURES at
 * module scope, so the flag must be present when expo bundles. The export goes
 * to the same dist/ withServedApp serves (lib/servedApp.mjs), with the flag
 * passed the same way as the other EXPO_PUBLIC_* vars the export:web script
 * already carries.
 *
 * withServedApp ends in process.exit(), so the light and dark schemes cannot
 * share one process: the orchestrating invocation exports once, runs one child
 * per scheme, then exits non-zero if any of the nine PNGs was not written.
 *
 * Usage: node e2e/capture-leetcode-shots.mjs             (both schemes)
 *        node e2e/capture-leetcode-shots.mjs light|dark  (one scheme)
 */
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { APP_DIR, fail, withServedApp } from './lib/servedApp.mjs';

const OUT_DIR = join(APP_DIR, '.report', 'leetcode');
const SCHEME = process.argv[2];
const THIS_FILE = fileURLToPath(import.meta.url);

const SHOT_NAMES = [
  'editor-light',
  'editor-dark',
  'recording',
  'review',
  'alternatives',
  'diff',
  'run-result',
  'login-status',
  'description',
];

const settle = (ms = 900) => new Promise((r) => setTimeout(r, ms));

async function waitFor(ctx, expression, attempts = 40) {
  for (let i = 0; i < attempts; i++) {
    if (await ctx.evalInApp(expression).catch(() => null)) return true;
    await settle(400);
  }
  return false;
}

const DONE_KEY = `(() => {
  const done = appDoc.querySelector('[data-testid="EditorAction-Done"]');
  const rail = appDoc.querySelector('[data-testid="EditorActionToolbar"]');
  if (!done || !rail) return null;
  const d = done.getBoundingClientRect();
  const width = appDoc.defaultView.innerWidth;
  const rowY = rail.getBoundingClientRect().top + rail.getBoundingClientRect().height / 2;
  return { x: d.left + d.width / 2, y: d.top + d.height / 2, onScreen: d.left >= 0 && d.right <= width, rowY, width };
})()`;

// The keyboard goes down only as a user puts it away: a touch on Done, the action row swiped first when Done is past
// the screen's edge.
async function tapDone(ctx) {
  if (!(await ctx.evalInApp(`appDoc.defaultView.__MCP_KEYBOARD_VISIBLE__ === true`))) return;
  let done = await ctx.evalInApp(DONE_KEY);
  if (done && !done.onScreen) {
    await ctx.container.swipe({ name: ctx.name, from: { x: done.width - 30, y: done.rowY }, dx: 60 - done.width, dy: 0, durationMs: 400, screen: true });
    await settle(300);
    done = await ctx.evalInApp(DONE_KEY);
  }
  if (!done?.onScreen) {
    fail('no Done key on screen to put the keyboard away');
    return;
  }
  await ctx.container.tap({ name: ctx.name, x: done.x, y: done.y });
  await settle(600);
}

async function capture(ctx, shot) {
  const result = await ctx.container.screenshot({ name: ctx.name });
  if (!result?.path) {
    fail(`screenshot failed for ${shot}`);
    return;
  }
  writeFileSync(join(OUT_DIR, `${shot}.png`), readFileSync(result.path));
  console.log(`  · ${shot} → ${join(OUT_DIR, `${shot}.png`)}`);
}

async function walkDark(ctx) {
  if (!(await waitFor(ctx, `!!appDoc.querySelector('[data-testid="ProblemEditorScreen"]')`))) {
    fail('editor screen never became ready (dark)');
    return;
  }
  await settle();
  await capture(ctx, 'editor-dark');
}

async function walkLight(ctx) {
  if (!(await waitFor(ctx, `!!appDoc.querySelector('[data-testid="ProblemEditorScreen"]')`))) {
    fail('editor screen never became ready (light)');
    return;
  }
  await settle();

  // The editor auto-focuses on open, so the mock keyboard and helper toolbar
  // cover the Format / Run Local / Run / Reset / mic row.
  await tapDone(ctx);
  const rowClear = `(() => {
    if (appDoc.defaultView.__MCP_KEYBOARD_VISIBLE__ === true) return false;
    const btn = appDoc.querySelector('[data-testid="ProblemEditorRunLocalButton"]');
    if (!btn) return false;
    const r = btn.getBoundingClientRect();
    if (!r.width || !r.height) return false;
    if (r.top < 0 || r.bottom > appDoc.defaultView.innerHeight) return false;
    const hit = appDoc.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return !!hit && (btn === hit || btn.contains(hit));
  })()`;
  if (!(await waitFor(ctx, rowClear))) {
    fail('editor never blurred: the action row stayed covered by the keyboard/toolbar (light)');
    return;
  }
  await settle();
  await capture(ctx, 'editor-light');

  // The fixtures' plan alternatives rewrite the JS/TS Two Sum starter, so the
  // diff shot stays coherent only in a JS buffer; the default is python3.
  // Since the editor initialises on web the mock keyboard's syntax rail sits
  // over the picker row while the editor holds focus.
  await tapDone(ctx);
  await ctx.tapInApp('[data-testid="ProblemEditorLanguagePicker-Trigger"]');
  if (!(await waitFor(ctx, `!!appDoc.querySelector('[data-testid="ProblemEditorLanguagePicker-JavaScript"]')`))) {
    fail('language picker never opened (light)');
    return;
  }
  await settle(400);
  await ctx.tapInApp('[data-testid="ProblemEditorLanguagePicker-JavaScript"]');
  const jsReady = `(() => {
    const trigger = appDoc.querySelector('[data-testid="ProblemEditorLanguagePicker-Trigger"]');
    return !!trigger && trigger.textContent.trim() === 'JavaScript';
  })()`;
  if (!(await waitFor(ctx, jsReady))) {
    fail('language switch to JavaScript never landed (light)');
    return;
  }
  await settle();

  // recording: mic tapped, fixture levels flowing into the waveform.
  await ctx.tapInApp('[data-testid="ProblemEditorVoiceStartButton"]');
  const levels = `(() => {
    const bar = appDoc.querySelector('[data-testid="ProblemEditorVoiceToolbar-Waveform-Bar-3"]');
    return !!bar && bar.getBoundingClientRect().height > 10;
  })()`;
  if (!(await waitFor(ctx, levels))) {
    fail('recording state never showed flowing levels (light)');
    return;
  }
  await settle();
  await capture(ctx, 'recording');

  // review: stopped, review sheet open with the fixture transcript.
  await ctx.tapInApp('[data-testid="ProblemEditorVoiceToolbar-Send"]');
  const review = `(() => {
    const input = appDoc.querySelector('[data-testid="ProblemEditorVoiceReviewSheet-TranscriptInput"]');
    return !!input && input.value === 'rename nums to values and return early when target is found';
  })()`;
  if (!(await waitFor(ctx, review))) {
    fail('review state never showed the fixture transcript (light)');
    return;
  }
  await settle();
  await capture(ctx, 'review');

  // The fixture transcript is itself a rename ("rename nums to values ..."),
  // which parseLocalEdit resolves locally and auto-applies -- no plan, no
  // picker. Editing it to a plain "hash map" instruction sends it through the
  // plan, whose two fixture alternatives then appear in the picker. review.png
  // above keeps the fixture transcript; this edit happens after it is captured.
  const retype = `(() => {
    const input = appDoc.querySelector('[data-testid="ProblemEditorVoiceReviewSheet-TranscriptInput"]');
    if (!input) return false;
    const setter = Object.getOwnPropertyDescriptor(appDoc.defaultView.HTMLTextAreaElement.prototype, 'value').set;
    setter.call(input, 'solve two sum with a hash map pass and return early when target is found');
    input.dispatchEvent(new appDoc.defaultView.Event('input', { bubbles: true }));
    return input.value;
  })()`;
  if (!(await ctx.evalInApp(retype))) {
    fail('could not retype the review transcript (light)');
    return;
  }

  // alternatives: Send taps into planning, whose two fixture alternatives the
  // picker lists.
  await ctx.tapInApp('[data-testid="ProblemEditorVoiceReviewSheet-Send"]');
  const alternatives = `(() => {
    const picker = appDoc.querySelector('[data-testid="ProblemEditorAlternativePicker"]');
    const second = appDoc.querySelector('[data-testid="ProblemEditorAlternativePicker-Row-1"]');
    return !!picker && !!second;
  })()`;
  if (!(await waitFor(ctx, alternatives))) {
    fail('alternative picker never listed the two fixture alternatives (light)');
    return;
  }
  await settle();
  await capture(ctx, 'alternatives');

  // diff: pick the second alternative (rename nums to values); the applied edit
  // shows the diff decoration over the changed lines.
  await ctx.tapInApp('[data-testid="ProblemEditorAlternativePicker-Row-1"]');
  if (!(await waitFor(ctx, `!!appDoc.querySelector('[data-testid="ProblemEditorDiffToggle"]')`))) {
    fail('diff decoration never appeared after picking the second alternative (light)');
    return;
  }
  await settle();
  await capture(ctx, 'diff');

  // run-result: Run Local tapped, fixture returns a passing Accepted result.
  // The applied edit refocuses the editor, which brings the keyboard rail back
  // over the action row -- put it away again before tapping Run Local.
  await tapDone(ctx);
  await ctx.tapInApp('[data-testid="ProblemEditorRunLocalButton"]');
  const run = `(() => {
    const status = appDoc.querySelector('[data-testid="ProblemEditorRunOutcomeStatus"]');
    return !!status && status.textContent === 'Accepted';
  })()`;
  if (!(await waitFor(ctx, run))) {
    fail('run local never showed a passing result (light)');
    return;
  }
  await settle(1200);
  await capture(ctx, 'run-result');

  // login-status: /leetcode/login, after its status fetch settles.
  await ctx.goto('leetcode/login');
  const login = `(() => !!appDoc.querySelector('[data-testid="LoginScreen"]') && !appDoc.querySelector('[data-testid="LoginScreenSpinner"]'))()`;
  if (!(await waitFor(ctx, login))) {
    fail('login screen never became ready (light)');
    return;
  }
  await settle();
  await capture(ctx, 'login-status');

  // description: /leetcode/two-sum/description opens the problem statement as
  // an overlay sheet over the editor.
  await ctx.goto('leetcode/two-sum/description');
  const desc = `(() => {
    const sheet = appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]');
    const body = appDoc.querySelector('[data-testid="ProblemDescriptionBody"]');
    return !!sheet && !!body && body.textContent.trim().length > 0;
  })()`;
  if (!(await waitFor(ctx, desc))) {
    fail('description overlay never showed the problem statement (light)');
    return;
  }
  await settle();
  await capture(ctx, 'description');
}

function exportWebWithFixtures() {
  console.log('› exporting web build with EXPO_PUBLIC_LEETCODE_FIXTURES=1');
  const { status } = spawnSync('pnpm', ['run', 'export:web'], {
    cwd: APP_DIR,
    stdio: 'inherit',
    env: { ...process.env, EXPO_PUBLIC_LEETCODE_FIXTURES: '1' },
  });
  if (status !== 0) {
    console.error('FAIL: the fixture web export did not build');
    process.exit(1);
  }
}

function runScheme(scheme) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [THIS_FILE, scheme], { stdio: 'inherit' });
    child.on('exit', (code) => resolve(code ?? 0));
  });
}

mkdirSync(OUT_DIR, { recursive: true });

if (SCHEME === 'dark') {
  await withServedApp(
    {
      port: 8251,
      name: 'leetcode-shots-dark',
      path: 'leetcode/two-sum',
      devicePreset: 'iphone-15-pro',
      colorScheme: 'dark',
    },
    walkDark
  );
} else if (SCHEME === 'light') {
  await withServedApp(
    {
      port: 8250,
      name: 'leetcode-shots-light',
      path: 'leetcode/two-sum',
      devicePreset: 'iphone-15-pro',
      colorScheme: 'light',
    },
    walkLight
  );
} else {
  exportWebWithFixtures();
  const dark = await runScheme('dark');
  if (dark !== 0) process.exit(dark || 1);
  const light = await runScheme('light');
  if (light !== 0) process.exit(light || 1);
  const missing = SHOT_NAMES.filter((shot) => !existsSync(join(OUT_DIR, `${shot}.png`)));
  if (missing.length) {
    console.error(`FAIL: PNG(s) not written: ${missing.join(', ')}`);
    process.exit(1);
  }
  console.log(`ok: ${SHOT_NAMES.length} leetcode shots → ${OUT_DIR}`);
}