/**
 * Runs every browser check in sequence and reports one summary.
 *
 * Sequential, not parallel: each check spawns its own static server and its own
 * in-process browser daemon, and the daemon's shutdown path calls
 * `process.exit(0)` -- so they have to be separate processes, and running them
 * concurrently would have several daemons contending for the same registry.
 *
 * Exports once up front rather than per check. Every check reads the same
 * `dist/`, and re-exporting between them would add roughly a minute for no
 * added coverage. An existing `dist/` is reused as-is.
 *
 * `--out <dir>` additionally captures one PNG per check: after each check, a
 * child process re-serves the same export through the same wf browser daemon
 * container and screenshots the route the check exercises at
 * `<dir>/<check>.png`. The child is a separate process because
 * `withServedApp` ends in `process.exit()` (see the daemon note above); the
 * captures are auxiliary, so a failed capture logs a warning and never
 * changes the pass/fail exit code.
 */
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { withServedApp } from './lib/servedApp.mjs';

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const REPO_ROOT = resolve(APP_DIR, '../..');

// Hidden child mode: `--capture <outDir> <slug> [path] [devicePreset] [colorScheme]`.
// The parent spawns one of these per check; withServedApp exits the process.
const CAPTURE_ARGS = process.argv[2] === '--capture' ? process.argv.slice(3) : null;

const outIdx = process.argv.indexOf('--out');
const OUT_DIR = !CAPTURE_ARGS && outIdx >= 0 ? resolve(process.argv[outIdx + 1]) : null;

const CHECKS = [
  // First, because it is the cheapest and the most fundamental: if app #1
  // drifted, every check below is measuring against a moved reference.
  ['the regression spine is untouched', 'verify-regression-spine.mjs', []],
  ['design system renders themed', 'verify-renders.mjs', [], { path: 'showcase' }],
  [
    'appearance settings: screen, mode control and a preset tile render themed',
    'verify-settings.mjs',
    [],
    { path: 'settings/appearance' },
  ],
  ['debug FAB opens the live log sheet', 'verify-debug-sheet.mjs', []],
  ['leetcode: list, editor, typing, description', 'verify-leetcode.mjs', []],
  [
    'tamil: course list, practice screen, grading advances',
    'verify-tamil.mjs',
    [],
    { path: 'tamil' },
  ],
  ['header menu: format, version history, login', 'verify-header-menu.mjs', []],
  ['overlays: backdrop, dismissal, scroll, reopen', 'verify-sheet.mjs', []],
  [
    'overlay deep link: cold start on an overlay route dismisses to its parent',
    'verify-overlay-deeplink.mjs',
    [],
  ],
  ['dark theme: surfaces and text stay visible', 'verify-dark-theme.mjs', [], { colorScheme: 'dark' }],
  [
    'the shell color-scheme picker actually repaints the app',
    'verify-color-scheme-picker.mjs',
    [],
  ],
  [
    'run outcome and toasts clear what they describe',
    'verify-result-and-toast.mjs',
    [],
  ],
  [
    'keyboard overlays on iOS',
    'verify-keyboard-platforms.mjs',
    ['iphone-15-pro'],
  ],
  ['keyboard resizes on Android', 'verify-keyboard-platforms.mjs', ['pixel-8'], { devicePreset: 'pixel-8' }],
  [
    'coding-agent: thread list, new thread, server-side reload',
    'verify-coding-agent-thread.mjs',
    [],
  ],
  [
    'coding-agent: streamed text grows and Stop dispatches interrupt',
    'verify-coding-agent-stream.mjs',
    [],
  ],
  [
    'coding-agent: tool group toggle collapses and expands 8 rows',
    'verify-coding-agent-tools.mjs',
    [],
  ],
  [
    'coding-agent: ApprovalCard allow + deny wire the right decisions',
    'verify-coding-agent-approval.mjs',
    [],
  ],
  [
    'coding-agent: UserInputCard option + submit dispatches user-input.respond',
    'verify-coding-agent-question.mjs',
    [],
  ],
  [
    'coding-agent: Plan segment placeholder, fixture:plan, and PlanCardImplement',
    'verify-coding-agent-plan.mjs',
    [],
  ],
  [
    'coding-agent: DiffView add/del rows paint distinct, non-transparent backgrounds',
    'verify-coding-agent-diff.mjs',
    [],
  ],
  [
    'coding-agent: ThreadSettingsButton opens the model picker with probe-sourced CLI groups',
    'verify-coding-agent-model.mjs',
    [],
  ],
  [
    'coding-agent: AssistantMessage, ToolRow, ApprovalCard, and DiffView rows stay readable on dark',
    'verify-coding-agent-dark.mjs',
    [],
  ],
  [
    'coding-agent: dispatch runs section lists the fake opencode-log runs',
    'verify-coding-agent-runs-list.mjs',
    [],
  ],
  [
    'coding-agent: dispatch run transcript is read-only with a dashboard link',
    'verify-coding-agent-runs-transcript.mjs',
    [],
  ],
  [
    'coding-agent: running dispatch run tails live and flips to finished',
    'verify-coding-agent-runs-live.mjs',
    [],
  ],
];

/**
 * `verify-result-and-toast.mjs` drives Run Local and Format, which are real
 * calls to the leetcode backend. Without it the toast reads "Format failed --
 * Failed to fetch" and the check fails for a missing service rather than a
 * defect; worse, its geometry assertions would pass just as happily on that
 * error card, which is the exact vacuous-green this check was written to stop.
 * So the suite starts the backend itself, with the seeds the app's development
 * config already carries, and stops it at the end.
 */
const BACKEND_PORT = 8789;
const BACKEND_ENV = {
  VM_GATEWAY_AUTH_SEED: 'local-e2e-token',
  LEETCODE_BACKEND_ADMIN_SECRET: 'local-e2e-admin',
  LEETCODE_CAPTURE_TOOL_BASE_URL: 'http://127.0.0.1:8792',
  LEETCODE_CAPTURE_TRIGGER_SECRET: 'local-e2e-capture',
};

async function backendAnswering() {
  try {
    const res = await fetch(`http://127.0.0.1:${BACKEND_PORT}/health`);
    return res.ok;
  } catch {
    return false;
  }
}

async function startBackendIfNeeded() {
  if (await backendAnswering()) {
    console.log('› leetcode backend already running');
    return null;
  }
  console.log('› starting the leetcode backend');
  const child = spawn(
    'pnpm',
    ['--filter', '@app/leetcode-backend', 'run', 'dev'],
    {
      cwd: REPO_ROOT,
      env: { ...process.env, ...BACKEND_ENV },
      stdio: 'ignore',
      detached: true,
    }
  );
  for (let i = 0; i < 40; i++) {
    await new Promise((resolve) => setTimeout(resolve, 500));
    if (await backendAnswering()) return child;
  }
  console.error(
    `FAIL: the leetcode backend did not answer on :${BACKEND_PORT} within 20s`
  );
  try {
    process.kill(-child.pid);
  } catch {
    // already gone
  }
  process.exit(1);
}

function run(command, args) {
  return (
    spawnSync(command, args, { cwd: APP_DIR, stdio: 'inherit' }).status === 0
  );
}

if (CAPTURE_ARGS) {
  const [outDir, slug, path = '', devicePreset = 'iphone-15-pro', colorScheme = 'system'] =
    CAPTURE_ARGS;
  mkdirSync(outDir, { recursive: true });
  // Port 8399: every verify-* script owns 8199-8225, and captures run
  // sequentially, one child process per check.
  const opts = { port: 8399, name: `capture-${slug}`, devicePreset, colorScheme };
  if (path) opts.path = path;
  await withServedApp(
    opts,
    async ({ container, name }) => {
      await new Promise((r) => setTimeout(r, 1200));
      const result = await container.screenshot({ name });
      if (!result?.path) throw new Error(`screenshot failed for ${slug}`);
      copyFileSync(result.path, join(outDir, `${slug}.png`));
      console.log(`ok: ${join(outDir, `${slug}.png`)}`);
    },
  );
  // withServedApp ends in process.exit().
}

// The route/scheme a check's PNG should show. Coding-agent checks all boot the
// same list route; the dark variant is the only one that emulates dark.
const captureFor = (script, spec = {}) => ({
  path: spec.path ?? (script.startsWith('verify-coding-agent') ? 'coding-agent' : ''),
  devicePreset: spec.devicePreset ?? 'iphone-15-pro',
  colorScheme:
    spec.colorScheme ?? (script === 'verify-coding-agent-dark.mjs' ? 'dark' : 'system'),
});

const checkSlug = (script, args) =>
  script.replace(/^verify-/, '').replace(/\.mjs$/, '') +
  (args.length ? `-${args.join('-')}` : '');

const backend = await startBackendIfNeeded();

if (existsSync(join(APP_DIR, 'dist'))) {
  console.log('› dist/ already present, reusing the existing web export');
} else {
  console.log('› exporting web bundle');
  if (!run('pnpm', ['run', 'export:web'])) {
    console.error('FAIL: the web export did not build');
    process.exit(1);
  }
}

const failed = [];
for (const [label, script, args, spec] of CHECKS) {
  console.log(`\n› ${label}`);
  if (!run(process.execPath, [`e2e/${script}`, ...args])) failed.push(label);
  if (OUT_DIR) {
    const { path, devicePreset, colorScheme } = captureFor(script, spec);
    const ok = run(process.execPath, [
      fileURLToPath(import.meta.url),
      '--capture',
      OUT_DIR,
      checkSlug(script, args),
      path,
      devicePreset,
      colorScheme,
    ]);
    if (!ok) console.error(`warn: screenshot failed for ${checkSlug(script, args)}`);
  }
}

if (backend) {
  try {
    process.kill(-backend.pid);
  } catch {
    // already exited
  }
}

console.log('');
if (failed.length) {
  console.error(`FAIL: ${failed.length}/${CHECKS.length} checks failed:`);
  for (const label of failed) console.error(`  - ${label}`);
  process.exit(1);
}
console.log(`ok: all ${CHECKS.length} checks passed`);
