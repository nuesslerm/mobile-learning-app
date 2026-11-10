import fs from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

const repo = path.resolve(__dirname, '../../../../..');
const listed = JSON.parse(fs.readFileSync(path.join(repo, 'docs/plans/device-lab/native-ui-residuals.json'), 'utf8')).files as Record<
  string,
  { libraries: string[]; differs: string; residual: string }
>;

const ROOTS = ['frontend/mla-tamagui', 'libraries', 'features'];
const SKIP = /(^|\/)(node_modules|dist|\.expo|\.serve-tmp[^/]*|__tests__|tests|e2e|eslint-plugin-mobile-learn)(\/|$)|\.test\.tsx?$/;
const NATIVE_UI = /from ['"](react-native-gesture-handler|react-native-reanimated|@shopify\/react-native-skia)['"]/;

function walk(dir: string, found: string[] = []): string[] {
  for (const entry of fs.readdirSync(path.join(repo, dir), { withFileTypes: true })) {
    const rel = `${dir}/${entry.name}`;
    if (SKIP.test(rel)) continue;
    if (entry.isDirectory()) walk(rel, found);
    else if (/\.(ts|tsx)$/.test(entry.name)) found.push(rel);
  }
  return found;
}

const importers = ROOTS.flatMap((root) => walk(root)).filter((file) => NATIVE_UI.test(fs.readFileSync(path.join(repo, file), 'utf8')));

describe('native UI libraries', () => {
  it('Given every file importing gesture handler, reanimated or Skia, When listed, Then each has what differs and what stays residual', () => {
    // Given the importers found in the tree
    // When compared with the residual list
    const missing = importers.filter((file) => !(file in listed));
    const stale = Object.keys(listed).filter((file) => !importers.includes(file));
    const bare = Object.entries(listed).filter(([, entry]) => !entry.differs.trim() || !entry.residual.trim()).map(([file]) => file);

    // Then none is missing, stale or blank: a new screen on these libraries says what a browser cannot show of it
    expect({ missing, stale, bare }).toEqual({ missing: [], stale: [], bare: [] });
  });
});
