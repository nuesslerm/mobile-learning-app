import { clipTo, outline, SYNTAX_TABLES, type Range, type SyntaxNode, type TreeValue } from '@lib/code-editor-api';
import { parseFor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { PYTHON_BLOCK_KINDS } from '../blockKinds';
import { buildBlockModel, MAX_ROWS, placeOf, type BlockModel, type BlockRow, type BlockSlot } from '../blockModel';

const DEF_IF = ['def f(x):', '    if x:', '        a = 1', '        b = 2', '    return b', ''].join('\n');

function treeOf(text: string, over: Partial<TreeValue> = {}): TreeValue {
  return { instance: 'page-1', revision: 7, language: 'python', length: text.length, parsedTo: text.length, complete: true, detail: 'outline', root: outline('python', parseFor(text, 'python')), ...over };
}

const modelOf = (text: string, over: Partial<TreeValue> = {}): BlockModel => buildBlockModel({ text, tree: treeOf(text, over) });
const node = (type: string, from: number, to: number, children: readonly SyntaxNode[] = []): SyntaxNode => ({ type, from, to, ...(children.length > 0 ? { children } : {}) });
const handBuilt = (text: string, children: readonly SyntaxNode[]): BlockModel => buildBlockModel({ text, tree: { ...treeOf(text), root: node('Script', 0, text.length, children) } });

const labels = (model: BlockModel) => model.rows.map((row) => row.label);
const kinds = (model: BlockModel) => model.rows.map((row) => row.kind);
const depths = (model: BlockModel) => model.rows.map((row) => row.depth);
const rowFor = (model: BlockModel, line: string): BlockRow => {
  const row = model.rows.find((one) => one.label === line);
  if (!row) throw new Error(`no row for ${line}`);
  return row;
};
const rangeOf = (text: string, snippet: string): Range => ({ from: text.indexOf(snippet), to: text.indexOf(snippet) + snippet.length });
const after = (slot: BlockSlot): Range | null => ('after' in slot.destination ? slot.destination.after : null);
const validSlots = (model: BlockModel) => model.slots.filter((slot) => slot.valid);

describe('the block model of a Python document', () => {
  it('given a def holding an if and a return, when it is built, then the rows are the statements with their depths and every one is movable', () => {
    const model = modelOf(DEF_IF);

    expect(kinds(model)).toEqual(['def', 'if', 'statement', 'statement', 'return']);
    expect(depths(model)).toEqual([0, 1, 2, 2, 1]);
    expect(model.rows.every((row) => row.movable)).toBe(true);
    expect(model.rows.map((row) => DEF_IF.slice(row.range.from, row.range.to))).toEqual([DEF_IF.trimEnd(), 'if x:\n        a = 1\n        b = 2', 'a = 1', 'b = 2', 'return b']);
    expect(rowFor(model, 'if x:').subtreeEnd).toBe(4);
    expect(rowFor(model, 'def f(x):').subtreeEnd).toBe(model.rows.length);
    expect(model.rows.map((row) => row.index)).toEqual([0, 1, 2, 3, 4]);
  });

  it('given a tree reply, when the model is built, then it carries the reply\'s instance, revision and completeness and the text it was built on', () => {
    const model = modelOf(DEF_IF);

    expect([model.instance, model.revision, model.complete, model.text, model.unreadFrom]).toEqual(['page-1', 7, true, DEF_IF, null]);
  });

  it('given an if with an else, when it is built, then the else line is a clause row at the if\'s depth between the two bodies and each block has its own slots', () => {
    const text = ['def f(x):', '    if x:', '        a = 1', '    else:', '        b = 2', '    return b', ''].join('\n');

    const model = modelOf(text);

    expect(labels(model)).toEqual(['def f(x):', 'if x:', 'a = 1', 'else:', 'b = 2', 'return b']);
    expect(kinds(model)).toEqual(['def', 'if', 'statement', 'clause', 'statement', 'return']);
    expect(depths(model)).toEqual([0, 1, 2, 1, 2, 1]);
    expect(rowFor(model, 'else:').movable).toBe(false);
    expect(rowFor(model, 'if x:').subtreeEnd).toBe(5);
    const ifRow = rowFor(model, 'if x:');
    const inIf = model.slots.filter((slot) => slot.owner === ifRow.id);
    expect(inIf.map((slot) => [slot.block, slot.position, slot.gap])).toEqual([
      [0, 0, 2],
      [0, 1, 3],
      [1, 0, 4],
      [1, 1, 5],
    ]);
    expect(inIf.map((slot) => slot.destination)).toEqual([
      { into: ifRow.range, at: 'start', block: 0 },
      { after: rangeOf(text, 'a = 1') },
      { into: ifRow.range, at: 'start', block: 1 },
      { after: rangeOf(text, 'b = 2') },
    ]);
    expect(inIf.every((slot) => slot.valid)).toBe(true);
  });

  it('given the slots of the def-and-if example, when they are read, then each sits at the row boundary after its reference and at the depth of the block it is in', () => {
    const model = modelOf(DEF_IF);
    const ifRow = rowFor(model, 'if x:');
    const defRow = rowFor(model, 'def f(x):');

    expect(model.slots.map((slot) => [slot.gap, slot.depth, slot.valid])).toEqual([
      [0, 0, true],
      [1, 1, true],
      [2, 2, true],
      [3, 2, true],
      [4, 1, true],
      [4, 2, true],
      [5, 0, true],
      [5, 1, true],
    ]);
    expect(model.slots[0]?.destination).toEqual({ before: defRow.range });
    expect(model.slots[1]?.destination).toEqual({ into: defRow.range, at: 'start', block: 0 });
    expect(after(model.slots[4] as BlockSlot)).toEqual(ifRow.range);
  });

  it('given a half-typed for header between statements, when it is built, then that line is a text row, its siblings stay movable and no valid slot has it as the reference', () => {
    const text = ['def f(x):', '    a = 1', '    for x in', '    b = 2', '    return b', ''].join('\n');

    const model = modelOf(text);

    const typed = rowFor(model, 'for x in');
    expect([typed.kind, typed.movable]).toEqual(['text', false]);
    expect(model.rows.filter((row) => row.depth === typed.depth).map((row) => [row.label, row.movable])).toEqual([
      ['a = 1', true],
      ['for x in', false],
      ['b = 2', true],
      ['return b', true],
    ]);
    expect(rowFor(model, 'def f(x):').movable).toBe(false);
    const withTyped = model.slots.filter((slot) => after(slot)?.from === typed.range.from);
    expect(withTyped.length).toBe(1);
    expect(withTyped.every((slot) => !slot.valid)).toBe(true);
    expect(validSlots(model).some((slot) => after(slot)?.from === typed.range.from)).toBe(false);
  });

  it('given the untouched starter whose body is only whitespace, when it is built, then the class and the def are header rows that cannot move and the def\'s block has no valid slot', () => {
    const text = 'class Solution:\n    def twoSum(self, nums: List[int], target: int) -> List[int]:\n        ';

    const model = modelOf(text);

    expect(kinds(model)).toEqual(['class', 'def']);
    expect(model.rows.map((row) => row.movable)).toEqual([false, false]);
    const defRow = rowFor(model, 'def twoSum(self, nums: List[int], target: int) -> List[int]:');
    const own = model.slots.filter((slot) => slot.owner === defRow.id);
    expect(own.length).toBe(1);
    expect(own[0]?.destination).toEqual({ into: defRow.range, at: 'start', block: 0 });
    expect(validSlots(model)).toEqual([]);
  });

  it('given two statements on one line and a one-line if, when they are built, then the statements that share a line are text rows and the if stays one movable row', () => {
    const pair = 'a = 1; b = 2\nreturn 1\n';
    const pairModel = handBuilt(pair, [node('AssignStatement', 0, 5), node('AssignStatement', 7, 12), node('ReturnStatement', 13, 21)]);
    const oneLine = 'if x: return 1\n';
    const oneLineModel = handBuilt(oneLine, [node('IfStatement', 0, 14, [node('Body', 4, 14, [node('ReturnStatement', 6, 14)])])]);

    expect(kinds(pairModel)).toEqual(['text', 'text', 'return']);
    expect(pairModel.rows.map((row) => row.movable)).toEqual([false, false, true]);
    expect(kinds(oneLineModel)).toEqual(['if', 'text']);
    expect(oneLineModel.rows.map((row) => row.movable)).toEqual([true, false]);
    expect(oneLineModel.slots.filter((slot) => 'into' in slot.destination).every((slot) => !slot.valid)).toBe(true);
  });

  it('given a decorated function, when it is built, then it is one leaf row that moves as a whole', () => {
    const text = '@cache\ndef f():\n    return 1\n';
    const decorated = node('DecoratedStatement', 0, 29, [node('FunctionDefinition', 7, 29, [node('Body', 14, 29, [node('ReturnStatement', 20, 28)])])]);

    const model = handBuilt(text, [decorated]);

    expect(model.rows.map((row) => [row.kind, row.depth, row.movable, row.label])).toEqual([['statement', 0, true, '@cache']]);
    expect(text.slice(model.rows[0]?.range.from, model.rows[0]?.range.to)).toBe('@cache\ndef f():\n    return 1');
  });

  it('given a parse that stopped short of the text, when it is built, then everything from there on is one text row and no slot past it is valid', () => {
    const text = 'a = 1\nb = 2\nc = 3\n';
    const parsedTo = 10;
    const tree = treeOf(text, { complete: false, parsedTo });
    const clipped = { ...tree, root: clipTo(tree.root as SyntaxNode, parsedTo) };

    const model = buildBlockModel({ text, tree: clipped });

    expect(model.complete).toBe(false);
    expect(model.unreadFrom).toBe(text.indexOf('b = 2'));
    expect(model.rows.map((row) => [row.kind, row.label])).toEqual([
      ['statement', 'a = 1'],
      ['text', 'b = 2'],
    ]);
    expect(model.rows[1]?.range).toEqual({ from: text.indexOf('b = 2'), to: text.indexOf('c = 3') + 'c = 3'.length });
    expect(validSlots(model).map((slot) => slot.gap)).toEqual([0]);
  });

  it('given a parse that reached the end, when it is built, then there is no unread text and the slot after the last statement is valid', () => {
    const model = modelOf('a = 1\nb = 2\n');

    expect(model.unreadFrom).toBeNull();
    expect(validSlots(model).map((slot) => slot.gap)).toEqual([0, 1, 2]);
  });

  it('given comment lines above a statement, when it is built, then no comment row appears', () => {
    const text = ['def f():', '    # note', '    # more', '    return 1', ''].join('\n');

    const model = modelOf(text);

    expect(labels(model)).toEqual(['def f():', 'return 1']);
  });

  it('given class Solution with a def that has a body, when it is built, then the depths are 0, 1 and 2 and the ids are paths', () => {
    const text = ['class Solution:', '    def twoSum(self, nums):', '        seen = {}', '        return seen', ''].join('\n');

    const model = modelOf(text);

    expect(depths(model)).toEqual([0, 1, 2, 2]);
    expect(kinds(model)).toEqual(['class', 'def', 'statement', 'return']);
    expect(model.rows.map((row) => row.id)).toEqual(['0', '0.0.0', '0.0.0.0.0', '0.0.0.0.1']);
    expect(model.rows.map((row) => row.movable)).toEqual([true, false, true, true]);
  });

  it('given a block that holds one statement, when it is built, then that statement cannot be dragged out (the block would be left empty) but a slot after it is valid', () => {
    const text = ['def f(x):', '    if x:', '        a = 1', '    return a', ''].join('\n');

    const model = modelOf(text);

    expect(rowFor(model, 'a = 1').movable).toBe(false);
    expect(rowFor(model, 'if x:').movable).toBe(true);
    expect(model.slots.filter((slot) => after(slot)?.from === rowFor(model, 'a = 1').range.from).map((slot) => slot.valid)).toEqual([true]);
  });

  it('given forty statements, when it is built, then the cap keeps the first rows and one text row says how many more there are', () => {
    const text = `${Array.from({ length: 40 }, (_, n) => `x${n} = ${n}`).join('\n')}\n`;

    const model = modelOf(text);

    expect(model.rows.length).toBe(MAX_ROWS + 1);
    const last = model.rows[MAX_ROWS] as BlockRow;
    expect([last.kind, last.movable, last.label]).toEqual(['text', false, `… ${40 - MAX_ROWS} more rows`]);
    expect(model.rows[MAX_ROWS - 1]?.label).toBe(`x${MAX_ROWS - 1} = ${MAX_ROWS - 1}`);
    expect(Math.max(...model.slots.map((slot) => slot.gap))).toBe(MAX_ROWS);
    expect(model.slots.some((slot) => after(slot)?.from === rangeOf(text, `x${MAX_ROWS - 1} = `).from)).toBe(true);
  });

  it('given a compound that runs past the cap, when it is built, then its subtree ends at the cap and the more row is not inside it', () => {
    const body = Array.from({ length: 35 }, (_, n) => `    x${n} = ${n}`).join('\n');
    const text = `def f():\n${body}\n`;

    const model = modelOf(text);

    expect(model.rows[0]?.subtreeEnd).toBe(MAX_ROWS);
    expect(model.rows[MAX_ROWS]?.id).toBe('more');
  });

  it('given a JavaScript tree, when it is built, then there are no rows and no slots', () => {
    const text = 'let a = 1;\n';

    const model = buildBlockModel({ text, tree: { ...treeOf(text), language: 'javascript' } });

    expect([model.rows, model.slots]).toEqual([[], []]);
  });

  it('given a document with no statement, when it is built, then there are no rows and no slots', () => {
    expect(modelOf('')).toMatchObject({ rows: [], slots: [] });
    expect(modelOf('# only a note\n')).toMatchObject({ rows: [], slots: [] });
  });

  it('given a row id, when its place is read, then it names the owner, the block of the owner and the position', () => {
    expect(placeOf('3')).toEqual({ owner: null, block: 0, position: 3 });
    expect(placeOf('3.1.2')).toEqual({ owner: '3', block: 1, position: 2 });
    expect(placeOf('3.1.2.0.4')).toEqual({ owner: '3.1.2', block: 0, position: 4 });
  });

  it('given the kind table, when it is checked against the library, then every name is a Python statement the library classifies', () => {
    const known = new Set([...SYNTAX_TABLES.python.compound, ...SYNTAX_TABLES.python.simple]);

    for (const name of PYTHON_BLOCK_KINDS.keys()) expect(known.has(name)).toBe(true);
  });
});
