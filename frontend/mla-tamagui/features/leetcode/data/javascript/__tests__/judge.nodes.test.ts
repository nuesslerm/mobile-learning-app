import { describe, expect, it } from 'vitest';

import { GraphNode, ListNode, TreeNode, buildArgs, findByVal, linkCycle, readValue, reachable, ValueError, type NodeConstructors } from '../harness/nodes';

const SQUARE = [[2, 4], [1, 3], [2, 4], [1, 3]];

const failure = (run: () => unknown): Error => {
  try {
    run();
  } catch (error) {
    return error as Error;
  }
  throw new Error('it did not throw');
};

describe('a tree argument and result', () => {
  it('Given a level-order list with nulls, When buildArgs runs with {root: TreeNode}, Then the tree has the right shape', () => {
    const { args } = buildArgs({ root: [3, 9, 20, null, null, 15, 7] }, { root: 'TreeNode' });
    const root = args[0] as TreeNode;
    expect(root.val).toBe(3);
    expect(root.left?.val).toBe(9);
    expect(root.left?.left).toBeNull();
    expect(root.right?.left?.val).toBe(15);
    expect(root.right?.right?.val).toBe(7);
  });

  it('Given an empty list or a null root, When buildArgs runs, Then the argument is null', () => {
    expect(buildArgs({ root: [] }, { root: 'TreeNode' }).args).toEqual([null]);
    expect(buildArgs({ root: [null] }, { root: 'TreeNode' }).args).toEqual([null]);
  });

  it('Given a tree, When it is read back, Then it is its level order with the trailing nulls trimmed', () => {
    const { args } = buildArgs({ root: [1, null, 2, 3] }, { root: 'TreeNode' });
    expect(readValue('TreeNode', args[0])).toEqual([1, null, 2, 3]);
    expect(readValue('TreeNode', null)).toEqual([]);
    expect(readValue('TreeNode', undefined)).toEqual([]);
  });

  it('Given a tree whose node is reachable twice, When it is read back, Then the row is the not-a-tree error', () => {
    const shared = new TreeNode(2);
    const root = new TreeNode(1, shared, shared);
    expect(failure(() => readValue('TreeNode', root)).message).toBe('the returned tree is not a tree');
  });

  it('Given TreeNode_val, When the answer is a node, Then its val is read, and anything else is left as it is', () => {
    expect(readValue('TreeNode_val', new TreeNode(7))).toBe(7);
    expect(readValue('TreeNode_val', null)).toBeNull();
    expect(readValue('TreeNode_val', 4)).toBe(4);
  });

  it('Given {root: TreeNode, p: TreeNode_by_val}, When buildArgs runs, Then p is the node of that value in the first tree', () => {
    const { args } = buildArgs({ root: [6, 2, 8, 0, 4], p: 4 }, { root: 'TreeNode', p: 'TreeNode_by_val' });
    expect((args[1] as TreeNode).val).toBe(4);
    expect(reachable(args[0]).has(args[1])).toBe(true);
  });

  it('Given a value that is not in the tree, When buildArgs runs with TreeNode_by_val, Then it fails with the no-node text', () => {
    const error = failure(() => buildArgs({ root: [1, 2], p: 9 }, { root: 'TreeNode', p: 'TreeNode_by_val' }));
    expect(error).toBeInstanceOf(ValueError);
    expect(`${error.name}: ${error.message}`).toBe('ValueError: no node with value 9 in the tree');
    expect(failure(() => findByVal(null, 1)).message).toBe('no node with value 1 in the tree');
  });
});

describe('a linked list argument and result', () => {
  it('Given {head: ListNode}, When buildArgs runs, Then the list holds the values in order, and an empty list is null', () => {
    const { args } = buildArgs({ head: [1, 2, 3] }, { head: 'ListNode' });
    expect(readValue('ListNode', args[0])).toEqual([1, 2, 3]);
    expect(buildArgs({ head: [] }, { head: 'ListNode' }).args).toEqual([null]);
  });

  it('Given {lists: ListNode[]}, When buildArgs runs, Then every list is built, an empty one as null', () => {
    const { args } = buildArgs({ lists: [[1, 4], [], [2]] }, { lists: 'ListNode[]' });
    const lists = args[0] as (ListNode | null)[];
    expect(lists.map((list) => readValue('ListNode', list))).toEqual([[1, 4], [], [2]]);
    expect(lists[1]).toBeNull();
  });

  it('Given a returned list, When it is read back, Then it is its values, and null is []', () => {
    expect(readValue('ListNode', new ListNode(1, new ListNode(2)))).toEqual([1, 2]);
    expect(readValue('ListNode', null)).toEqual([]);
    expect(readValue('ListNode', undefined)).toEqual([]);
  });

  it('Given a returned list with a cycle, When it is read back, Then the row is the cycle error', () => {
    const head = new ListNode(1);
    head.next = new ListNode(2, head);
    expect(failure(() => readValue('ListNode', head)).message).toBe('the returned linked list has a cycle');
  });

  it('Given {head: ListNode, pos: ListNode_cycle_pos} and a pos, When buildArgs runs, Then the tail links back to that node and pos is no argument', () => {
    const { args, named } = buildArgs({ head: [3, 2, 0, -4], pos: 1 }, { head: 'ListNode', pos: 'ListNode_cycle_pos' });
    const head = args[0] as ListNode;
    expect(args).toHaveLength(1);
    expect(Object.keys(named)).toEqual(['head']);
    expect(head.next?.next?.next?.next).toBe(head.next);
  });

  it('Given pos -1, When buildArgs runs, Then the list stays open', () => {
    const { args } = buildArgs({ head: [1, 2], pos: -1 }, { head: 'ListNode', pos: 'ListNode_cycle_pos' });
    expect(readValue('ListNode', args[0])).toEqual([1, 2]);
  });

  it('Given a pos beyond the list, When buildArgs runs, Then it fails with the ValueError text naming the list size', () => {
    const error = failure(() => buildArgs({ head: [1], pos: 5 }, { head: 'ListNode', pos: 'ListNode_cycle_pos' }));
    expect(`${error.name}: ${error.message}`).toBe('ValueError: pos 5 is not -1 or an index of the list (1 nodes)');
    expect(failure(() => linkCycle(null, 0)).message).toBe('pos 0 is not -1 or an index of the list (0 nodes)');
  });

  it('Given a pos with no ListNode parameter before it, When buildArgs runs, Then it fails naming the key', () => {
    const error = failure(() => buildArgs({ pos: 0 }, { pos: 'ListNode_cycle_pos' }));
    expect(error.message).toBe("'pos' links the list before it, and no ListNode parameter comes before it");
  });
});

describe('a graph argument and result', () => {
  it('Given an adjacency list, When buildArgs runs with {node: GraphNode}, Then node i has val i + 1 and its neighbours as listed', () => {
    const { args } = buildArgs({ node: SQUARE }, { node: 'GraphNode' });
    const first = args[0] as GraphNode;
    expect(first.val).toBe(1);
    expect(first.neighbors.map((neighbour) => neighbour.val)).toEqual([2, 4]);
    expect(first.neighbors[0]?.neighbors[0]).toBe(first);
    expect(readValue('GraphNode', first)).toEqual(SQUARE);
  });

  it('Given [] and [[]], When buildArgs runs, Then [] is no node at all and [[]] is one node with no neighbours', () => {
    expect(buildArgs({ node: [] }, { node: 'GraphNode' }).args).toEqual([null]);
    const { args } = buildArgs({ node: [[]] }, { node: 'GraphNode' });
    expect(readValue('GraphNode', args[0])).toEqual([[]]);
    expect(readValue('GraphNode', null)).toEqual([]);
  });

  it('Given a neighbour that is no node of the graph, When buildArgs runs, Then it fails with the ValueError text', () => {
    const error = failure(() => buildArgs({ node: [[3]] }, { node: 'GraphNode' }));
    expect(`${error.name}: ${error.message}`).toBe('ValueError: neighbour 3 is not a node of the graph (1 to 1)');
    expect(failure(() => buildArgs({ node: [[true]] }, { node: 'GraphNode' })).message).toBe('neighbour true is not a node of the graph (1 to 1)');
  });

  it('Given a returned graph whose vals are not 1 to n, When it is read back, Then the row says the vals must be 1 to n', () => {
    const a = new GraphNode(11);
    const b = new GraphNode(12);
    a.neighbors = [b];
    b.neighbors = [a];
    expect(failure(() => readValue('GraphNode', a)).message).toBe('the returned graph must have the vals 1 to 2, each on one node (it has [11,12])');
  });

  it('Given a clone, When reachable is asked of the input and of the clone, Then a clone shares no node and the input itself shares all of them', () => {
    const { args } = buildArgs({ node: SQUARE }, { node: 'GraphNode' });
    const input = reachable(args[0]);
    const copy = buildArgs({ node: SQUARE }, { node: 'GraphNode' }).args[0];
    expect(input.size).toBe(4);
    expect([...reachable(copy)].some((node) => input.has(node))).toBe(false);
    expect([...reachable(args[0])].every((node) => input.has(node))).toBe(true);
  });

  it('Given a graph returned for a tree or list kind, When it is read back, Then a value of another shape is left as it is', () => {
    expect(readValue('TreeNode', 5)).toBe(5);
    expect(readValue('ListNode', 'x')).toBe('x');
    expect(readValue('GraphNode', 7)).toBe(7);
    expect(readValue(null, [1])).toEqual([1]);
  });
});

describe('the constructors a build uses', () => {
  it('Given the learner’s own ListNode, When buildArgs runs, Then the list is made of that class and not of the harness’s', () => {
    class MyList {
      next: MyList | null = null;
      constructor(public val: unknown) {}
    }
    const constructors: NodeConstructors = { ListNode: MyList as unknown as NodeConstructors['ListNode'], TreeNode, GraphNode };
    const { args } = buildArgs({ head: [1, 2] }, { head: 'ListNode' }, constructors);
    expect(args[0]).toBeInstanceOf(MyList);
    expect((args[0] as MyList).next).toBeInstanceOf(MyList);
  });

  it('Given the harness’s own classes, When a node is made with no arguments, Then it has LeetCode’s defaults', () => {
    expect({ ...new ListNode() }).toEqual({ val: 0, next: null });
    expect({ ...new TreeNode() }).toEqual({ val: 0, left: null, right: null });
    expect({ ...new GraphNode() }).toEqual({ val: 0, neighbors: [] });
  });
});

describe('arguments by name', () => {
  it('Given a harness-only key before an in-place parameter, When buildArgs runs, Then the parameter is found by name and not by position', () => {
    const { args, named } = buildArgs({ head: [1, 2], pos: -1, nums: [3, 1, 2] }, { head: 'ListNode', pos: 'ListNode_cycle_pos' });
    expect(args).toHaveLength(2);
    expect(named.nums).toBe(args[1]);
    expect(named.nums).toEqual([3, 1, 2]);
  });

  it('Given inputs, When buildArgs runs, Then the arguments come in the order of the case’s keys', () => {
    expect(buildArgs({ b: 1, a: 2, c: 3 }, {}).args).toEqual([1, 2, 3]);
  });
});
