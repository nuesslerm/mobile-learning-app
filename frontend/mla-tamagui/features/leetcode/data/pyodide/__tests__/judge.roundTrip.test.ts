import { beforeAll, describe, expect, it, vi } from 'vitest';

import type { TestCase } from '../../../domain/run/execution';
import type { JudgePayload } from '../../runner/judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

const STRING_CODEC = `class Codec:
    def encode(self, strs):
        return ''.join(str(len(s)) + '#' + s for s in strs)

    def decode(self, s):
        out, i = [], 0
        while i < len(s):
            j = s.index('#', i)
            n = int(s[i:j])
            out.append(s[j + 1:j + 1 + n])
            i = j + 1 + n
        return out
`;

const TREE_CODEC = `class Codec:
    def serialize(self, root):
        out = []

        def walk(node):
            if node is None:
                out.append('#')
                return
            out.append(str(node.val))
            walk(node.left)
            walk(node.right)

        walk(root)
        return ','.join(out)

    def deserialize(self, data):
        values = iter(data.split(','))

        def build():
            value = next(values)
            if value == '#':
                return None
            node = TreeNode(int(value))
            node.left = build()
            node.right = build()
            return node

        return build()
`;

const strings = (code: string, lists: string[][], extra: Partial<JudgePayload> = {}): JudgePayload => ({
  code,
  methodName: '',
  testCases: lists.map((strs): TestCase => ({ inputs: { strs }, expected: strs })),
  paramTypes: {},
  harness: { kind: 'roundTrip', className: 'Codec', encode: 'encode', decode: 'decode' },
  ...extra,
});
const trees = (code: string, lists: (number | null)[][], extra: Partial<JudgePayload> = {}): JudgePayload => ({
  code,
  methodName: '',
  testCases: lists.map((root): TestCase => ({ inputs: { root }, expected: root })),
  paramTypes: { root: 'TreeNode' },
  harness: { kind: 'roundTrip', className: 'Codec', encode: 'serialize', decode: 'deserialize' },
  ...extra,
});

describe('the round-trip harness', () => {
  let judge: NodeJudge;
  beforeAll(async () => {
    judge = await bootNodeJudge();
  });

  it('given a codec with length-prefix encoding, when it runs on [Hello, World], [], [""], [#], [3#abc] and unicode strings, then every case passes', () => {
    const lists = [['Hello', 'World'], [], [''], ['#'], ['3#abc'], ['héllo', '日本語', '😀#1']];

    const rows = judge.run(strings(STRING_CODEC, lists)).results;

    expect(rows?.map((r) => r.pass)).toEqual([true, true, true, true, true, true]);
    expect(rows?.[0]).toMatchObject({ actual: ['Hello', 'World'], input: { strs: ['Hello', 'World'] }, error: null });
  });

  it('given an encode that returns the list itself, when it runs, then the row is a TypeError naming str', () => {
    const code = 'class Codec:\n    def encode(self, strs):\n        return strs\n    def decode(self, s):\n        return s\n';

    const row = judge.run(strings(code, [['a']])).results?.[0];

    expect(row).toMatchObject({ pass: false, error: 'TypeError: encode must return a str, got list' });
  });

  it('given a codec that stores the input on self in encode and returns it from decode, when it runs, then the case fails because decode runs on a new instance', () => {
    const code = "class Codec:\n    def encode(self, strs):\n        self.kept = strs\n        return 'x'\n    def decode(self, s):\n        return self.kept\n";

    const row = judge.run(strings(code, [['a', 'b']])).results?.[0];

    expect(row?.pass).toBe(false);
    expect(row?.error).toMatch(/^AttributeError: .*kept/);
  });

  it('given a tree codec with preorder and null markers, when it runs on [1,2,3,null,null,4,5], [], [1], [1,null,2], then every case passes and the decoded tree reads back as the canonical level-order list', () => {
    const lists = [[1, 2, 3, null, null, 4, 5], [], [1], [1, null, 2]];

    const rows = judge.run(trees(TREE_CODEC, lists)).results;

    expect(rows?.map((r) => r.pass)).toEqual([true, true, true, true]);
    expect(rows?.map((r) => r.actual)).toEqual(lists);
  });

  it('given a deserialize that returns the original tree it was given through a global, when it runs, then the row is a ValueError saying the input tree was returned', () => {
    const code = "kept = []\nclass Codec:\n    def serialize(self, root):\n        kept.append(root)\n        return 'x'\n    def deserialize(self, data):\n        return kept[-1]\n";

    const rows = judge.run(trees(code, [[1, 2, 3], []])).results;

    expect(rows?.[0]).toMatchObject({ pass: false, error: 'ValueError: the returned tree is the input tree, not a decoded copy' });
    expect(rows?.[1]?.pass).toBe(true);
  });

  it('given a deserialize that returns a new tree sharing a subtree with the input, when it runs, then the row is the same ValueError', () => {
    const code = "kept = []\nclass Codec:\n    def serialize(self, root):\n        kept.append(root)\n        return 'x'\n    def deserialize(self, data):\n        return TreeNode(kept[-1].val, kept[-1].left, kept[-1].right)\n";

    const row = judge.run(trees(code, [[1, 2, 3]])).results?.[0];

    expect(row?.error).toBe('ValueError: the returned tree is the input tree, not a decoded copy');
  });

  it('given a deserialize that raises, when it runs, then the row carries the error and the line, and the next case still runs', () => {
    const code = TREE_CODEC.replace("        values = iter(data.split(','))", "        if data.startswith('2'):\n            raise RuntimeError('boom')\n        values = iter(data.split(','))");
    const raiseLine = code.split('\n').findIndex((line) => line.includes("raise RuntimeError('boom')")) + 1;

    const rows = judge.run(trees(code, [[2], [1]])).results;

    expect(rows?.[0]).toMatchObject({ pass: false, error: 'RuntimeError: boom', error_at: { line: raiseLine } });
    expect(rows?.[1]?.pass).toBe(true);
  });

  it('given an encode that never returns, when it runs with a short limit, then that case times out and the others pass', () => {
    const code = STRING_CODEC.replace("        return ''.join", "        while strs == ['hang']:\n            pass\n        return ''.join");

    const rows = judge.run(strings(code, [['hang'], ['a', 'b']], { timeoutMs: 1000 })).results;

    expect(rows?.[0]).toMatchObject({ pass: false, error: 'TimeoutError: Code execution timed out (1s limit)' });
    expect(rows?.[1]?.pass).toBe(true);
  });

  it('given a print inside encode, when it runs, then the text is that row’s stdout', () => {
    const code = STRING_CODEC.replace('    def encode(self, strs):', "    def encode(self, strs):\n        print('encoding', len(strs))");

    const rows = judge.run(strings(code, [['a'], ['b', 'c']])).results;

    expect(rows?.map((r) => r.stdout)).toEqual(['encoding 1\n', 'encoding 2\n']);
  });
});
