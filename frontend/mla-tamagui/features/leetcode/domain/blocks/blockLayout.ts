/** The geometry the view draws and the planner reads: rows of one height, a gutter for the grip, one indent step per depth. The list's origin is its top-left corner. */
export const ROW_HEIGHT = 44;
export const INDENT_STEP = 24;
export const GUTTER = 44;
export const PALETTE_HEIGHT = 44;
export const CHIP_WIDTH = 72;

export type Point = { readonly x: number; readonly y: number };

export const rowTop = (index: number): number => index * ROW_HEIGHT;
export const rowCenter = (index: number): number => index * ROW_HEIGHT + ROW_HEIGHT / 2;
export const gapY = (gap: number): number => gap * ROW_HEIGHT;
export const depthX = (depth: number): number => GUTTER + depth * INDENT_STEP;
export const listHeight = (rowCount: number): number => rowCount * ROW_HEIGHT;
/** where chip `index` of the palette starts in list coordinates: the palette sits right above the list */
export const chipOrigin = (index: number): Point => ({ x: index * CHIP_WIDTH, y: -PALETTE_HEIGHT });

/** The row boundary nearest to `y`, from 0 above the first row to `rowCount` below the last. */
export const gapAt = (y: number, rowCount: number): number => Math.min(rowCount, Math.max(0, Math.round(y / ROW_HEIGHT)));

/** The depth an x names, between the whole ones: depth 0 starts where the gutter ends. */
export const depthAt = (x: number): number => (x - GUTTER) / INDENT_STEP;
