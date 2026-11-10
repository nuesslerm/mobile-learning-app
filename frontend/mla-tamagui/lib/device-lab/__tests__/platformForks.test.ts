import fs from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

type Entry = { class: 'inherent' | 'removable' | 'instrumentation'; why: string; nativeTest?: string; residual?: string };

const repo = path.resolve(__dirname, '../../../../..');
const classified = JSON.parse(fs.readFileSync(path.join(repo, 'docs/plans/device-lab/platform-forks.json'), 'utf8')).files as Record<string, Entry>;

const ROOTS = ['frontend/mla-tamagui', 'libraries', 'features'];
const SKIP = /(^|\/)(node_modules|dist|\.expo|\.serve-tmp[^/]*|__tests__|tests|e2e|eslint-plugin-mobile-learn)(\/|$)|\.test\.tsx?$/;
const CODE = /\.(ts|tsx)$/;
const FORK_FILE = /\.(web|native)\.tsx?$/;
const USES_PLATFORM = /\bPlatform\.(OS|select)\b/;

function walk(dir: string, found: string[] = []): string[] {
  for (const entry of fs.readdirSync(path.join(repo, dir), { withFileTypes: true })) {
    const rel = `${dir}/${entry.name}`;
    if (SKIP.test(rel)) continue;
    if (entry.isDirectory()) walk(rel, found);
    else if (CODE.test(entry.name)) found.push(rel);
  }
  return found;
}

const forks = ROOTS.flatMap((root) => walk(root)).filter(
  (file) => FORK_FILE.test(file) || USES_PLATFORM.test(fs.readFileSync(path.join(repo, file), 'utf8'))
);

describe('platform forks', () => {
  it('Given every Platform.OS use and fork file in shared code, When listed, Then each one is classified', () => {
    // Given the forks found in the tree
    // When compared with the classification
    const unclassified = forks.filter((file) => !(file in classified));

    // Then none is missing: a new fork says whether it is inherent, removable or instrumentation
    expect(unclassified).toEqual([]);
  });

  it('Given the classification, When read, Then it names no file that is gone or no longer forks', () => {
    // Given the classified files
    // When each is looked up among the forks found
    const stale = Object.keys(classified).filter((file) => !forks.includes(file));

    // Then every entry still describes a real fork
    expect(stale).toEqual([]);
  });

  it('Given an inherent fork, When classified, Then it has a unit test of its native branch or says why it has none', () => {
    // Given the inherent forks
    const inherent = Object.entries(classified).filter(([, entry]) => entry.class === 'inherent');

    // When each is checked for evidence
    const bare = inherent
      .filter(([, entry]) => !(entry.nativeTest && fs.existsSync(path.join(repo, entry.nativeTest))) && !entry.residual?.trim())
      .map(([file]) => file);

    // Then each has a test that exists, or a residual
    expect(bare).toEqual([]);
  });
});
