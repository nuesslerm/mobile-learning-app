import { beforeAll, describe, expect, it, vi } from 'vitest';

import type { TestCase } from '../../../domain/run/execution';
import type { JudgePayload } from '../judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

const TRIE = `class Trie:
    def __init__(self):
        self.words = set()

    def insert(self, word):
        self.words.add(word)

    def search(self, word):
        return word in self.words

    def startsWith(self, prefix):
        return any(w.startswith(prefix) for w in self.words)
`;

const OPERATIONS = ['Trie', 'insert', 'search', 'search', 'startsWith', 'insert', 'search'];
const ARGUMENTS = [[], ['apple'], ['apple'], ['app'], ['app'], ['app'], ['app']];
const LEETCODE_EXAMPLE: TestCase = { inputs: { operations: OPERATIONS, arguments: ARGUMENTS }, expected: [null, null, true, false, true, null, true] };

const design = (code: string, testCases: TestCase[], extra: Partial<JudgePayload> = {}): JudgePayload => ({ code, methodName: '', testCases, harness: { kind: 'design', className: 'Trie' }, ...extra });
const trieCase = (operations: string[], args: unknown[][], expected: unknown[]): TestCase => ({ inputs: { operations, arguments: args }, expected });

describe('the design harness', () => {
  let judge: NodeJudge;
  beforeAll(async () => {
    judge = await bootNodeJudge();
  });

  it('given a correct Trie and operations [Trie, insert, search, search, startsWith, insert, search], when it runs, then actual is [null,null,true,false,true,null,true] and the case passes', () => {
    const row = judge.run(design(TRIE, [LEETCODE_EXAMPLE])).results?.[0];

    expect(row).toMatchObject({ pass: true, actual: [null, null, true, false, true, null, true], error: null });
    expect(row?.input).toEqual(LEETCODE_EXAMPLE.inputs);
  });

  it('given two cases on the same class, when the second runs, then it starts from a new instance with no words from the first', () => {
    const first = trieCase(['Trie', 'insert', 'search'], [[], ['apple'], ['apple']], [null, null, true]);
    const second = trieCase(['Trie', 'search'], [[], ['apple']], [null, false]);

    expect(judge.run(design(TRIE, [first, second])).results?.map((r) => r.pass)).toEqual([true, true]);
  });

  it('given the starter left as is where every method returns None, when it runs, then the case fails and actual shows the nulls', () => {
    const starter = 'class Trie:\n    def __init__(self):\n        pass\n    def insert(self, word):\n        pass\n    def search(self, word):\n        pass\n    def startsWith(self, prefix):\n        pass\n';

    const row = judge.run(design(starter, [LEETCODE_EXAMPLE])).results?.[0];

    expect(row).toMatchObject({ pass: false, actual: [null, null, null, null, null, null, null], error: null });
  });

  it('given a startsWith that is wrong, when it runs, then only the cases that call it fail and actual shows the whole list', () => {
    const wrong = TRIE.replace('return any(w.startswith(prefix) for w in self.words)', 'return False');
    const withoutStartsWith = trieCase(['Trie', 'insert', 'search'], [[], ['apple'], ['apple']], [null, null, true]);

    const rows = judge.run(design(wrong, [withoutStartsWith, LEETCODE_EXAMPLE])).results;

    expect(rows?.map((r) => r.pass)).toEqual([true, false]);
    expect(rows?.[1]?.actual).toEqual([null, null, true, false, false, null, true]);
  });

  it('given a class without the method an operation names, when it runs, then the row error is AttributeError ending (operation 4: startsWith) and actual holds what ran before it', () => {
    const withoutStartsWith = TRIE.replace(/\n {4}def startsWith[\s\S]*$/, '\n');

    const row = judge.run(design(withoutStartsWith, [LEETCODE_EXAMPLE])).results?.[0];

    expect(row?.pass).toBe(false);
    expect(row?.error).toMatch(/^AttributeError: .*\(operation 4: startsWith\)$/);
    expect(row?.actual).toEqual([null, null, true, false]);
    expect(row?.error_at).toBeUndefined();
  });

  it('given a method that raises inside the learner’s code, when it runs, then the row carries the error, the operation and the line, and the next case still runs', () => {
    const raising = TRIE.replace('return word in self.words', "raise KeyError(word)\n        return word in self.words");
    const search = trieCase(['Trie', 'search'], [[], ['x']], [null, false]);
    const insertOnly = trieCase(['Trie', 'insert'], [[], ['x']], [null, null]);

    const rows = judge.run(design(raising, [search, insertOnly])).results;

    expect(rows?.[0]).toMatchObject({ pass: false, error: "KeyError: 'x' (operation 1: search)", error_at: { line: 9 }, actual: [null] });
    expect(rows?.[1]?.pass).toBe(true);
  });

  it('given an operation that never returns, when it runs with a short limit, then that case times out naming the operation and the next case still passes', () => {
    const hanging = TRIE.replace('return word in self.words', "while word == 'hang':\n            pass\n        return word in self.words");
    const hang = trieCase(['Trie', 'insert', 'search'], [[], ['hang'], ['hang']], [null, null, true]);

    const rows = judge.run(design(hanging, [hang, LEETCODE_EXAMPLE], { timeoutMs: 1000 })).results;

    expect(rows?.[0]?.pass).toBe(false);
    expect(rows?.[0]?.error).toBe('TimeoutError: Code execution timed out (1s limit) (operation 2: search)');
    expect(rows?.[1]?.pass).toBe(true);
  });

  it('given no class of the harness’s name, when it runs, then the run fails as a compile error No Trie class defined', () => {
    const raw = judge.run(design('class Solution:\n    pass\n', [LEETCODE_EXAMPLE]));

    expect(raw).toMatchObject({ ok: false, error_kind: 'compile', error: 'No Trie class defined' });
  });

  it('given a print inside an operation, when it runs, then the text is that row’s stdout', () => {
    const printing = TRIE.replace('self.words.add(word)', "print('adding', word)\n        self.words.add(word)");
    const quiet = trieCase(['Trie', 'search'], [[], ['x']], [null, false]);

    const rows = judge.run(design(printing, [LEETCODE_EXAMPLE, quiet])).results;

    expect(rows?.[0]?.stdout).toBe('adding apple\nadding app\n');
    expect(rows?.[1]?.stdout).toBe('');
  });

  it('given operations and arguments of different lengths, when it runs, then that row is a ValueError and the run goes on', () => {
    const uneven = trieCase(['Trie', 'insert', 'search'], [[], ['apple']], [null, null, true]);

    const raw = judge.run(design(TRIE, [uneven, LEETCODE_EXAMPLE]));

    expect(raw.ok).toBe(true);
    expect(raw.results?.[0]).toMatchObject({ pass: false, actual: [] });
    expect(raw.results?.[0]?.error).toMatch(/^ValueError: operations and arguments must have the same length/);
    expect(raw.results?.[1]?.pass).toBe(true);
  });

  it('given a case with no operations key, when it runs, then that row is a ValueError and the run goes on', () => {
    const missing: TestCase = { inputs: { arguments: [[]] }, expected: [null] };

    const rows = judge.run(design(TRIE, [missing, LEETCODE_EXAMPLE])).results;

    expect(rows?.[0]?.error).toMatch(/^ValueError: a design case needs/);
    expect(rows?.[1]?.pass).toBe(true);
  });

  it('given a constructor that raises, when it runs, then the row names operation 0', () => {
    const broken = TRIE.replace('self.words = set()', 'self.words = undefined_name');

    const row = judge.run(design(broken, [LEETCODE_EXAMPLE])).results?.[0];

    expect(row?.error).toBe("NameError: name 'undefined_name' is not defined (operation 0: Trie)");
    expect(row?.actual).toEqual([]);
  });
});
