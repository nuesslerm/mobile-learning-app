import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { cruise } from 'dependency-cruiser';
import { describe, expect, it } from 'vitest';

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

describe.each(CASES)('$preset', ({ preset, config, rules, entries = ['src'] }) => {
  it('Given the clean twin, When cruised with the real preset, Then nothing is reported', async () => {
    expect(await violations(preset, 'clean', config, entries)).toEqual([]);
  });
  it('Given the dirty tree with one offending import per rule, When cruised, Then exactly those rules are reported', async () => {
    expect(await violations(preset, 'dirty', config, entries)).toEqual([...rules].sort());
  });
});
