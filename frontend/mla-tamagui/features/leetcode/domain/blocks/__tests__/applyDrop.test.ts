import { createEditorClient, type ApplyParams, type EditorClient, type EditorError, type Reply, type Step } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { applyDrop, describeOutcome, DROP_BUDGET_MS, type DropOutcome } from '../applyDrop';
import { depthX, gapY } from '../blockLayout';
import type { BlockModel } from '../blockModel';
import { INSERT_LABEL, MOVE_LABEL } from '../blockStep';
import { planDrop, type DragSource, type DropPlan } from '../dropPlan';
import { readBlockModel } from '../readBlockModel';

const DEF_IF = ['def f(x):', '    if x:', '        a = 1', '        b = 2', '    return b', ''].join('\n');
const MOVED = ['def f(x):', '    if x:', '        a = 1', '    b = 2', '    return b', ''].join('\n');
const HALF_TYPED = ['def f(x):', '    a = 1', '    for x in', '    b = 2', '    return b', ''].join('\n');

type Call = { readonly steps: readonly Step[]; readonly options: Omit<ApplyParams, 'steps'> | undefined };

async function drawn(text: string) {
  const fake = createFakeEditor({ text, tree: true });
  const real = createEditorClient(fake.engine);
  const calls: Call[] = [];
  const client: EditorClient = {
    ...real,
    transact: (steps, options) => {
      calls.push({ steps, options });
      return real.transact(steps, options);
    },
  };
  const read = await readBlockModel(client);
  if (read.kind !== 'ready') throw new Error(`the model was not read: ${read.kind}`);
  return { fake, client, calls, model: read.model };
}

const rowId = (model: BlockModel, label: string): string => {
  const row = model.rows.find((one) => one.label === label);
  if (!row) throw new Error(`no row for ${label}`);
  return row.id;
};
const planAt = (model: BlockModel, source: DragSource, gap: number, depth: number): DropPlan => {
  const plan = planDrop(model, source, { x: depthX(depth), y: gapY(gap) });
  if (!plan) throw new Error('no plan');
  return plan;
};
const moveBAfterIf = (model: BlockModel) => planAt(model, { kind: 'block', id: rowId(model, 'b = 2') }, (model.rows.find((row) => row.label === 'if x:') as { subtreeEnd: number }).subtreeEnd, 1);

const failing = (client: EditorClient, error: EditorError): EditorClient => ({ ...client, transact: async () => ({ ok: false, revision: 0, error }) as Reply<never> });

describe('applyDrop: a plan becomes one call', () => {
  it('given b = 2 moved out of the if, when it is applied, then one moveNode is sent at the tree\'s revision and instance, the text moves and one undo restores it', async () => {
    const { fake, client, calls, model } = await drawn(DEF_IF);
    const plan = moveBAfterIf(model);

    const outcome = await applyDrop(client, model, plan);

    expect(calls).toHaveLength(1);
    expect(calls[0]).toEqual({
      steps: [{ op: 'moveNode', node: model.rows.find((row) => row.label === 'b = 2')?.range, to: plan.slot.destination, expect: 'b = 2' }],
      options: { at: model.revision, instance: model.instance, label: MOVE_LABEL, budgetMs: DROP_BUDGET_MS },
    });
    expect(outcome).toEqual({ kind: 'moved', revision: model.revision + 1 });
    expect(fake.text()).toBe(MOVED);
    expect(fake.peek().undoDepth).toBe(1);
    await client.undo();
    expect(fake.text()).toBe(DEF_IF);
  });

  it('given the if chip dropped after a = 1, when it is applied, then one insertSnippet is sent, the snippet lands at the slot\'s indent and one undo removes it', async () => {
    const { fake, client, calls, model } = await drawn(DEF_IF);
    const plan = planAt(model, { kind: 'palette', item: 'if' }, (model.rows.find((row) => row.label === 'a = 1')?.index ?? 0) + 1, 2);

    const outcome = await applyDrop(client, model, plan);

    expect(calls).toHaveLength(1);
    expect(calls[0]).toEqual({
      steps: [{ op: 'insertSnippet', at: plan.slot.destination, text: 'if condition:\n    pass' }],
      options: { at: model.revision, instance: model.instance, label: INSERT_LABEL, budgetMs: DROP_BUDGET_MS },
    });
    expect(outcome.kind).toBe('moved');
    expect(fake.text()).toBe(['def f(x):', '    if x:', '        a = 1', '        if condition:', '            pass', '        b = 2', '    return b', ''].join('\n'));
    expect(fake.peek().undoDepth).toBe(1);
    await client.undo();
    expect(fake.text()).toBe(DEF_IF);
  });

  it('given a keystroke typed after the rows were drawn, when the drop is applied, then it is outdated as stale, the text holds only the keystroke and the drop is sent once', async () => {
    const { fake, client, calls, model } = await drawn(DEF_IF);
    const plan = moveBAfterIf(model);
    fake.userTypes('#');

    const outcome = await applyDrop(client, model, plan);

    expect(outcome).toEqual({ kind: 'outdated', code: 'stale' });
    expect(fake.text()).toBe(`#${DEF_IF}`);
    expect(calls).toHaveLength(1);
  });

  it('given rows drawn from another page than the one that answers, when the drop is applied, then it is outdated as an instance mismatch and the text is untouched', async () => {
    const { fake, client, model } = await drawn(DEF_IF);

    const outcome = await applyDrop(client, { ...model, instance: 'another-page' }, moveBAfterIf(model));

    expect(outcome).toEqual({ kind: 'outdated', code: 'instance-mismatch' });
    expect(fake.text()).toBe(DEF_IF);
  });

  it('given a drop forced onto a slot beside a damaged statement, when it is applied, then it is refused as damaged and the text is untouched', async () => {
    const { fake, client, model } = await drawn(HALF_TYPED);
    const slot = model.slots.find((one) => !one.valid);
    if (!slot) throw new Error('the model withheld no slot');
    const plan: DropPlan = { source: { kind: 'block', id: rowId(model, 'b = 2') }, slot, text: '' };

    const outcome = await applyDrop(client, model, plan);

    expect(outcome).toEqual({ kind: 'refused', reason: 'damaged' });
    expect(fake.text()).toBe(HALF_TYPED);
    expect(describeOutcome(outcome)).toMatch(/syntax error/);
  });

  it('given a plan that leaves the code as it is, when it is applied, then the engine changes nothing and the outcome is a no-op', async () => {
    const { fake, client, model } = await drawn(DEF_IF);
    const stays = model.slots.find((slot) => slot.valid && slot.owner === null && slot.position === 0);
    if (!stays) throw new Error('no root slot');
    const plan: DropPlan = { source: { kind: 'block', id: rowId(model, 'def f(x):') }, slot: stays, text: '' };

    const outcome = await applyDrop(client, model, plan);

    expect(outcome).toEqual({ kind: 'noop' });
    expect(fake.text()).toBe(DEF_IF);
    expect(fake.peek().undoDepth).toBe(0);
  });

  it('given a plan for a block the model does not hold, when it is applied, then nothing is sent', async () => {
    const { client, calls, model } = await drawn(DEF_IF);
    const plan = { ...moveBAfterIf(model), source: { kind: 'block', id: 'nowhere' } satisfies DragSource };

    expect(await applyDrop(client, model, plan)).toEqual({ kind: 'noop' });
    expect(calls).toHaveLength(0);
  });
});

describe('applyDrop: what the engine can answer', () => {
  it.each<[string, EditorError, DropOutcome]>([
    ['structure-changed', { code: 'structure-changed', message: 'moved', head: 4 }, { kind: 'outdated', code: 'structure-changed' }],
    ['range-lost', { code: 'range-lost', message: 'lost', step: 0 }, { kind: 'outdated', code: 'range-lost' }],
    ['text-mismatch', { code: 'text-mismatch', message: 'other text', step: 0, expected: 'a', actual: 'b' }, { kind: 'outdated', code: 'text-mismatch' }],
    ['out-of-range', { code: 'out-of-range', message: 'past the end', length: 3 }, { kind: 'outdated', code: 'out-of-range' }],
    ['would-not-parse', { code: 'would-not-parse', message: 'breaks', errors: [], reason: 'structure' }, { kind: 'refused', reason: 'would-not-parse' }],
    ['parse-timeout', { code: 'parse-timeout', message: 'slow', budgetMs: 1000 }, { kind: 'refused', reason: 'parse-timeout' }],
    ['unsupported', { code: 'unsupported', message: 'no', what: 'step:moveNode' }, { kind: 'refused', reason: 'unsupported' }],
    ['not-a-node with a reason', { code: 'not-a-node', message: 'one line', range: { from: 0, to: 1 }, reason: 'shares-line' }, { kind: 'refused', reason: 'shares-line' }],
    ['not-a-node with none', { code: 'not-a-node', message: 'no', range: { from: 0, to: 1 } }, { kind: 'refused', reason: 'not-a-node' }],
    ['not-ready', { code: 'not-ready', message: 'starting' }, { kind: 'unavailable', code: 'not-ready' }],
    ['transport', { code: 'transport', message: 'gone', cause: 'timeout' }, { kind: 'unavailable', code: 'transport' }],
  ])('given the engine answers %s, when the drop is applied, then the outcome is %j', async (_name, error, expected) => {
    const { fake, client, model } = await drawn(DEF_IF);

    const outcome = await applyDrop(failing(client, error), model, moveBAfterIf(model));

    expect(outcome).toEqual(expected);
    expect(fake.text()).toBe(DEF_IF);
  });
});

describe('describeOutcome', () => {
  it('given a move and a no-op, when they are described, then there is nothing to say', () => {
    expect(describeOutcome({ kind: 'moved', revision: 3 })).toBeNull();
    expect(describeOutcome({ kind: 'noop' })).toBeNull();
  });

  it.each<DropOutcome>([
    { kind: 'outdated', code: 'stale' },
    { kind: 'unavailable', code: 'transport' },
    { kind: 'refused', reason: 'damaged' },
    { kind: 'refused', reason: 'shares-line' },
    { kind: 'refused', reason: 'would-not-parse' },
    { kind: 'refused', reason: 'parse-timeout' },
    { kind: 'refused', reason: 'unsupported' },
    { kind: 'refused', reason: 'something-new' },
  ])('given the outcome %j, when it is described, then there is a message', (outcome) => {
    expect(describeOutcome(outcome)?.length).toBeGreaterThan(0);
  });
});
