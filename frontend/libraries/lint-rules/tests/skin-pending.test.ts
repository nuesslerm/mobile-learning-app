import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const here = path.dirname(fileURLToPath(import.meta.url));
const frontend = path.resolve(here, '../../..');
const pluginEntry = path.resolve(here, '../dist/esm/index.js');
const oxlint = path.join(path.dirname(createRequire(import.meta.url).resolve('oxlint/package.json')), 'bin', 'oxlint');

type Override = { files: string[]; rules: Record<string, string> };

/** The files the repo's lint config still excuses from one skin rule: the override that turns only that rule off. */
function pending(rule: string): string[] {
  const text = readFileSync(path.join(frontend, '.oxlintrc.json'), 'utf8');
  const config = ts.parseConfigFileTextToJson('.oxlintrc.json', text).config as { overrides: Override[] };
  const lists = config.overrides.filter(
    (override) => Object.keys(override.rules).length === 1 && override.rules[`mobile-learn/${rule}`] === 'off' && override.files.every((file) => !file.includes('*')),
  );
  return lists.flatMap((override) => override.files);
}

/** The listed files the rule reports on, when it runs on exactly those files. */
function reported(rule: string, files: string[]): Set<string> {
  const config = path.join(mkdtempSync(path.join(os.tmpdir(), 'skin-pending-')), 'oxlintrc.json');
  writeFileSync(config, JSON.stringify({ jsPlugins: [pluginEntry], categories: { correctness: 'off' }, rules: { [`mobile-learn/${rule}`]: 'error' } }));
  let out: string;
  try {
    out = execFileSync(oxlint, ['-c', config, '--format', 'unix', ...files], { cwd: frontend, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  } catch (err) {
    const stdout = (err as { stdout?: string }).stdout;
    if (typeof stdout !== 'string') throw err;
    out = stdout;
  }
  const marker = `mobile-learn(${rule})`;
  return new Set(out.split('\n').filter((line) => line.includes(marker)).map((line) => line.slice(0, line.indexOf(':'))));
}

describe.each(['no-design-system-outside-a-skin', 'no-react-native-controls'])('the files still excused from %s', (rule) => {
  it('Given the list in the lint config, When the rule runs on those files, Then every one still exists and still breaks it', () => {
    // Given the pending list
    const files = pending(rule);

    // When the rule runs on exactly those files
    const missing = files.filter((file) => !existsSync(path.join(frontend, file)));
    const stillBreaking = reported(rule, files.filter((file) => !missing.includes(file)));

    // Then no line is stale: a file that moved behind the skin, or is gone, leaves the list
    expect(missing, 'listed but deleted').toEqual([]);
    expect(files.filter((file) => !stillBreaking.has(file)), 'listed but no longer breaking the rule').toEqual([]);
  });
});
