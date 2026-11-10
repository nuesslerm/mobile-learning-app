import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const pluginEntry = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../dist/esm/index.js');
const oxlint = path.join(path.dirname(createRequire(import.meta.url).resolve('oxlint/package.json')), 'bin', 'oxlint');

/** One planted file per rule the repo's oxlint configs run from this plugin. */
const PLANTED: Record<string, { file: string; code: string }> = {
  'no-hermes-unsupported-api': { file: 'hermes.ts', code: 'export const s = AbortSignal.timeout(5);\n' },
  'no-unhandled-catch': { file: 'catch.ts', code: 'export function f(g: () => void) {\n  try {\n    g();\n  } catch (e) {\n    void e;\n  }\n}\n' },
};

function lint(dir: string): string {
  const rules = Object.fromEntries(Object.keys(PLANTED).map((rule) => [`mobile-learn/${rule}`, 'error']));
  writeFileSync(path.join(dir, '.oxlintrc.json'), JSON.stringify({ jsPlugins: [pluginEntry], rules }));
  try {
    return execFileSync(oxlint, ['--format', 'unix', '.'], { cwd: dir, encoding: 'utf8' });
  } catch (err) {
    const out = (err as { stdout?: string }).stdout;
    if (typeof out !== 'string') throw err;
    return out;
  }
}

describe('the plugin under oxlint', () => {
  it('Given one planted file per kept rule, When oxlint runs the plugin, Then each rule reports on its own file', () => {
    // Given a directory with one planted violation per rule
    const dir = mkdtempSync(path.join(os.tmpdir(), 'mobile-learn-oxlint-'));
    for (const { file, code } of Object.values(PLANTED)) {
      mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
      writeFileSync(path.join(dir, file), code);
    }

    // When oxlint lints it with every kept rule on
    const out = lint(dir);

    // Then every rule fires on its planted file
    for (const [rule, { file }] of Object.entries(PLANTED)) {
      expect(out, rule).toMatch(new RegExp(`^${file.replace('.', '\\.')}:\\d+:\\d+: .*mobile-learn\\(${rule}\\)`, 'm'));
    }
  });

  it('Given an unawaited promise, a promise where void is expected and an await on a plain value, When oxlint runs type-aware, Then each promise rule reports, and node:test calls do not', () => {
    // Given a typed project with one case per promise rule and a node:test file
    const dir = mkdtempSync(path.join(os.tmpdir(), 'mobile-learn-promises-'));
    writeFileSync(path.join(dir, 'tsconfig.json'), JSON.stringify({ compilerOptions: { strict: true, module: 'nodenext', target: 'es2022', types: [] }, include: ['*.ts'] }));
    writeFileSync(path.join(dir, 'floating.ts'), 'export async function save(): Promise<void> {}\nexport function f(): void {\n  save();\n}\n');
    writeFileSync(path.join(dir, 'misused.ts'), 'export async function save(): Promise<void> {}\nexport const run = (cb: () => void): void => cb();\nrun(async () => save());\n');
    writeFileSync(path.join(dir, 'thenable.ts'), 'export async function f(): Promise<number> {\n  return await 1;\n}\n');
    writeFileSync(path.join(dir, 'runner.test.ts'), "import { describe, it } from 'node:test';\ndescribe('x', () => {\n  it('y', () => {});\n});\n");
    const config = JSON.parse(readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../.oxlintrc.json'), 'utf8').replace(/^\s*\/\/.*$/gm, ''));
    const rules = Object.fromEntries(Object.entries(config.rules as Record<string, unknown>).filter(([name]) => name.startsWith('typescript/')));
    writeFileSync(path.join(dir, '.oxlintrc.json'), JSON.stringify({ plugins: ['typescript'], options: { typeAware: true }, rules }));

    // When oxlint lints it with the repo's promise rules, type-aware
    let out: string;
    try {
      out = execFileSync(oxlint, ['--format', 'unix', '.'], { cwd: dir, encoding: 'utf8' });
    } catch (err) {
      out = (err as { stdout: string }).stdout;
    }

    // Then every rule fires on its own file, and the node:test calls stay clean
    expect(out).toMatch(/^floating\.ts:3:\d+: .*no-floating-promises/m);
    expect(out).toMatch(/^misused\.ts:3:\d+: .*no-misused-promises/m);
    expect(out).toMatch(/^thenable\.ts:2:\d+: .*await-thenable/m);
    expect(out).not.toMatch(/^runner\.test\.ts/m);
  });
});
