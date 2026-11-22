// Compiles what the on-device JavaScript runner ships. buildPrelude turns the worker harness
// (features/leetcode/data/javascript/harness/worker.src.ts) into the one line that goes in front of the learner's code, and buildPage turns
// the hidden page (jsPage.src.ts) into the script the WebView runs, with the prelude defined in it as __JS_RUNNER_PRELUDE__. Shared by
// the page build (js-runner-bundle.mjs) and the tests, so both run the same text.
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const WORKER_SOURCE = 'features/leetcode/data/javascript/harness/worker.src.ts';
export const PAGE_SOURCE = 'features/leetcode/data/javascript/jsPage.src.ts';

const esbuild = () => createRequire(join(appDir, 'package.json'))('esbuild');

async function compile(entry, options) {
  const built = await esbuild().build({
    entryPoints: [entry],
    absWorkingDir: appDir,
    bundle: true,
    minify: true,
    format: 'iife',
    write: false,
    platform: 'browser',
    target: ['es2020'],
    legalComments: 'none',
    ...options,
  });
  const text = built.outputFiles?.[0]?.text;
  if (text === undefined) throw new Error(`esbuild wrote nothing for ${entry}`);
  return text.trim();
}

export async function buildPrelude() {
  const text = await compile(WORKER_SOURCE, {
    charset: 'ascii',
    // the minifier writes a string holding a newline as a template literal with a raw one, which would shift the learner's lines
    supported: { 'template-literal': false },
    define: { 'process.env.NODE_ENV': '"production"' },
  });
  const prelude = text.replace(/;$/, '');
  if (/[\n\r\u2028\u2029]/.test(prelude)) throw new Error('the worker prelude must be one line: a newline would shift every line of the learner\'s code');
  return prelude;
}

/** `entry` is the page's source, for a test that builds another one. */
export async function buildPage({ prelude, entry = PAGE_SOURCE }) {
  const js = await compile(entry, { define: { 'process.env.NODE_ENV': '"production"', __JS_RUNNER_PRELUDE__: JSON.stringify(prelude) } });
  // esbuild escapes it in every string, regex and template; this keeps a build option from undoing that
  if (/<\/script/i.test(js)) throw new Error('the page holds a closing script tag: the html that wraps it would end the script there');
  return js;
}
