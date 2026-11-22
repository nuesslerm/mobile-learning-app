import { createEditorClient, unwrap, type EditorClient } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { buildBlockModel, type BlockModel, type BlockSlot } from '../blockModel';
import { stepOf } from '../blockStep';
import { dropSlots, dropText, type DragSource } from '../dropPlan';
import { PYTHON_PALETTE } from '../palette';

const CORPUS: Readonly<Record<string, string>> = {
  'def with an if and an else': ['def f(x):', '    if x:', '        a = 1', '    else:', '        b = 2', '    return b', ''].join('\n'),
  'def with an if': ['def f(x):', '    if x:', '        a = 1', '        b = 2', '    return b', ''].join('\n'),
  'nested loops': ['def f(n):', '    for i in range(n):', '        while i:', '            i -= 1', '        total = i', '    return total', ''].join('\n'),
  'comments above statements': ['def f():', '    # first', '    a = 1', '    # second', '    # third', '    b = 2', '    return a', ''].join('\n'),
  'class Solution with two methods': ['class Solution:', '    def one(self):', '        return 1', '', '    def two(self):', '        x = 1', '        return x', ''].join('\n'),
  'try with an except and a finally': ['def f():', '    try:', '        a = 1', '    except ValueError:', '        a = 2', '    finally:', '        b = 3', '    return a', ''].join('\n'),
  'statements at the top of the document': ['a = 1', 'b = 2', 'if a:', '    c = 3', 'd = 4', ''].join('\n'),
  'one damaged line among statements': ['def f(x):', '    a = 1', '    for x in', '    b = 2', '    return b', ''].join('\n'),
};

type Drawn = { readonly client: EditorClient; readonly model: BlockModel };

/** The model of what a fake editor holds, built as the view builds it: the text and the tree of the same revision, read through the client. */
async function drawn(text: string): Promise<Drawn> {
  const fake = createFakeEditor({ text, tree: true });
  const client = createEditorClient(fake.engine);
  const read = unwrap(await client.getText());
  const tree = unwrap(await client.tree({ detail: 'outline' }));
  expect(tree.complete).toBe(true);
  expect(read.text).toBe(text);
  return { client, model: buildBlockModel({ text: read.text, tree }) };
}

const sources = (model: BlockModel): readonly DragSource[] => [
  ...model.rows.filter((row) => row.movable).map((row): DragSource => ({ kind: 'block', id: row.id })),
  ...PYTHON_PALETTE.map((item): DragSource => ({ kind: 'palette', item: item.id })),
];

const preview = async ({ client, model }: Drawn, source: DragSource, slot: BlockSlot) => {
  const step = stepOf({ source, slot, text: dropText(model, source, slot) }, model);
  if (!step) throw new Error('no step for the source');
  return client.transact([step], { at: model.revision, instance: model.instance, preview: true, budgetMs: 1000 });
};

const describeDrop = (model: BlockModel, source: DragSource, slot: BlockSlot) => `${dropText(model, source, slot)} (gap ${slot.gap}, depth ${slot.depth}) for ${source.kind === 'block' ? source.id : source.item}`;

describe('the block model against the real engine', () => {
  it.each(Object.entries(CORPUS))('given %s, when every movable block and every chip is previewed at every slot the model offers, then the engine accepts each one', async (_name, text) => {
    const state = await drawn(text);
    const refused: string[] = [];
    let tried = 0;

    for (const source of sources(state.model)) {
      for (const slot of dropSlots(state.model, source)) {
        tried += 1;
        const reply = await preview(state, source, slot);
        if (!reply.ok) refused.push(`${describeDrop(state.model, source, slot)}: ${reply.error.code}`);
      }
    }

    expect(refused).toEqual([]);
    expect(tried).toBeGreaterThan(0);
  });

  it.each(Object.entries(CORPUS))('given %s, when a chip is previewed at every slot the model withheld, then the engine refuses each one as not a node or as code that would not parse', async (_name, text) => {
    const state = await drawn(text);
    const accepted: string[] = [];
    const wrong: string[] = [];

    for (const slot of state.model.slots.filter((one) => !one.valid)) {
      const source: DragSource = { kind: 'palette', item: 'return' };
      const reply = await preview(state, source, slot);
      if (reply.ok) accepted.push(describeDrop(state.model, source, slot));
      else if (reply.error.code !== 'not-a-node' && reply.error.code !== 'would-not-parse') wrong.push(`${describeDrop(state.model, source, slot)}: ${reply.error.code}`);
    }

    expect({ accepted, wrong }).toEqual({ accepted: [], wrong: [] });
  });

  it('given the corpus, when the model is read, then at least one case offers a slot and one withholds a slot, so the two checks above are not empty', async () => {
    const withheld = await Promise.all(Object.values(CORPUS).map(async (text) => (await drawn(text)).model.slots.filter((slot) => !slot.valid).length));

    expect(withheld.some((count) => count > 0)).toBe(true);
    expect(withheld.some((count) => count === 0)).toBe(true);
  });

  it('given a drop previewed at the revision of the model, when the editor has the same text, then the preview names the text a real drop would leave', async () => {
    const state = await drawn(CORPUS['def with an if'] as string);
    const source: DragSource = { kind: 'block', id: state.model.rows.find((row) => row.label === 'b = 2')?.id as string };
    const slot = dropSlots(state.model, source).find((one) => one.gap === 4 && one.depth === 1) as BlockSlot;

    const reply = await preview(state, source, slot);

    expect(reply.ok && reply.value.preview?.text).toBe(['def f(x):', '    if x:', '        a = 1', '    b = 2', '    return b', ''].join('\n'));
  });
});
