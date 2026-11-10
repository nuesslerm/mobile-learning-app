#!/usr/bin/env node
// Builds the code editor's CodeMirror bundle: cm6Bundle.src.ts compiled by esbuild into the string cm6Bundle.ts exports,
// which the WebView (a srcdoc iframe on web) runs. Generated, so it is rebuilt here, never edited by hand.
// Usage: node scripts/cm6-bundle.mjs [--check] (the package scripts build:editor-bundle and check:editor-bundle); prints one JSON line
// {path, bytes, fresh, written} and, with --check, exits 1 when the committed file is stale.
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const SOURCE = 'features/leetcode/code-editor/core/cm6Bundle.src.ts';
const OUTPUT = 'features/leetcode/code-editor/core/cm6Bundle.ts';

// Runs before the bundle: the WebView has no Node or Metro globals for the logger to read.
const SHIM = [
  '// Browser shim for Node/Metro globals that the bundled CodeMirror build',
  "// transitively references (via `@/lib/logging/logger`'s LogLayer transports,",
  '// which read `process.env.X` and `__DEV__` at module init).',
  '//',
  '// The CM6 bundle is built with esbuild for the browser (it runs inside a',
  '// `react-native-webview` iframe, which has no Node, no Metro). Without this',
  '// shim the bundle crashes with "process is not defined" / "__DEV__ is not',
  '// defined" on the very first line that touches them, before CodeMirror can',
  '// mount — the iframe then renders blank.',
  '//',
  "// `process.env` reads collapse to `undefined`, and the logger's",
  '// `s0(undefined, false)` falls back to the second arg (the `__DEV__` literal',
  '// defined below), so the console transport stays off and `logStore` stays',
  '// on — same as production mobile behavior.',
  '// eslint-disable-next-line no-unused-vars -- local escape; export kept for public API',
  'let process = { env: {} };',
  '// eslint-disable-next-line no-unused-vars -- local escape; export kept for public API',
  'let __DEV__ = false;',
].join('\n');

const check = process.argv.slice(2).includes('--check');
const esbuild = createRequire(join(appDir, 'package.json'))('esbuild');
const built = await esbuild.build({
  entryPoints: [SOURCE],
  absWorkingDir: appDir,
  bundle: true,
  minify: true,
  format: 'iife',
  globalName: 'CM',
  write: false,
  platform: 'browser',
  target: ['es2020'],
  define: { 'process.env.NODE_ENV': '"production"' },
});
const js = built.outputFiles?.[0]?.text;
if (js === undefined) throw new Error(`esbuild wrote nothing for ${SOURCE}`);

const output = join(appDir, OUTPUT);
const next = `export const CM6_BUNDLE_JS = ${JSON.stringify(`${SHIM}\n${js}`)};\n`;
const fresh = existsSync(output) && readFileSync(output, 'utf8') === next;
if (!check && !fresh) writeFileSync(output, next);
console.log(
  JSON.stringify({ path: OUTPUT, bytes: next.length, fresh, written: !check && !fresh })
);
if (check && !fresh) {
  console.error(`${OUTPUT} is stale: run pnpm --filter mla-tamagui run build:editor-bundle`);
  process.exit(1);
}
