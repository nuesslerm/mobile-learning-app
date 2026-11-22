// Compiles the worker harness (features/leetcode/data/javascript/harness/worker.src.ts) into the one-line prelude that goes in front of the
// learner's code. Shared by the page build (js-runner-bundle.mjs) and the tests, so both run the same text.
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const SOURCE = 'features/leetcode/data/javascript/harness/worker.src.ts';

export async function buildPrelude() {
  const esbuild = createRequire(join(appDir, 'package.json'))('esbuild');
  const built = await esbuild.build({
    entryPoints: [SOURCE],
    absWorkingDir: appDir,
    bundle: true,
    minify: true,
    format: 'iife',
    write: false,
    platform: 'browser',
    target: ['es2020'],
    legalComments: 'none',
    charset: 'ascii',
    // the minifier writes a string holding a newline as a template literal with a raw one, which would shift the learner's lines
    supported: { 'template-literal': false },
    define: { 'process.env.NODE_ENV': '"production"' },
  });
  const text = built.outputFiles?.[0]?.text;
  if (text === undefined) throw new Error(`esbuild wrote nothing for ${SOURCE}`);
  const prelude = text.trim().replace(/;$/, '');
  if (/[\n\r\u2028\u2029]/.test(prelude)) throw new Error('the worker prelude must be one line: a newline would shift every line of the learner\'s code');
  return prelude;
}
