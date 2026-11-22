import type { NodeType } from '../../../domain/models';

export const MAX_NODES = 1_000_000;

export class ValueError extends Error {
  override name = 'ValueError';
}

/** LeetCode's JavaScript definitions: the harness provides them as globals, and a definition of the same name in the learner's code replaces them. */
export class ListNode {
  val: unknown;
  next: ListNode | null;
  constructor(val?: unknown, next?: ListNode | null) {
    this.val = val === undefined ? 0 : val;
    this.next = next === undefined ? null : next;
  }
}

export class TreeNode {
  val: unknown;
  left: TreeNode | null;
  right: TreeNode | null;
  constructor(val?: unknown, left?: TreeNode | null, right?: TreeNode | null) {
    this.val = val === undefined ? 0 : val;
    this.left = left === undefined ? null : left;
    this.right = right === undefined ? null : right;
  }
}

export class GraphNode {
  val: unknown;
  neighbors: GraphNode[];
  constructor(val?: unknown, neighbors?: GraphNode[]) {
    this.val = val === undefined ? 0 : val;
    this.neighbors = neighbors === undefined ? [] : neighbors;
  }
}

type Linked = { val: unknown; next: Linked | null | undefined };
type Branched = { val: unknown; left?: Branched | null; right?: Branched | null };
type Connected = { val: unknown; neighbors: Connected[] };

/** The constructors a build uses: the learner's own where it defined one, else the harness's. */
export type NodeConstructors = {
  ListNode: new (val?: unknown) => Linked;
  TreeNode: new (val?: unknown) => Branched;
  GraphNode: new (val?: unknown) => Connected;
};
export const DEFAULT_CONSTRUCTORS: NodeConstructors = { ListNode, TreeNode, GraphNode };

const show = (value: unknown): string => JSON.stringify(value) ?? String(value);
const isObject = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null;
const has = (value: unknown, key: string): boolean => isObject(value) && key in value;

function buildList(Node: NodeConstructors['ListNode'], values: unknown[]): Linked | null {
  let head: Linked | null = null;
  let tail: Linked | null = null;
  for (const value of values) {
    const node = new Node(value);
    if (tail === null) head = node;
    else tail.next = node;
    tail = node;
  }
  return head;
}

function buildTree(Node: NodeConstructors['TreeNode'], values: unknown[]): Branched | null {
  if (!values || values.length === 0 || values[0] === null) return null;
  const root = new Node(values[0]);
  const queue: Branched[] = [root];
  let at = 0;
  let i = 1;
  while (at < queue.length && i < values.length) {
    const node = queue[at++]!;
    if (values[i] !== null) {
      node.left = new Node(values[i]);
      queue.push(node.left);
    }
    i += 1;
    if (i < values.length && values[i] !== null) {
      node.right = new Node(values[i]);
      queue.push(node.right);
    }
    i += 1;
  }
  return root;
}

function buildGraph(Node: NodeConstructors['GraphNode'], adjacency: unknown[]): Connected | null {
  const nodes = adjacency.map((_, i) => new Node(i + 1));
  adjacency.forEach((neighbours, i) => {
    for (const val of neighbours as unknown[]) {
      if (!Number.isInteger(val) || (val as number) < 1 || (val as number) > nodes.length) {
        throw new ValueError(`neighbour ${show(val)} is not a node of the graph (1 to ${nodes.length})`);
      }
    }
    nodes[i]!.neighbors = (neighbours as number[]).map((val) => nodes[val - 1]!);
  });
  return nodes[0] ?? null;
}

/** Links the tail of the list to the node at `pos` (-1 leaves it open). */
export function linkCycle(head: Linked | null, pos: unknown): void {
  const nodes: Linked[] = [];
  for (let at = head; at; at = at.next ?? null) nodes.push(at);
  if (!Number.isInteger(pos) || (pos as number) < -1 || (pos as number) >= nodes.length) {
    throw new ValueError(`pos ${show(pos)} is not -1 or an index of the list (${nodes.length} nodes)`);
  }
  if ((pos as number) >= 0) nodes[nodes.length - 1]!.next = nodes[pos as number]!;
}

export function findByVal(root: Branched | null | undefined, val: unknown): Branched {
  const queue: (Branched | null | undefined)[] = [root];
  for (let at = 0; at < queue.length; at += 1) {
    const node = queue[at];
    if (node === null || node === undefined) continue;
    if (node.val === val) return node;
    queue.push(node.left, node.right);
  }
  throw new ValueError(`no node with value ${show(val)} in the tree`);
}

/** The arguments in the order of the case's keys, and by name: a harness-only key (`ListNode_cycle_pos`) is no argument. */
export function buildArgs(
  inputs: Record<string, unknown>,
  paramTypes: Record<string, NodeType>,
  constructors: NodeConstructors = DEFAULT_CONSTRUCTORS
): { args: unknown[]; named: Record<string, unknown> } {
  const args: unknown[] = [];
  const named: Record<string, unknown> = {};
  let root: Branched | null | undefined;
  let listed = false;
  let lastList: Linked | null = null;
  for (const name of Object.keys(inputs)) {
    let value: unknown = inputs[name];
    const kind = paramTypes[name];
    if (kind === 'ListNode_cycle_pos') {
      if (!listed) throw new ValueError(`'${name}' links the list before it, and no ListNode parameter comes before it`);
      linkCycle(lastList, value);
      continue;
    }
    if (kind === 'ListNode') {
      lastList = buildList(constructors.ListNode, value as unknown[]);
      listed = true;
      value = lastList;
    } else if (kind === 'ListNode[]') {
      value = (value as unknown[][]).map((values) => buildList(constructors.ListNode, values));
    } else if (kind === 'TreeNode') {
      value = buildTree(constructors.TreeNode, value as unknown[]);
      root = root ?? (value as Branched | null);
    } else if (kind === 'TreeNode_by_val') {
      value = findByVal(root, value);
    } else if (kind === 'GraphNode') {
      value = buildGraph(constructors.GraphNode, value as unknown[]);
    }
    args.push(value);
    named[name] = value;
  }
  return { args, named };
}

function listValues(head: Linked | null | undefined): unknown[] {
  const out: unknown[] = [];
  const seen = new Set<Linked>();
  for (let at = head; at !== null && at !== undefined; at = at.next) {
    if (seen.has(at) || out.length >= MAX_NODES) throw new ValueError('the returned linked list has a cycle');
    seen.add(at);
    out.push(at.val);
  }
  return out;
}

function treeValues(root: Branched | null | undefined): unknown[] {
  const out: unknown[] = [];
  const seen = new Set<Branched>();
  const queue: (Branched | null | undefined)[] = [root];
  for (let at = 0; at < queue.length; at += 1) {
    const node = queue[at];
    if (node === null || node === undefined) {
      out.push(null);
      continue;
    }
    if (seen.has(node) || out.length >= MAX_NODES) throw new ValueError('the returned tree is not a tree');
    seen.add(node);
    out.push(node.val);
    queue.push(node.left, node.right);
  }
  while (out.length > 0 && out[out.length - 1] === null) out.pop();
  return out;
}

function graphValues(start: Connected): number[][] {
  const nodes: Connected[] = [];
  const seen = new Set<Connected>();
  const queue: Connected[] = [start];
  for (let at = 0; at < queue.length; at += 1) {
    const node = queue[at]!;
    if (seen.has(node)) continue;
    if (nodes.length >= MAX_NODES) throw new ValueError('the returned graph is too large');
    seen.add(node);
    nodes.push(node);
    for (const neighbour of node.neighbors) queue.push(neighbour);
  }
  const vals = nodes.map((node) => node.val);
  const numbered = vals.every((val) => Number.isInteger(val));
  if (!numbered || [...(vals as number[])].sort((a, b) => a - b).some((val, i) => val !== i + 1)) {
    throw new ValueError(`the returned graph must have the vals 1 to ${nodes.length}, each on one node (it has ${show(vals)})`);
  }
  const byVal = new Map(nodes.map((node) => [node.val as number, node]));
  return Array.from({ length: nodes.length }, (_, i) => byVal.get(i + 1)!.neighbors.map((neighbour) => neighbour.val as number));
}

/** A returned object read back into the JSON form the catalog's expected values use. */
export function readValue(kind: NodeType | null | undefined, value: unknown): unknown {
  if (kind === 'ListNode' && (value === null || value === undefined || has(value, 'next'))) return listValues(value as Linked | null | undefined);
  if (kind === 'GraphNode' && (value === null || value === undefined || has(value, 'neighbors'))) {
    return value === null || value === undefined ? [] : graphValues(value as Connected);
  }
  if (kind === 'TreeNode' && (value === null || value === undefined || has(value, 'left'))) return treeValues(value as Branched | null | undefined);
  if (kind === 'TreeNode_val' && value !== null && value !== undefined && has(value, 'val')) return (value as { val: unknown }).val;
  return value;
}

/** Every node a value reaches through `left`, `right` and `neighbors`: how the harness tells a returned structure from the one it built. */
export function reachable(root: unknown): Set<unknown> {
  const seen = new Set<unknown>();
  const stack: unknown[] = [root];
  while (stack.length > 0) {
    const node = stack.pop();
    if (!isObject(node) || seen.has(node)) continue;
    seen.add(node);
    stack.push(node.left, node.right);
    if (Array.isArray(node.neighbors)) for (const neighbour of node.neighbors as unknown[]) stack.push(neighbour);
  }
  return seen;
}
