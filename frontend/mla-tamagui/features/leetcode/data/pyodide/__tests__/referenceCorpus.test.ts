import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { BRUTE_DIR, classNameOf, loadReferenceCorpus, REFERENCE_DIR } from './support/referenceCorpus';

const dirs: string[] = [];
function corpusDirWith(files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), 'reference-corpus-'));
  dirs.push(dir);
  for (const [name, code] of Object.entries(files)) writeFileSync(join(dir, name), code);
  return dir;
}

describe('the reference corpus', () => {
  afterEach(() => {
    for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
  });

  it('given a file in the corpus directory named for no catalog problem, when the corpus loads, then it fails and names the file', () => {
    const dir = corpusDirWith({ 'not-a-problem.py': 'class Solution:\n    pass\n' });

    expect(() => loadReferenceCorpus(dir)).toThrow('not-a-problem.py');
  });

  it('given a reference that does not define the class of its starter, when the corpus loads, then it fails and names the file', () => {
    const dir = corpusDirWith({ 'two-sum.py': 'class Other:\n    pass\n' });

    expect(() => loadReferenceCorpus(dir)).toThrow('two-sum.py does not define class Solution');
  });

  it('given a folder inside the corpus directory, when the corpus loads, then only the files of the directory itself are references', () => {
    const dir = corpusDirWith({ 'two-sum.py': 'class Solution:\n    pass\n' });
    mkdirSync(join(dir, 'brute'));
    writeFileSync(join(dir, 'brute', 'no-problem.py'), 'not python at all');

    expect(loadReferenceCorpus(dir).map((entry) => entry.problem)).toEqual(['two-sum']);
  });

  it('given the oracle directory, when it loads, then every oracle is named for a catalog problem and defines the class its problem needs', () => {
    const oracles = loadReferenceCorpus(BRUTE_DIR);

    expect(oracles.length).toBeGreaterThan(0);
    for (const { problem, code } of oracles) expect(code, problem).toMatch(new RegExp(`^class ${classNameOf(problem)}\\b`, 'm'));
  });

  it('given a design problem and a problem with a Solution class, when the class a reference must define is asked, then it is the harness class or Solution', () => {
    expect(classNameOf('implement-trie-prefix-tree')).toBe('Trie');
    expect(classNameOf('serialize-and-deserialize-binary-tree')).toBe('Codec');
    expect(classNameOf('clone-graph')).toBe('Solution');
  });

  it('given every reference file, when its source is read, then it defines the class the catalog starter defines', () => {
    const corpus = loadReferenceCorpus(REFERENCE_DIR);

    for (const { problem, code } of corpus) {
      expect(code, problem).toMatch(new RegExp(`^class ${classNameOf(problem)}\\b`, 'm'));
      expect(ProblemCatalog.getById(problem), problem).toBeDefined();
    }
    expect(corpus.length).toBeGreaterThanOrEqual(29);
  });
});
