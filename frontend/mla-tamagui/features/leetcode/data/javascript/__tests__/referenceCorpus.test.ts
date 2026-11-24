import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { entryPointsOf, loadReferenceCorpus, REFERENCE_DIR } from './support/referenceCorpus';

const dirs: string[] = [];
function corpusDirWith(files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), 'js-reference-corpus-'));
  dirs.push(dir);
  for (const [name, code] of Object.entries(files)) writeFileSync(join(dir, name), code);
  return dir;
}

describe('the JavaScript reference corpus', () => {
  afterEach(() => {
    for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
  });

  it('Given a file named for no catalog problem, When the corpus loads, Then it fails and names the file', () => {
    const dir = corpusDirWith({ 'not-a-problem.js': 'var f = function() {};\n' });
    expect(() => loadReferenceCorpus(dir)).toThrow('not-a-problem.js is named for no catalog problem');
  });

  it('Given a reference that defines nothing the harness can call, When the corpus loads, Then it fails and names the file and what it lacks', () => {
    const dir = corpusDirWith({ 'two-sum.js': 'var other = function() {};\n' });
    expect(() => loadReferenceCorpus(dir)).toThrow('two-sum.js defines none of Solution, twoSum');
  });

  it('Given references written as a class, a var function and a function declaration, When the corpus loads, Then each is accepted for its problem', () => {
    const dir = corpusDirWith({
      'two-sum.js': 'class Solution {\n  twoSum() {}\n}\n',
      'valid-palindrome.js': 'var isPalindrome = function() {};\n',
      'valid-parentheses.js': 'function isValid() {}\n',
    });
    expect(loadReferenceCorpus(dir).map((entry) => entry.problem)).toEqual(['two-sum', 'valid-palindrome', 'valid-parentheses']);
  });

  it('Given a name that only appears in a comment or inside a line, When the corpus loads, Then it does not count as a definition', () => {
    const dir = corpusDirWith({ 'two-sum.js': '// class Solution {}\n  var inner = function() { var twoSum = 1; };\n' });
    expect(() => loadReferenceCorpus(dir)).toThrow('two-sum.js defines none of');
  });

  it('Given a folder inside the corpus directory, When the corpus loads, Then only the files of the directory itself are references', () => {
    const dir = corpusDirWith({ 'two-sum.js': 'class Solution {\n  twoSum() {}\n}\n' });
    mkdirSync(join(dir, 'brute'));
    writeFileSync(join(dir, 'brute', 'no-problem.js'), 'not javascript at all');
    expect(loadReferenceCorpus(dir).map((entry) => entry.problem)).toEqual(['two-sum']);
  });

  it('Given a design problem, a round trip problem and a plain one, When the entry points are asked, Then they are the harness class or Solution, and the method', () => {
    expect(entryPointsOf('implement-trie-prefix-tree')[0]).toBe('Trie');
    expect(entryPointsOf('serialize-and-deserialize-binary-tree')).toEqual(['Codec', 'serialize']);
    expect(entryPointsOf('clone-graph')).toEqual(['Solution', 'cloneGraph']);
  });

  it('Given every reference file, When its source is read, Then it is named for a catalog problem and defines one of its entry points', () => {
    const corpus = loadReferenceCorpus(REFERENCE_DIR);
    for (const { problem } of corpus) expect(ProblemCatalog.getById(problem), problem).toBeDefined();
    expect(corpus.length).toBeGreaterThanOrEqual(11);
  });
});
