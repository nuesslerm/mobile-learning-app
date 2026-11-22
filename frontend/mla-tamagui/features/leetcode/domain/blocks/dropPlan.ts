import { depthAt, gapAt, type Point } from './blockLayout';
import { placeOf, type BlockModel, type BlockRow, type BlockSlot } from './blockModel';
import type { PaletteItemId } from './palette';

/** What is being dragged: a block by its id, or a chip of the palette. */
export type DragSource = { readonly kind: 'block'; readonly id: string } | { readonly kind: 'palette'; readonly item: PaletteItemId };

/** Where a drop lands: the slot, and the words that say so. */
export type DropPlan = { readonly source: DragSource; readonly slot: BlockSlot; readonly text: string };

const isInside = (slot: BlockSlot, id: string): boolean => slot.owner !== null && (slot.owner === id || slot.owner.startsWith(`${id}.`));

/** The slots a source may be dropped at: the valid ones, and for a block neither one inside itself (a move into itself) nor one that leaves it where it is (the slot before it and the slot after it). */
export function dropSlots(model: BlockModel, source: DragSource): readonly BlockSlot[] {
  if (source.kind === 'palette') return model.slots.filter((slot) => slot.valid);
  const row = model.rows.find((one) => one.id === source.id);
  if (!row?.movable) return [];
  const own = placeOf(row.id);
  const stays = (slot: BlockSlot) => slot.owner === own.owner && slot.block === own.block && (slot.position === own.position || slot.position === own.position + 1);
  return model.slots.filter((slot) => slot.valid && !isInside(slot, row.id) && !stays(slot));
}

/** The row a slot is relative to: the statement it goes after or before, or the header or clause line of the block it goes into. */
export function referenceOf(model: BlockModel, slot: BlockSlot): BlockRow | undefined {
  const prefix = slot.owner === null ? '' : `${slot.owner}.${slot.block}.`;
  const id = slot.position > 0 ? `${prefix}${slot.position - 1}` : slot.owner === null ? '0' : slot.block === 0 ? slot.owner : `${slot.owner}.c${slot.block}`;
  return model.rows.find((row) => row.id === id);
}

/** What the drop line says in words: 'Move after if x:', 'Insert into else:'. */
export function dropText(model: BlockModel, source: DragSource, slot: BlockSlot): string {
  const verb = source.kind === 'block' ? 'Move' : 'Insert';
  const reference = referenceOf(model, slot)?.label ?? '';
  const where = slot.position > 0 ? 'after' : slot.owner === null ? 'before' : 'into';
  return `${verb} ${where} ${reference}`.trim();
}

/** The slot a pointer in list coordinates drops at: the valid slots at the row boundary nearest to y, then the one whose depth is nearest to x (the shallower when two are as near). Null when there is none. */
export function planDrop(model: BlockModel, source: DragSource, pointer: Point): DropPlan | null {
  const gap = gapAt(pointer.y, model.rows.length);
  const target = depthAt(pointer.x);
  let best: BlockSlot | null = null;
  for (const slot of dropSlots(model, source)) {
    if (slot.gap !== gap) continue;
    if (!best || Math.abs(slot.depth - target) < Math.abs(best.depth - target) || (Math.abs(slot.depth - target) === Math.abs(best.depth - target) && slot.depth < best.depth)) best = slot;
  }
  return best ? { source, slot: best, text: dropText(model, source, best) } : null;
}
