import { spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { cruise } from 'dependency-cruiser';
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

const require = createRequire(import.meta.url);
const presets = require('../depcruise.presets.cjs');
const planted = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'fixtures', 'planted');

async function violations(preset: string, variant: 'clean' | 'dirty', config: unknown, entries: string[]): Promise<string[]> {
  const cwd = path.join(planted, preset, variant);
  const before = process.cwd();
  process.chdir(cwd);
  try {
    const { forbidden, options } = config as { forbidden: unknown[]; options: Record<string, unknown> };
    const result = await cruise(entries, { ...options, tsConfig: 'tsconfig.json', ruleSet: { forbidden }, validate: true } as never);
    const output = result.output as { summary: { violations: { rule: { name: string } }[] } };
    return [...new Set(output.summary.violations.map((v) => v.rule.name))].sort();
  } finally {
    process.chdir(before);
  }
}

const CASES: { preset: string; config: unknown; rules: string[]; entries?: string[] }[] = [
  {
    preset: 'hexagonalFeature',
    config: presets.hexagonalFeature({ root: 'src' }),
    rules: [
      'data-imports-domain',
      'domain-imports-nothing',
      'domain-pulls-no-framework',
      'domain-pulls-no-node',
      'feature-does-not-import-the-shell',
      'module-in-a-ring',
      'no-circular',
      'only-di-reads-app-di',
      'production-does-not-import-testing',
      'routes-imports-di-ui',
      'state-imports-domain',
      'ui-imports-domain-state',
    ],
  },
  {
    preset: 'presentationFeature',
    config: presets.presentationFeature({ root: 'src' }),
    rules: ['presentation-has-no-ring-beyond-ui', 'routes-imports-ui', 'state-imports-nothing', 'ui-imports-state'],
  },
  {
    preset: 'contractsPackage',
    config: presets.contractsPackage({ root: 'src' }),
    rules: ['contracts-import-only-zod', 'contracts-no-node'],
  },
  {
    preset: 'pure',
    config: presets.pure(),
    rules: ['no-circular', 'pure-no-node', 'pure-no-workspace-package', 'pure-npm-allow-list'],
  },
  {
    preset: 'hexagonal',
    config: presets.hexagonal(),
    rules: ['adapters-import-core-and-ports', 'core-imports-core-and-ports', 'core-no-device-modules', 'core-no-node', 'library-does-not-import-the-app', 'ports-import-core'],
  },
  {
    preset: 'component',
    config: presets.component(),
    rules: ['lib-imports-nothing', 'theme-imports-lib', 'ui-imports-theme-primitives-icons-lib'],
  },
  {
    preset: 'harness',
    config: presets.harness(),
    rules: ['harness-imports-no-app-or-feature'],
  },
  {
    preset: 'appShell',
    config: presets.appShell(),
    entries: ['app', 'app-shell', 'lib', 'features', 'components'],
    rules: ['features-do-not-import-the-shell', 'lib-only-for-the-shell', 'shell-uses-feature-barrels-only'],
  },
];

for (const { preset, config, rules, entries = ['src'] } of CASES) describe(preset, () => {
  it('Given the clean twin, When cruised with the real preset, Then nothing is reported', async () => {
    assert.deepEqual(await violations(preset, 'clean', config, entries), []);
  });
  it('Given the dirty tree with one offending import per rule, When cruised, Then exactly those rules are reported', async () => {
    assert.deepEqual(await violations(preset, 'dirty', config, entries), [...rules].sort());
  });
});

const kit = path.join(planted, '..', '..');
const frontend = path.join(kit, '..');

function oxlintRules(variant: 'clean' | 'dirty'): Record<string, string[]> {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'oxlint-planted-'));
  try {
    cpSync(path.join(planted, 'oxlint', variant), dir, { recursive: true });
    const config = readFileSync(path.join(frontend, '.oxlintrc.json'), 'utf8')
      .replace('./libraries/eslint-plugin-mobile-learn', path.join(frontend, 'libraries', 'eslint-plugin-mobile-learn'))
      .replace('"typeAware": true', '"typeAware": false');
    writeFileSync(path.join(dir, '.oxlintrc.json'), config);
    const run = spawnSync(path.join(frontend, 'mla-tamagui', 'node_modules', '.bin', 'oxlint'), ['-c', '.oxlintrc.json', '--format=unix', '.'], { cwd: dir, encoding: 'utf8' });
    const found: Record<string, string[]> = {};
    for (const line of run.stdout.split('\n')) {
      const m = /^(.+?):\d+:\d+: .*\[(?:Error|Warning)\/eslint\(([\w-]+)\)\]/.exec(line);
      if (m) (found[m[1].replace(/^\.\//, '')] ??= []).push(m[2]);
    }
    return found;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe('oxlint arch overrides', () => {
  it('Given the clean twin, When linted with the real config, Then nothing is reported', () => {
    assert.deepEqual(oxlintRules('clean'), {});
  });
  it('Given one offending line per rule, When linted, Then each file reports exactly its rule', () => {
    assert.deepEqual(oxlintRules('dirty'), {
      'mla-tamagui/features/f/ui/Net.tsx': ['no-restricted-globals'],
      'mla-tamagui/features/f/ui/Store.tsx': ['no-restricted-imports'],
      'libraries/gesture-geometry/src/p.ts': ['no-restricted-globals'],
      'libraries/gesture-geometry/src/g.ts': ['no-restricted-properties'],
      'libraries/gesture-geometry/src/f.ts': ['no-restricted-imports'],
      'libraries/gesture-geometry/src/b.ts': ['no-restricted-imports'],
      'mla-tamagui/tests/flow/spy.flow.test.tsx': ['no-restricted-properties'],
      'libraries/gesture-geometry/src/m.test.ts': ['no-restricted-properties'],
    });
  });
});
