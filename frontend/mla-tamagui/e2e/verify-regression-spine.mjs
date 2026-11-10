/**
 * The regression spine (issue #217): the surviving app only means something as
 * a side-by-side against what it was ported from. If
 * `frontend/libraries/tamagui-design-system` or the surviving app's routes drifted
 * while this app was being built, there is nothing left to compare against,
 * and the comparison — the whole point of the exercise — is silently void.
 *
 * So this asserts the diff against the merge base with the default branch, not
 * against the working tree: a change to those paths ON THIS BRANCH is what
 * breaks the comparison, while upstream moving ahead is not this branch's
 * problem.
 */
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');

const SPINE_PATHS = [
  'frontend/libraries/tamagui-design-system',
  'frontend/mla-tamagui/app',
];

function git(args) {
  const result = spawnSync('git', args, {
    cwd: REPO_ROOT,
    encoding: 'utf8',
  });
  return { status: result.status, stdout: (result.stdout ?? '').trim() };
}

const baseRef = process.env.SPINE_BASE_REF ?? 'origin/main';

const mergeBase = git(['merge-base', 'HEAD', baseRef]);
if (mergeBase.status !== 0 || !mergeBase.stdout) {
  console.log(
    `skip: no merge base with ${baseRef} (shallow clone or missing remote ref) — spine not checked`
  );
  process.exit(0);
}

const changed = git(['diff', '--name-only', mergeBase.stdout, '--', ...SPINE_PATHS]);
if (changed.status !== 0) {
  console.error('FAIL: could not diff the spine paths');
  process.exit(1);
}

const waiverPath = resolve(
  dirname(fileURLToPath(import.meta.url)),
  'regression-spine-waivers.json'
);
/** Reviewed, both-apps-identical changes -- see the file's own note. */
const waived = JSON.parse(readFileSync(waiverPath, 'utf8')).waived;

if (changed.stdout) {
  const files = changed.stdout
    .split('\n')
    .filter((file) => waived[file] === undefined);
  const waivedCount = changed.stdout.split('\n').length - files.length;
  if (files.length === 0) {
    console.log(
      `ok: the regression spine changed in ${waivedCount} waived file(s) only (see e2e/regression-spine-waivers.json for the reason on each)`
    );
    process.exit(0);
  }
  console.error(
    `FAIL: ${files.length} file(s) under the regression spine changed since ${baseRef} (${mergeBase.stdout.slice(0, 8)}), so app #1 is no longer the thing app #2 is being compared against:`
  );
  for (const file of files) console.error(`  - ${file}`);
  process.exit(1);
}

console.log(
  `ok: the regression spine (${SPINE_PATHS.join(', ')}) is untouched since ${baseRef}`
);
