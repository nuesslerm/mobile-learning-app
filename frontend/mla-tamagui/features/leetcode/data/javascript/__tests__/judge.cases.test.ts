import { describe, expect, it, vi } from 'vitest';

import type { TestCase } from '../../../domain/run/execution';
import type { HarnessConfig } from '../workerProtocol';
import { runCasesInWorker } from './support/nodeWorkerSpawn';

vi.setConfig({ testTimeout: 30_000 });

const config = (over: Partial<HarnessConfig> = {}): HarnessConfig => ({ methodName: 'twoSum', paramTypes: {}, returnType: null, inPlace: null, harness: null, ...over });
const plain = (inputs: Record<string, unknown>, expected: unknown, compareMode?: TestCase['compareMode']): TestCase => ({ inputs, expected, ...(compareMode ? { compareMode } : {}) });

describe('finding what to call', () => {
  const ADD_CLASS = 'class Solution {\n  constructor() { this.calls = 0; }\n  add(a, b) { this.calls += 1; return a + b + this.calls * 100; }\n}\n';

  it('Given a class Solution with the method, When two cases run, Then each case has a fresh instance', async () => {
    const run = await runCasesInWorker({ code: ADD_CLASS, config: config({ methodName: 'add' }), cases: [plain({ a: 1, b: 2 }, 103), plain({ a: 1, b: 2 }, 103)] });
    expect(run.rows.map((row) => [row.pass, row.actual, row.error])).toEqual([[true, 103, null], [true, 103, null]]);
  });

  it('Given only var twoSum = function, a function declaration or a const arrow, When a case runs, Then the function is called', async () => {
    const sources = [
      'var twoSum = function(nums, target) { return [nums.indexOf(target - nums[0]), 0]; };',
      'function twoSum(nums, target) { return [nums.indexOf(target - nums[0]), 0]; }',
      'const twoSum = (nums, target) => [nums.indexOf(target - nums[0]), 0];',
    ];
    for (const code of sources) {
      const run = await runCasesInWorker({ code, config: config(), cases: [plain({ nums: [2, 7], target: 9 }, [1, 0])] });
      expect(run.rows[0], code).toMatchObject({ pass: true, actual: [1, 0], error: null });
    }
  });

  it('Given a class Solution without the method and a function of the method’s name, When a case runs, Then the function is called', async () => {
    const code = 'class Solution { other() { return 0; } }\nvar twoSum = function() { return 5; };';
    const run = await runCasesInWorker({ code, config: config(), cases: [plain({}, 5)] });
    expect(run.rows[0]).toMatchObject({ pass: true, actual: 5 });
  });

  it('Given neither a class with the method nor a function, When the code loads, Then the run fails as No Solution class or twoSum function defined', async () => {
    const run = await runCasesInWorker({ code: 'var unrelated = 1;', config: config(), cases: [plain({}, 1)] });
    expect(run.loaded?.error).toBe('No Solution class or twoSum function defined');
    expect(run.rows).toEqual([]);
  });
});

describe('arguments and what a row shows', () => {
  it('Given a case whose keys are b then a, When it runs, Then the arguments are passed in key order', async () => {
    const run = await runCasesInWorker({ code: 'var twoSum = function(first, second) { return first - second; };', config: config(), cases: [plain({ b: 3, a: 10 }, -7)] });
    expect(run.rows[0]).toMatchObject({ pass: true, actual: -7 });
  });

  it('Given an in-place parameter, When the solution mutates it, Then the mutated value is the answer and the row’s input is the unmutated one', async () => {
    const code = 'var rotate = function(matrix) { matrix.reverse(); };';
    const run = await runCasesInWorker({ code, config: config({ methodName: 'rotate', inPlace: 'matrix' }), cases: [plain({ matrix: [[1, 2], [3, 4]] }, [[3, 4], [1, 2]])] });
    expect(run.rows[0]).toMatchObject({ pass: true, actual: [[3, 4], [1, 2]], input: { matrix: [[1, 2], [3, 4]] }, error: null });
  });

  it('Given an in-place parameter after a harness-only key, When it runs, Then the parameter is found by name', async () => {
    const code = 'var sortInPlace = function(head, nums) { nums.sort((a, b) => a - b); };';
    const run = await runCasesInWorker({
      code,
      config: config({ methodName: 'sortInPlace', paramTypes: { head: 'ListNode', pos: 'ListNode_cycle_pos' }, inPlace: 'nums' }),
      cases: [plain({ head: [1, 2], pos: -1, nums: [3, 1, 2] }, [1, 2, 3])],
    });
    expect(run.rows[0]).toMatchObject({ pass: true, actual: [1, 2, 3], error: null });
  });

  it('Given a compare mode, When the answer is the same items in another order, Then sorted passes and exact does not', async () => {
    const code = 'var twoSum = function() { return [1, 0]; };';
    const run = await runCasesInWorker({ code, config: config(), cases: [plain({}, [0, 1], 'sorted'), plain({}, [0, 1], 'exact')] });
    expect(run.rows.map((row) => row.pass)).toEqual([true, false]);
  });

  it('Given an answer of undefined, NaN or a Set, When a case runs, Then the row shows null, "NaN" and the Set’s String', async () => {
    const code = 'var twoSum = function(kind) { return kind === 1 ? undefined : kind === 2 ? NaN : new Set([1]); };';
    const run = await runCasesInWorker({ code, config: config(), cases: [plain({ kind: 1 }, null), plain({ kind: 2 }, 0), plain({ kind: 3 }, [1])] });
    expect(run.rows.map((row) => [row.pass, row.actual])).toEqual([[true, null], [false, 'NaN'], [false, '[object Set]']]);
  });

  it('Given a list, a tree and a graph, When a case runs with the catalog’s paramTypes, Then the harness builds the arguments and reads the answers back', async () => {
    const code = [
      'var reverseList = function(head) { let prev = null; while (head) { const next = head.next; head.next = prev; prev = head; head = next; } return prev; };',
      'var invertTree = function(root) { if (!root) return null; const left = invertTree(root.left); root.left = invertTree(root.right); root.right = left; return root; };',
      'var cloneGraph = function(node) { if (!node) return null; const copies = new Map([[node, new _Node(node.val)]]); const queue = [node]; while (queue.length) { const at = queue.shift(); for (const next of at.neighbors) { if (!copies.has(next)) { copies.set(next, new _Node(next.val)); queue.push(next); } copies.get(at).neighbors.push(copies.get(next)); } } return copies.get(node); };',
    ].join('\n');
    const list = await runCasesInWorker({ code, config: config({ methodName: 'reverseList', paramTypes: { head: 'ListNode' }, returnType: 'ListNode' }), cases: [plain({ head: [1, 2, 3] }, [3, 2, 1])] });
    const tree = await runCasesInWorker({ code, config: config({ methodName: 'invertTree', paramTypes: { root: 'TreeNode' }, returnType: 'TreeNode' }), cases: [plain({ root: [4, 2, 7, 1, 3, 6, 9] }, [4, 7, 2, 9, 6, 3, 1])] });
    const square = [[2, 4], [1, 3], [2, 4], [1, 3]];
    const graph = await runCasesInWorker({ code, config: config({ methodName: 'cloneGraph', paramTypes: { node: 'GraphNode' }, returnType: 'GraphNode' }), cases: [plain({ node: square }, square)] });
    expect([list.rows[0]?.pass, tree.rows[0]?.pass, graph.rows[0]?.pass]).toEqual([true, true, true]);
  });

  it('Given a clone that returns the node it was given, When it runs, Then the row is the shares-a-node error', async () => {
    const code = 'var cloneGraph = function(node) { return node; };';
    const square = [[2, 4], [1, 3], [2, 4], [1, 3]];
    const run = await runCasesInWorker({ code, config: config({ methodName: 'cloneGraph', paramTypes: { node: 'GraphNode' }, returnType: 'GraphNode' }), cases: [plain({ node: square }, square)] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'ValueError: the returned graph shares a node with the input: build copies' });
  });

  it('Given a pos beyond the list, When it runs, Then that row is a ValueError and the next case still runs', async () => {
    const code = 'var hasCycle = function(head) { let slow = head, fast = head; while (fast && fast.next) { slow = slow.next; fast = fast.next.next; if (slow === fast) return true; } return false; };';
    const run = await runCasesInWorker({
      code,
      config: config({ methodName: 'hasCycle', paramTypes: { head: 'ListNode', pos: 'ListNode_cycle_pos' } }),
      cases: [plain({ head: [1], pos: 5 }, true), plain({ head: [1, 2], pos: 1 }, true)],
    });
    expect(run.rows[0]?.error).toBe('ValueError: pos 5 is not -1 or an index of the list (1 nodes)');
    expect(run.rows[1]?.pass).toBe(true);
  });

  it('Given the learner’s own class ListNode, When a case runs, Then the harness builds with that class', async () => {
    const own = ['class ListNode {', '  constructor(val) { this.val = val; this.next = null; this.marker = "own"; }', '}', 'var firstMarker = function(head) { return head.marker; };'].join('\n');
    const fn = ['function ListNode(val, next) { this.val = val; this.next = next === undefined ? null : next; this.marker = "own"; }', 'var firstMarker = function(head) { return head.marker; };'].join('\n');
    for (const code of [own, fn]) {
      const run = await runCasesInWorker({ code, config: config({ methodName: 'firstMarker', paramTypes: { head: 'ListNode' } }), cases: [plain({ head: [1] }, 'own')] });
      expect(run.rows[0], code).toMatchObject({ pass: true, actual: 'own' });
    }
  });

  it('Given no definition of its own, When the learner’s code reads ListNode, Then it is the harness’s class with LeetCode’s defaults', async () => {
    const code = 'var shape = function() { const node = new ListNode(); return [node.val, node.next, typeof TreeNode, typeof _Node, typeof Node]; };';
    const run = await runCasesInWorker({ code, config: config({ methodName: 'shape' }), cases: [plain({}, [0, null, 'function', 'function', 'function'])] });
    expect(run.rows[0]).toMatchObject({ pass: true });
  });

  it('Given the loaded worker, When the learner’s code looks at its global scope, Then no harness name is there', async () => {
    const code = [
      'var probe = function() {',
      '  const symbols = Object.getOwnPropertySymbols(globalThis).map(String).filter((name) => name.includes("mla"));',
      '  return [typeof startHarness, typeof createCapture, typeof runCase, typeof resolveTarget, typeof composeWorkerScript, symbols.length];',
      '};',
    ].join('\n');
    const run = await runCasesInWorker({ code, config: config({ methodName: 'probe' }), cases: [plain({}, ['undefined', 'undefined', 'undefined', 'undefined', 'undefined', 0])] });
    expect(run.rows[0]).toMatchObject({ pass: true });
  });
});

describe('output', () => {
  it('Given prints at module level and in two cases, When they run, Then the module output comes with the load and each case has its own streams', async () => {
    const code = [
      'console.log("boot");',
      'var twoSum = function(n) { console.log("case", n); console.error("warn", n); process.stdout.write("raw" + n); return n; };',
    ].join('\n');
    const run = await runCasesInWorker({ code, config: config(), cases: [plain({ n: 1 }, 1), plain({ n: 2 }, 2)] });
    expect(run.loaded?.stdout).toBe('boot\n');
    expect(run.rows.map((row) => [row.stdout, row.stderr])).toEqual([['case 1\nraw1', 'warn 1\n'], ['case 2\nraw2', 'warn 2\n']]);
  });

  it('Given more output than the cap, When a case runs, Then the stream is cut with the marker', async () => {
    const code = 'var twoSum = function() { for (let i = 0; i < 1000; i += 1) console.log("0123456789"); return 0; };';
    const run = await runCasesInWorker({ code, config: config({ outputCap: 100 }), cases: [plain({}, 0)] });
    expect(run.rows[0]?.stdout.endsWith('\n...[output truncated]')).toBe(true);
    expect(run.rows[0]?.stdout.length).toBe(100 + '\n...[output truncated]'.length);
  });
});

describe('errors', () => {
  it('Given a syntax error on line 3, When the code loads, Then the worker reports a SyntaxError at the learner’s line 3', async () => {
    const run = await runCasesInWorker({ code: 'var a = 1;\nvar b = 2;\nvar c = ;\n', config: config(), cases: [] });
    expect(run.error?.message).toMatch(/^SyntaxError: /);
    expect(run.error?.at).toMatchObject({ line: 3 });
  });

  it('Given a syntax error on line 1 after the prelude, When the code loads, Then it is reported on line 1 and never at a column inside the prelude', async () => {
    const run = await runCasesInWorker({ code: 'var x = ;', config: config(), cases: [] });
    expect(run.error?.message).toMatch(/^SyntaxError: /);
    expect(run.error?.at?.line).toBe(1);
    expect(run.error?.at?.column ?? 1).toBeLessThanOrEqual('var x = ;'.length);
  });

  it('Given a throw at module level, When the code loads, Then the error is the ReferenceError with its line', async () => {
    const run = await runCasesInWorker({ code: 'var ok = 1;\nundefinedFunction();\n', config: config(), cases: [] });
    expect(run.error?.message).toBe('ReferenceError: undefinedFunction is not defined');
    expect(run.error?.at).toMatchObject({ line: 2 });
  });

  it('Given a throw inside a class method on line 4, When the case runs, Then the row has the error and the line from the stack', async () => {
    const code = 'class Solution {\n  twoSum(nums, target) {\n    if (nums.length === 3) {\n      throw new Error("boom");\n    }\n    return [0, 1];\n  }\n}\n';
    const run = await runCasesInWorker({ code, config: config(), cases: [plain({ nums: [1, 2, 3], target: 0 }, [0, 1]), plain({ nums: [1, 2], target: 0 }, [0, 1])] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'Error: boom', located: { line: 4 } });
    expect(run.rows[1]?.pass).toBe(true);
  });

  it('Given a TypeError in a function on line 2, When the case runs, Then the row names it and its line', async () => {
    const code = 'var twoSum = function(nums, target) {\n  return nums.missing.x;\n};\n';
    const run = await runCasesInWorker({ code, config: config(), cases: [plain({ nums: [1], target: 0 }, 0)] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: "TypeError: Cannot read properties of undefined (reading 'x')", located: { line: 2 } });
  });

  it('Given a throw on line 1, When the case runs, Then the line is 1 and the column is inside the learner’s text, not shifted by the prelude', async () => {
    const code = 'var twoSum = function() { return null.x; };';
    const run = await runCasesInWorker({ code, config: config(), cases: [plain({}, 0)] });
    const located = run.rows[0]?.located;
    expect(located?.line).toBe(1);
    expect(located?.column).toBeGreaterThan(0);
    expect(located?.column).toBeLessThanOrEqual(code.length);
  });

  it('Given infinite recursion, When the case runs, Then the row is a RangeError and the next case still runs', async () => {
    const code = 'var twoSum = function f(n) { return n === 0 ? 0 : f(n); };';
    const run = await runCasesInWorker({ code, config: config(), cases: [plain({ n: 1 }, 0), plain({ n: 0 }, 0)] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'RangeError: Maximum call stack size exceeded' });
    expect(run.rows[0]?.located?.line).toBe(1);
    expect(run.rows[1]?.pass).toBe(true);
  });

  it('Given a thrown string, When the case runs, Then the row’s error is the string and it has no location', async () => {
    const run = await runCasesInWorker({ code: 'var twoSum = function() { throw "text"; };', config: config(), cases: [plain({}, 0)] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'text' });
    expect(run.rows[0]?.located).toBeUndefined();
  });

  it('Given a thrown object whose own printing throws, When the case runs, Then the row is a row with <unprintable>', async () => {
    const run = await runCasesInWorker({ code: 'var twoSum = function() { throw { toString() { throw 1; } }; };', config: config(), cases: [plain({}, 0), plain({}, 0)] });
    expect(run.rows).toHaveLength(2);
    expect(run.rows[0]).toMatchObject({ pass: false, error: '<unprintable>' });
  });
});

describe('the design harness', () => {
  const TRIE_FUNCTION = [
    'var Trie = function() { this.words = new Set(); };',
    'Trie.prototype.insert = function(word) { this.words.add(word); };',
    'Trie.prototype.search = function(word) { return this.words.has(word); };',
    'Trie.prototype.startsWith = function(prefix) { return [...this.words].some((w) => w.startsWith(prefix)); };',
  ].join('\n');
  const TRIE_CLASS = 'class Trie {\n  constructor() { this.words = new Set(); }\n  insert(word) { this.words.add(word); }\n  search(word) { return this.words.has(word); }\n  startsWith(prefix) { return [...this.words].some((w) => w.startsWith(prefix)); }\n}\n';
  const harness = config({ methodName: '', harness: { kind: 'design', className: 'Trie' } });
  const trie = (operations: string[], args: unknown[][], expected: unknown[]): TestCase => ({ inputs: { operations, arguments: args }, expected });
  const EXAMPLE = trie(['Trie', 'insert', 'search', 'search', 'startsWith', 'insert', 'search'], [[], ['apple'], ['apple'], ['app'], ['app'], ['app'], ['app']], [null, null, true, false, true, null, true]);

  it('Given a correct Trie as a function with prototype methods or as a class, When LeetCode’s example runs, Then actual is [null,null,true,false,true,null,true] and it passes', async () => {
    for (const code of [TRIE_FUNCTION, TRIE_CLASS]) {
      const run = await runCasesInWorker({ code, config: harness, cases: [EXAMPLE] });
      expect(run.rows[0], code).toMatchObject({ pass: true, actual: [null, null, true, false, true, null, true], error: null, input: EXAMPLE.inputs });
    }
  });

  it('Given two cases on the same class, When the second runs, Then it starts from a new instance', async () => {
    const first = trie(['Trie', 'insert', 'search'], [[], ['apple'], ['apple']], [null, null, true]);
    const second = trie(['Trie', 'search'], [[], ['apple']], [null, false]);
    const run = await runCasesInWorker({ code: TRIE_FUNCTION, config: harness, cases: [first, second] });
    expect(run.rows.map((row) => row.pass)).toEqual([true, true]);
  });

  it('Given the starter left as is, When it runs, Then the case fails and actual shows the nulls', async () => {
    const starter = 'var Trie = function() {};\nTrie.prototype.insert = function(word) {};\nTrie.prototype.search = function(word) {};\nTrie.prototype.startsWith = function(prefix) {};\n';
    const run = await runCasesInWorker({ code: starter, config: harness, cases: [EXAMPLE] });
    expect(run.rows[0]).toMatchObject({ pass: false, actual: [null, null, null, null, null, null, null], error: null });
  });

  it('Given a class without the method an operation names, When it runs, Then the row error ends (operation 4: startsWith) and actual holds what ran before it', async () => {
    const code = TRIE_FUNCTION.replace(/\nTrie\.prototype\.startsWith.*$/, '');
    const run = await runCasesInWorker({ code, config: harness, cases: [EXAMPLE] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'TypeError: Trie.startsWith is not a function (operation 4: startsWith)', actual: [null, null, true, false] });
  });

  it('Given a method that throws in the learner’s code, When it runs, Then the row carries the error, the operation and the line, and the next case still runs', async () => {
    const code = TRIE_FUNCTION.replace('return this.words.has(word);', 'throw new Error(word);');
    const search = trie(['Trie', 'search'], [[], ['x']], [null, false]);
    const insertOnly = trie(['Trie', 'insert'], [[], ['x']], [null, null]);
    const run = await runCasesInWorker({ code, config: harness, cases: [search, insertOnly] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'Error: x (operation 1: search)', actual: [null], located: { line: 3 } });
    expect(run.rows[1]?.pass).toBe(true);
  });

  it('Given no class of the harness’s name, When the code loads, Then the run fails as No Trie class defined', async () => {
    const run = await runCasesInWorker({ code: 'class Solution {}', config: harness, cases: [EXAMPLE] });
    expect(run.loaded?.error).toBe('No Trie class defined');
  });

  it('Given a print inside an operation, When it runs, Then the text is that row’s stdout', async () => {
    const code = TRIE_FUNCTION.replace('this.words.add(word);', 'console.log("adding", word); this.words.add(word);');
    const quiet = trie(['Trie', 'search'], [[], ['x']], [null, false]);
    const run = await runCasesInWorker({ code, config: harness, cases: [EXAMPLE, quiet] });
    expect(run.rows.map((row) => row.stdout)).toEqual(['adding apple\nadding app\n', '']);
  });

  it('Given operations and arguments of different lengths, When it runs, Then that row is a ValueError and the run goes on', async () => {
    const uneven = trie(['Trie', 'insert', 'search'], [[], ['apple']], [null, null, true]);
    const run = await runCasesInWorker({ code: TRIE_FUNCTION, config: harness, cases: [uneven, EXAMPLE] });
    expect(run.rows[0]).toMatchObject({ pass: false, actual: [] });
    expect(run.rows[0]?.error).toMatch(/^ValueError: operations and arguments must have the same length/);
    expect(run.rows[1]?.pass).toBe(true);
  });

  it('Given a case with no operations key, When it runs, Then that row is a ValueError and the run goes on', async () => {
    const missing: TestCase = { inputs: { arguments: [[]] }, expected: [null] };
    const run = await runCasesInWorker({ code: TRIE_FUNCTION, config: harness, cases: [missing, EXAMPLE] });
    expect(run.rows[0]?.error).toMatch(/^ValueError: a design case needs/);
    expect(run.rows[1]?.pass).toBe(true);
  });

  it('Given a constructor that throws, When it runs, Then the row names operation 0', async () => {
    const code = TRIE_FUNCTION.replace('this.words = new Set();', 'this.words = undefinedName;');
    const run = await runCasesInWorker({ code, config: harness, cases: [EXAMPLE] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'ReferenceError: undefinedName is not defined (operation 0: Trie)', actual: [] });
  });
});

describe('the round-trip harness', () => {
  const STRING_CODEC = [
    'class Codec {',
    '  encode(strs) { return strs.map((s) => s.length + "#" + s).join(""); }',
    '  decode(s) { const out = []; let i = 0; while (i < s.length) { const j = s.indexOf("#", i); const n = Number(s.slice(i, j)); out.push(s.slice(j + 1, j + 1 + n)); i = j + 1 + n; } return out; }',
    '}',
  ].join('\n');
  const TREE_FUNCTIONS = [
    'var serialize = function(root) {',
    '  const out = [];',
    '  const walk = (node) => { if (!node) { out.push("#"); return; } out.push(String(node.val)); walk(node.left); walk(node.right); };',
    '  walk(root);',
    '  return out.join(",");',
    '};',
    'var deserialize = function(data) {',
    '  const values = data.split(",");',
    '  const build = () => { const value = values.shift(); if (value === "#") return null; const node = new TreeNode(Number(value)); node.left = build(); node.right = build(); return node; };',
    '  return build();',
    '};',
  ].join('\n');
  const strings = config({ methodName: '', harness: { kind: 'roundTrip', className: 'Codec', encode: 'encode', decode: 'decode' } });
  const trees = config({ methodName: '', paramTypes: { root: 'TreeNode' }, harness: { kind: 'roundTrip', className: 'Codec', encode: 'serialize', decode: 'deserialize' } });
  const echo = (strs: string[]): TestCase => ({ inputs: { strs }, expected: strs });
  const tree = (root: (number | null)[]): TestCase => ({ inputs: { root }, expected: root });

  it('Given a codec class with length-prefix encoding, When it runs on plain and unicode strings, Then every case passes', async () => {
    const lists = [['Hello', 'World'], [], [''], ['#'], ['3#abc'], ['héllo', '日本語', '😀#1']];
    const run = await runCasesInWorker({ code: STRING_CODEC, config: strings, cases: lists.map(echo) });
    expect(run.rows.map((row) => row.pass)).toEqual(lists.map(() => true));
    expect(run.rows[0]).toMatchObject({ actual: ['Hello', 'World'], input: { strs: ['Hello', 'World'] }, error: null });
  });

  it('Given an encode that returns the list itself, When it runs, Then the row is a TypeError naming string', async () => {
    const code = 'class Codec { encode(strs) { return strs; } decode(s) { return s; } }';
    const run = await runCasesInWorker({ code, config: strings, cases: [echo(['a'])] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'TypeError: encode must return a string, got object' });
  });

  it('Given an encode that keeps the input on this, When it runs, Then the case fails because decode runs on a new instance', async () => {
    const code = 'class Codec { encode(strs) { this.kept = strs; return "x"; } decode(s) { return this.kept; } }';
    const run = await runCasesInWorker({ code, config: strings, cases: [echo(['a', 'b'])] });
    expect(run.rows[0]?.pass).toBe(false);
    expect(run.rows[0]?.actual).toBeNull();
  });

  it('Given a tree codec written as the two top-level functions of the starter, When it runs, Then every case passes and the tree reads back as the canonical list', async () => {
    const lists = [[1, 2, 3, null, null, 4, 5], [], [1], [1, null, 2]];
    const run = await runCasesInWorker({ code: TREE_FUNCTIONS, config: trees, cases: lists.map(tree) });
    expect(run.rows.map((row) => row.pass)).toEqual([true, true, true, true]);
    expect(run.rows.map((row) => row.actual)).toEqual(lists);
  });

  it('Given a tree codec as a class, When it runs, Then it passes through new instances', async () => {
    const code = [
      'class Codec {',
      '  serialize(root) { return root === null ? "#" : root.val + "," + this.serialize(root.left) + "," + this.serialize(root.right); }',
      '  deserialize(data) {',
      '    const values = data.split(",");',
      '    const build = () => { const value = values.shift(); if (value === "#") return null; const node = new TreeNode(Number(value)); node.left = build(); node.right = build(); return node; };',
      '    return build();',
      '  }',
      '}',
    ].join('\n');
    const run = await runCasesInWorker({ code, config: trees, cases: [tree([1, 2, 3]), tree([])] });
    expect(run.rows.map((row) => [row.pass, row.actual])).toEqual([[true, [1, 2, 3]], [true, []]]);
  });

  it('Given a deserialize that returns the tree it was given through a global, When it runs, Then the row says the input tree was returned', async () => {
    const code = 'var kept = [];\nvar serialize = function(root) { kept.push(root); return "x"; };\nvar deserialize = function(data) { return kept[kept.length - 1]; };';
    const run = await runCasesInWorker({ code, config: trees, cases: [tree([1, 2, 3]), tree([])] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'ValueError: the returned tree is the input tree, not a decoded copy' });
    expect(run.rows[1]?.pass).toBe(true);
  });

  it('Given a deserialize that returns a new tree sharing a subtree with the input, When it runs, Then it is the same ValueError', async () => {
    const code = 'var kept = [];\nvar serialize = function(root) { kept.push(root); return "x"; };\nvar deserialize = function(data) { const last = kept[kept.length - 1]; return new TreeNode(last.val, last.left, last.right); };';
    const run = await runCasesInWorker({ code, config: trees, cases: [tree([1, 2, 3])] });
    expect(run.rows[0]?.error).toBe('ValueError: the returned tree is the input tree, not a decoded copy');
  });

  it('Given a deserialize that throws, When it runs, Then the row has the error and the line, and the next case still runs', async () => {
    const code = TREE_FUNCTIONS.replace('const values = data.split(",");', 'if (data.startsWith("2")) { throw new Error("boom"); }\n  const values = data.split(",");');
    const raiseLine = code.split('\n').findIndex((line) => line.includes('throw new Error("boom")')) + 1;
    const run = await runCasesInWorker({ code, config: trees, cases: [tree([2]), tree([1])] });
    expect(run.rows[0]).toMatchObject({ pass: false, error: 'Error: boom', located: { line: raiseLine } });
    expect(run.rows[1]?.pass).toBe(true);
  });

  it('Given neither the class nor the two functions, When the code loads, Then the run fails naming what is missing', async () => {
    const run = await runCasesInWorker({ code: 'var serialize = function() {};', config: trees, cases: [tree([1])] });
    expect(run.loaded?.error).toBe('No Codec class or serialize and deserialize functions defined');
  });

  it('Given a print inside encode, When it runs, Then the text is that row’s stdout', async () => {
    const code = STRING_CODEC.replace('encode(strs) {', 'encode(strs) { console.log("encoding", strs.length);');
    const run = await runCasesInWorker({ code, config: strings, cases: [echo(['a']), echo(['b', 'c'])] });
    expect(run.rows.map((row) => row.stdout)).toEqual(['encoding 1\n', 'encoding 2\n']);
  });
});
