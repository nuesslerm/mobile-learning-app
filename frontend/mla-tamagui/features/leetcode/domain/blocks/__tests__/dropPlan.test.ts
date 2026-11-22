import { outline, type Range, type TreeValue } from '@lib/code-editor-api';
import { parseFor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { depthX, gapY, INDENT_STEP, listHeight, ROW_HEIGHT } from '../blockLayout';
import { buildBlockModel, type BlockModel, type BlockRow } from '../blockModel';
import { INSERT_LABEL, labelOf, MOVE_LABEL, stepOf } from '../blockStep';
import { dropSlots, dropText, planDrop, type DragSource, type DropPlan } from '../dropPlan';
import { PYTHON_PALETTE, type PaletteItemId } from '../palette';

const DEF_IF = ['def f(x):', '    if x:', '        a = 1', '        b = 2', '    return b', ''].join('\n');
const DEF_IF_ELSE = ['def f(x):', '    if x:', '        a = 1', '    else:', '        b = 2', '    return b', ''].join('\n');
const HALF_TYPED = ['def f(x):', '    a = 1', '    for x in', '    b = 2', '    return b', ''].join('\n');

function modelOf(text: string): BlockModel {
  const tree: TreeValue = { instance: 'page-1', revision: 3, language: 'python', length: text.length, parsedTo: text.length, complete: true, detail: 'outline', root: outline('python', parseFor(text, 'python')) };
  return buildBlockModel({ text, tree });
}

const rowFor = (model: BlockModel, label: string): BlockRow => {
  const row = model.rows.find((one) => one.label === label);
  if (!row) throw new Error(`no row for ${label}`);
  return row;
};
const block = (model: BlockModel, label: string): DragSource => ({ kind: 'block', id: rowFor(model, label).id });
const chip = (item: PaletteItemId = 'if'): DragSource => ({ kind: 'palette', item });
const at = (gap: number, depth: number) => ({ x: depthX(depth), y: gapY(gap) });
const textOf = (model: BlockModel, range: Range) => model.text.slice(range.from, range.to);

describe('planDrop: a pointer in the list becomes a slot', () => {
  it('given a pointer a little above and a little below the middle of a row, when it is planned, then the nearer row boundary is the gap', () => {
    const model = modelOf(DEF_IF);
    const middle = gapY(2) + ROW_HEIGHT / 2;

    const above = planDrop(model, chip(), { x: depthX(2), y: middle - 1 });
    const below = planDrop(model, chip(), { x: depthX(2), y: middle });

    expect(above?.slot.gap).toBe(2);
    expect(below?.slot.gap).toBe(3);
  });

  it('given a pointer above the first row and below the last, when it is planned, then it is clamped to the first and the last boundary', () => {
    const model = modelOf(DEF_IF);

    const top = planDrop(model, chip(), { x: depthX(0), y: -3 * ROW_HEIGHT });
    const bottom = planDrop(model, chip(), { x: depthX(0), y: listHeight(model.rows.length) + 3 * ROW_HEIGHT });

    expect(top?.slot.gap).toBe(0);
    expect(bottom?.slot.gap).toBe(model.rows.length);
  });

  it('given b = 2 dragged to the boundary after the if, when the pointer is at depth 1 and at depth 2, then both give the slot after the if because the depth-2 slot would leave b where it is', () => {
    const model = modelOf(DEF_IF);
    const ifRow = rowFor(model, 'if x:');
    const gap = ifRow.subtreeEnd;

    const shallow = planDrop(model, block(model, 'b = 2'), at(gap, 1));
    const deep = planDrop(model, block(model, 'b = 2'), at(gap, 2));

    expect(shallow?.slot.destination).toEqual({ after: ifRow.range });
    expect(deep?.slot.destination).toEqual({ after: ifRow.range });
  });

  it('given b = 2 dragged to the end of the list, when the pointer is at depth 0 and at depth 1, then the slots are after the def and after the return', () => {
    const model = modelOf(DEF_IF);
    const end = model.rows.length;

    const outer = planDrop(model, block(model, 'b = 2'), at(end, 0));
    const inner = planDrop(model, block(model, 'b = 2'), at(end, 1));

    expect(outer?.slot.destination).toEqual({ after: rowFor(model, 'def f(x):').range });
    expect(inner?.slot.destination).toEqual({ after: rowFor(model, 'return b').range });
  });

  it('given return b dragged to the first slot of the if body, when it is planned, then the destination is the start of the if\'s block', () => {
    const model = modelOf(DEF_IF);
    const ifRow = rowFor(model, 'if x:');

    const plan = planDrop(model, block(model, 'return b'), at(ifRow.index + 1, 2));

    expect(plan?.slot.destination).toEqual({ into: ifRow.range, at: 'start', block: 0 });
  });

  it('given return b dragged above the first row, when it is planned, then the destination is before the first statement of the document', () => {
    const model = modelOf(DEF_IF);

    const plan = planDrop(model, block(model, 'return b'), at(0, 0));

    expect(plan?.slot.destination).toEqual({ before: rowFor(model, 'def f(x):').range });
  });

  it('given the if dragged over its own body, when it is planned, then there is no plan: a node cannot move into itself', () => {
    const model = modelOf(DEF_IF);
    const ifRow = rowFor(model, 'if x:');

    const overBody = [ifRow.index + 1, ifRow.index + 2].map((gap) => planDrop(model, block(model, 'if x:'), at(gap, 2)));

    expect(overBody).toEqual([null, null]);
  });

  it('given the if dragged to the boundaries just above and just below itself, when it is planned, then there is no plan: it would stay where it is', () => {
    const model = modelOf(DEF_IF);
    const ifRow = rowFor(model, 'if x:');

    const above = planDrop(model, block(model, 'if x:'), at(ifRow.index, 1));
    const below = planDrop(model, block(model, 'if x:'), at(ifRow.subtreeEnd, 1));

    expect([above, below]).toEqual([null, null]);
  });

  it('given a statement beside a half-typed line, when a chip is dropped at the boundary after the half-typed line, then there is no plan, and at the boundary before it there is one', () => {
    const model = modelOf(HALF_TYPED);
    const typed = rowFor(model, 'for x in');

    const after = planDrop(model, chip(), at(typed.index + 1, 1));
    const before = planDrop(model, chip(), at(typed.index, 1));

    expect(after).toBeNull();
    expect(before?.slot.destination).toEqual({ after: rowFor(model, 'a = 1').range });
  });

  it('given a pointer halfway between two depths, when it is planned, then the shallower slot wins', () => {
    const model = modelOf(DEF_IF);
    const gap = rowFor(model, 'if x:').subtreeEnd;

    const plan = planDrop(model, chip(), { x: depthX(1) + INDENT_STEP / 2, y: gapY(gap) });

    expect(plan?.slot.depth).toBe(1);
  });

  it('given a pointer far to the right, when it is planned, then the deepest slot at the boundary wins', () => {
    const model = modelOf(DEF_IF);
    const gap = rowFor(model, 'if x:').subtreeEnd;

    const plan = planDrop(model, chip(), { x: depthX(9), y: gapY(gap) });

    expect(plan?.slot.depth).toBe(2);
  });

  it('given a block that cannot move or does not exist, when its slots are asked, then there are none', () => {
    const model = modelOf(HALF_TYPED);
    const typed = rowFor(model, 'for x in');
    const elseModel = modelOf(DEF_IF_ELSE);

    expect(dropSlots(model, { kind: 'block', id: typed.id })).toEqual([]);
    expect(dropSlots(model, { kind: 'block', id: 'nowhere' })).toEqual([]);
    expect(dropSlots(elseModel, { kind: 'block', id: rowFor(elseModel, 'else:').id })).toEqual([]);
  });

  it('given every row of the def-and-if example, when each is dragged to every offered slot, then no slot is inside the row and none leaves it in place', () => {
    const model = modelOf(DEF_IF);

    for (const row of model.rows) {
      const slots = dropSlots(model, { kind: 'block', id: row.id });
      for (const slot of slots) {
        expect(slot.valid).toBe(true);
        expect(slot.owner === row.id || slot.owner?.startsWith(`${row.id}.`)).toBeFalsy();
      }
    }
    // b = 2 is the second statement of the if's block: the slots before it (gap 3) and after it (gap 4, depth 2) leave it in place
    expect(dropSlots(model, block(model, 'b = 2')).map((slot) => [slot.gap, slot.depth])).toEqual([
      [0, 0],
      [1, 1],
      [2, 2],
      [4, 1],
      [5, 0],
      [5, 1],
    ]);
  });
});

describe('dropText: what the drop line says', () => {
  it('given a block dragged after the if, when the plan is read, then it says Move after the if line', () => {
    const model = modelOf(DEF_IF);

    const plan = planDrop(model, block(model, 'b = 2'), at(rowFor(model, 'if x:').subtreeEnd, 1));

    expect(plan?.text).toBe('Move after if x:');
  });

  it('given a chip dropped at the start of the if body, at the start of the else body and above the first row, when the plans are read, then each says where', () => {
    const model = modelOf(DEF_IF_ELSE);
    const ifRow = rowFor(model, 'if x:');
    const clause = rowFor(model, 'else:');

    const intoIf = planDrop(model, chip(), at(ifRow.index + 1, 2));
    const intoElse = planDrop(model, chip(), at(clause.index + 1, 2));
    const top = planDrop(model, chip(), at(0, 0));

    expect([intoIf?.text, intoElse?.text, top?.text]).toEqual(['Insert into if x:', 'Insert into else:', 'Insert before def f(x):']);
  });

  it('given a slot of the model, when its text is asked for a block and for a chip, then the verb follows the source', () => {
    const model = modelOf(DEF_IF);
    const slot = dropSlots(model, chip())[0];

    expect(slot && dropText(model, block(model, 'return b'), slot)).toMatch(/^Move /);
    expect(slot && dropText(model, chip(), slot)).toMatch(/^Insert /);
  });
});

describe('stepOf: the one step a plan makes', () => {
  it('given a block dragged to the slot after the if, when the step is built, then it is one moveNode of the statement, expecting its text', () => {
    const model = modelOf(DEF_IF);
    const ifRow = rowFor(model, 'if x:');
    const plan = planDrop(model, block(model, 'b = 2'), at(ifRow.subtreeEnd, 1)) as DropPlan;

    const step = stepOf(plan, model);

    const moved = rowFor(model, 'b = 2');
    expect(step).toEqual({ op: 'moveNode', node: moved.range, to: { after: ifRow.range }, expect: 'b = 2' });
    expect(step && labelOf(step)).toBe(MOVE_LABEL);
    expect(textOf(model, moved.range)).toBe('b = 2');
  });

  it.each(PYTHON_PALETTE.map((item) => [item.id, item] as const))('given the %s chip dropped after a = 1, when the step is built, then it is one insertSnippet of the chip\'s snippet', (_id, item) => {
    const model = modelOf(DEF_IF);
    const plan = planDrop(model, { kind: 'palette', item: item.id }, at(rowFor(model, 'a = 1').index + 1, 2)) as DropPlan;

    const step = stepOf(plan, model);

    expect(step).toEqual({ op: 'insertSnippet', at: { after: rowFor(model, 'a = 1').range }, text: item.snippet });
    expect(step && labelOf(step)).toBe(INSERT_LABEL);
  });

  it('given a plan for a block the model does not hold, when the step is built, then there is none', () => {
    const model = modelOf(DEF_IF);
    const plan = planDrop(model, block(model, 'b = 2'), at(rowFor(model, 'if x:').subtreeEnd, 1)) as DropPlan;

    expect(stepOf({ ...plan, source: { kind: 'block', id: 'nowhere' } }, model)).toBeNull();
  });
});
