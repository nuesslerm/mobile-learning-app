// features/coding-agent/src/testing/node/export-fixtures.mjs
//
// Deterministic fixture export for the fake servers in this package (WS
// agent + opencode-log). Runs via `tsx` (this package's devDependency)
// so it can import the TypeScript barrel
// `../../fixtures/index.ts` and write the serializable
// `fixtures.json` (beside this file) (FIXTURE_THREADS + FIXTURE_SERVER_CONFIG +
// dispatchRuns) that the plain-JS servers load. Regenerated on demand when
// `fakeWsAgentServer.mjs` finds the file missing or when
// `fakeOpencodeLogServer.mjs` finds the `dispatchRuns` key missing.

import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  FIXTURE_THREADS,
  FIXTURE_SERVER_CONFIG,
  FIXTURE_DISPATCH_RUNS,
} from '../../fixtures/index.ts';

const here = dirname(fileURLToPath(import.meta.url));
const outFile = join(here, 'fixtures.json');

export function exportFixtures() {
  const payload = {
    FIXTURE_THREADS,
    FIXTURE_SERVER_CONFIG,
    dispatchRuns: FIXTURE_DISPATCH_RUNS,
  };
  mkdirSync(dirname(outFile), { recursive: true });
  writeFileSync(outFile, `${JSON.stringify(payload, null, 2)}\n`, 'utf8');
  return outFile;
}

if (
  process.argv[1] !== undefined &&
  fileURLToPath(import.meta.url) === process.argv[1]
) {
  const path = exportFixtures();
  process.stdout.write(`exported fixtures -> ${path}\n`);
}
