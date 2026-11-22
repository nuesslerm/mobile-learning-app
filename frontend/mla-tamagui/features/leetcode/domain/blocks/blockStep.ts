import type { Step } from '@lib/code-editor-api';

import type { BlockModel } from './blockModel';
import type { DropPlan } from './dropPlan';
import { paletteItem } from './palette';

export type BlockStep = Extract<Step, { readonly op: 'moveNode' | 'insertSnippet' }>;

export const MOVE_LABEL = 'blocks.move';
export const INSERT_LABEL = 'blocks.insert';

export const labelOf = (step: BlockStep): string => (step.op === 'moveNode' ? MOVE_LABEL : INSERT_LABEL);

/** The one step a drop makes: the block's statement moved to the slot's destination, expecting the text it was drawn from, or the chip's snippet inserted there. Null when the source is not in the model. */
export function stepOf(plan: DropPlan, model: BlockModel): BlockStep | null {
  const { source, slot } = plan;
  if (source.kind === 'palette') {
    const item = paletteItem(source.item);
    return item ? { op: 'insertSnippet', at: slot.destination, text: item.snippet } : null;
  }
  const row = model.rows.find((one) => one.id === source.id);
  if (!row) return null;
  return { op: 'moveNode', node: row.range, to: slot.destination, expect: model.text.slice(row.range.from, row.range.to) };
}
