#!/usr/bin/env node
// Builds what the on-device Python runner loads: the Pyodide runtime files copied from the pinned `pyodide` package into
// the data ring as Metro assets (the two text files get a `.bin` suffix, since Metro would otherwise bundle them as source),
// and pyodidePage.src.ts compiled by esbuild into the string pyodidePage.ts exports, which the hidden WebView runs.
// Generated, so it is rebuilt here, never edited by hand.
// Usage: node scripts/pyodide-bundle.mjs [--check] (the package scripts build:pyodide and check:pyodide); prints one JSON line
// {fresh, written, files} and, with --check, exits 1 when anything committed is stale.
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const DIR = 'features/leetcode/data/pyodide';
const SOURCE = `${DIR}/pyodidePage.src.ts`;
const OUTPUT = `${DIR}/pyodidePage.ts`;
const ASSETS = [
  ['pyodide.asm.wasm', 'pyodide.asm.wasm'],
  ['python_stdlib.zip', 'python_stdlib.zip'],
  ['pyodide.asm.mjs', 'pyodide.asm.mjs.bin'],
  ['pyodide-lock.json', 'pyodide-lock.json.bin'],
];

const check = process.argv.slice(2).includes('--check');
const require = createRequire(join(appDir, 'package.json'));
const packageDir = dirname(require.resolve('pyodide/package.json'));
const stale = [];

for (const [from, to] of ASSETS) {
  const source = readFileSync(join(packageDir, from));
  const target = join(appDir, DIR, 'assets', to);
  const fresh = existsSync(target) && readFileSync(target).equals(source);
  if (!fresh) {
    stale.push(`${DIR}/assets/${to}`);
    if (!check) {
      mkdirSync(dirname(target), { recursive: true });
      copyFileSync(join(packageDir, from), target);
    }
  }
}

const esbuild = require('esbuild');
const built = await esbuild.build({
  entryPoints: [SOURCE],
  absWorkingDir: appDir,
  bundle: true,
  minify: true,
  format: 'iife',
  write: false,
  platform: 'browser',
  target: ['es2020'],
  // the judge harness is Python text the page runs
  loader: { '.py': 'text' },
  // pyodide.mjs imports Node's modules and ws behind an IN_NODE test that is false in a WebView
  external: ['node:*', 'ws'],
  define: { 'process.env.NODE_ENV': '"production"' },
});
const js = built.outputFiles?.[0]?.text;
if (js === undefined) throw new Error(`esbuild wrote nothing for ${SOURCE}`);

const output = join(appDir, OUTPUT);
const next = `export const PYODIDE_PAGE_JS = ${JSON.stringify(js)};\n`;
if (!(existsSync(output) && readFileSync(output, 'utf8') === next)) {
  stale.push(OUTPUT);
  if (!check) writeFileSync(output, next);
}
console.log(JSON.stringify({ fresh: stale.length === 0, written: check ? [] : stale, files: ASSETS.length + 1 }));
if (check && stale.length > 0) {
  console.error(`${stale.join(', ')} stale: run pnpm --filter mla-tamagui run build:pyodide`);
  process.exit(1);
}
