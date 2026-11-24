#!/usr/bin/env node
// Builds what the on-device JavaScript runner loads: jsPage.src.ts compiled by esbuild (with the worker prelude defined in it, see
// js-runner-build.mjs) into the string jsPage.ts exports, which the hidden WebView runs. Generated, so it is rebuilt here, never edited by hand.
// Usage: node scripts/js-runner-bundle.mjs [--check] (the package scripts build:js-runner and check:js-runner); prints one JSON line
// {fresh, written, bytes} and, with --check, exits 1 when the committed page is stale.
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { buildPage, buildPrelude } from './js-runner-build.mjs';

const appDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const OUTPUT = 'features/leetcode/data/javascript/jsPage.ts';

const check = process.argv.slice(2).includes('--check');
const js = await buildPage({ prelude: await buildPrelude() });

const output = join(appDir, OUTPUT);
const next = `export const JS_PAGE_JS = ${JSON.stringify(js)};\n`;
const fresh = existsSync(output) && readFileSync(output, 'utf8') === next;
if (!check && !fresh) writeFileSync(output, next);
console.log(JSON.stringify({ fresh, written: check ? [] : fresh ? [] : [OUTPUT], bytes: next.length }));
if (check && !fresh) {
  console.error(`${OUTPUT} is stale: run pnpm --filter mla-tamagui run build:js-runner`);
  process.exit(1);
}
