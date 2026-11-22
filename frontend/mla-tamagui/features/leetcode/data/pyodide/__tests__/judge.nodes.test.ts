import { beforeAll, describe, expect, it, vi } from 'vitest';

import type { TestCase } from '../../../domain/run/execution';
import type { JudgePayload } from '../../runner/judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

const SQUARE = [[2, 4], [1, 3], [2, 4], [1, 3]];

const CLONE = `from collections import deque


class Solution:
    def cloneGraph(self, node):
        if node is None:
            return None
        copies = {node: Node(node.val)}
        queue = deque([node])
        while queue:
            current = queue.popleft()
            for neighbour in current.neighbors:
                if neighbour not in copies:
                    copies[neighbour] = Node(neighbour.val)
                    queue.append(neighbour)
                copies[current].neighbors.append(copies[neighbour])
        return copies[node]
`;

const FLOYD = `class Solution:
    def hasCycle(self, head):
        slow = fast = head
        while fast and fast.next:
            slow = slow.next
            fast = fast.next.next
            if slow is fast:
                return True
        return False
`;

const graph = (code: string, adjacencies: number[][][], extra: Partial<JudgePayload> = {}): JudgePayload => ({
  code,
  methodName: 'cloneGraph',
  testCases: adjacencies.map((node): TestCase => ({ inputs: { node }, expected: node })),
  paramTypes: { node: 'GraphNode' },
  returnType: 'GraphNode',
  ...extra,
});

const cycle = (code: string, cases: { head: number[]; pos: number; expected: boolean }[], extra: Partial<JudgePayload> = {}): JudgePayload => ({
  code,
  methodName: 'hasCycle',
  testCases: cases.map(({ head, pos, expected }): TestCase => ({ inputs: { head, pos }, expected })),
  paramTypes: { head: 'ListNode', pos: 'ListNode_cycle_pos' },
  returnType: null,
  ...extra,
});

describe('the node kinds of the design and graph problems', () => {
  let judge: NodeJudge;
  beforeAll(async () => {
    judge = await bootNodeJudge();
  });

  describe('a graph argument and result', () => {
    it('given a BFS clone of [[2,4],[1,3],[2,4],[1,3]], when it runs with paramTypes {node: GraphNode} and returnType GraphNode, then the case passes', () => {
      const row = judge.run(graph(CLONE, [SQUARE])).results?.[0];

      expect(row).toMatchObject({ pass: true, actual: SQUARE, input: { node: SQUARE }, error: null });
    });

    it('given [] and [[]], when a correct clone runs, then they pass: None in and None out, and one node with no neighbours', () => {
      const rows = judge.run(graph(CLONE, [[], [[]]])).results;

      expect(rows?.map((r) => r.pass)).toEqual([true, true]);
      expect(rows?.map((r) => r.actual)).toEqual([[], [[]]]);
    });

    it('given a clone that returns the very node it was given, when it runs, then the row is the shares-a-node error', () => {
      const code = 'class Solution:\n    def cloneGraph(self, node):\n        return node\n';

      const row = judge.run(graph(code, [SQUARE])).results?.[0];

      expect(row).toMatchObject({ pass: false, error: 'ValueError: the returned graph shares a node with the input: build copies' });
    });

    it('given a clone that copies nodes but reuses the neighbours list, when it runs, then the row is the shares-a-node error', () => {
      const code = 'class Solution:\n    def cloneGraph(self, node):\n        return Node(node.val, node.neighbors)\n';

      const row = judge.run(graph(code, [SQUARE])).results?.[0];

      expect(row).toMatchObject({ pass: false, error: 'ValueError: the returned graph shares a node with the input: build copies' });
    });

    it('given a clone whose neighbours come out in another order, when it runs, then the case fails and actual shows the order it built', () => {
      const code = CLONE.replace('copies[current].neighbors.append(copies[neighbour])', 'copies[current].neighbors.insert(0, copies[neighbour])');

      const row = judge.run(graph(code, [SQUARE])).results?.[0];

      expect(row).toMatchObject({ pass: false, actual: [[4, 2], [3, 1], [4, 2], [3, 1]], error: null });
    });

    it('given a clone whose nodes carry the wrong vals, when it runs, then the row says the vals must be 1 to n', () => {
      const code = CLONE.replace('Node(node.val)', 'Node(node.val + 10)').replace('Node(neighbour.val)', 'Node(neighbour.val + 10)');

      const row = judge.run(graph(code, [SQUARE])).results?.[0];

      expect(row?.error).toMatch(/^ValueError: the returned graph must have the vals 1 to 4/);
    });

    it('given a neighbour that is no node of the input graph, when it runs, then the row is a ValueError and the run goes on', () => {
      const bad: TestCase = { inputs: { node: [[3]] }, expected: [[3]] };
      const good: TestCase = { inputs: { node: SQUARE }, expected: SQUARE };

      const rows = judge.run({ ...graph(CLONE, []), testCases: [bad, good] }).results;

      expect(rows?.[0]?.error).toMatch(/^ValueError: neighbour 3 is not a node of the graph/);
      expect(rows?.[1]?.pass).toBe(true);
    });
  });

  describe('a cyclic list with a harness-only pos', () => {
    const CASES = [
      { head: [3, 2, 0, -4], pos: 1, expected: true },
      { head: [1, 2], pos: 0, expected: true },
      { head: [1], pos: 0, expected: true },
      { head: [1], pos: -1, expected: false },
      { head: [], pos: -1, expected: false },
    ];

    it('given Floyd’s hasCycle(self, head) and the cyclic and acyclic cases, when it runs, then the cases pass and the method receives one argument, not two', () => {
      const rows = judge.run(cycle(FLOYD, CASES)).results;

      expect(rows?.map((r) => r.pass)).toEqual([true, true, true, true, true]);
      expect(rows?.[0]).toMatchObject({ input: { head: [3, 2, 0, -4], pos: 1 }, actual: true, error: null });
    });

    it('given a solution that follows next until None, when it runs on a cyclic case with a short limit, then that case times out and the acyclic cases pass', () => {
      const code = 'class Solution:\n    def hasCycle(self, head):\n        while head:\n            head = head.next\n        return False\n';

      const rows = judge.run(cycle(code, [{ head: [1, 2], pos: 0, expected: true }, { head: [1, 2], pos: -1, expected: false }], { timeoutMs: 1000 })).results;

      expect(rows?.[0]).toMatchObject({ pass: false, error: 'TimeoutError: Code execution timed out (1s limit)' });
      expect(rows?.[1]?.pass).toBe(true);
    });

    it('given pos beyond the list, when it runs, then the row is a ValueError and the run goes on', () => {
      const rows = judge.run(cycle(FLOYD, [{ head: [1], pos: 5, expected: true }, { head: [1, 2], pos: 1, expected: true }])).results;

      expect(rows?.[0]?.error).toMatch(/^ValueError: pos 5 is not -1 or an index of the list \(1 nodes\)/);
      expect(rows?.[1]?.pass).toBe(true);
    });

    it('given a pos with no ListNode parameter before it, when it runs, then the row is a ValueError', () => {
      const payload: JudgePayload = { ...cycle(FLOYD, []), testCases: [{ inputs: { pos: 0 }, expected: false }], paramTypes: { pos: 'ListNode_cycle_pos' } };

      expect(judge.run(payload).results?.[0]?.error).toMatch(/^ValueError: 'pos' links the list before it/);
    });
  });

  it('given an in-place parameter that follows a harness-only key, when it runs, then the mutated parameter is the one compared, found by name and not by position', () => {
    const code = 'class Solution:\n    def sortInPlace(self, head, nums):\n        nums.sort()\n';
    const payload: JudgePayload = {
      code,
      methodName: 'sortInPlace',
      testCases: [{ inputs: { head: [1, 2], pos: -1, nums: [3, 1, 2] }, expected: [1, 2, 3] }],
      paramTypes: { head: 'ListNode', pos: 'ListNode_cycle_pos' },
      returnType: null,
      inPlace: 'nums',
    };

    const row = judge.run(payload).results?.[0];

    expect(row).toMatchObject({ pass: true, actual: [1, 2, 3], error: null });
  });
});
