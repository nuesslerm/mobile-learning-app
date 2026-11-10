import type { CanvasSize } from './strokes';

/** A guide fills at most this share of the pad's width. */
const GUIDE_FILL = 0.9;
const MIN_FONT_SIZE = 24;

/**
 * The faint guide's font size: `scale` of the pad's shorter side, shrunk when
 * the text, `naturalWidth` wide at that size, would run past the pad and be
 * clipped. A long word does; a single letter does not. Zero sizes mean not
 * measured yet.
 */
export function guideFontSize(pad: CanvasSize, scale: number, naturalWidth: number): number {
  const base = Math.max(MIN_FONT_SIZE, Math.min(pad.height, pad.width || pad.height) * scale);
  const fit = naturalWidth > 0 && pad.width > 0 ? Math.min(1, (pad.width * GUIDE_FILL) / naturalWidth) : 1;
  return Math.max(MIN_FONT_SIZE, base * fit);
}

/** The size the guide is measured at before it is fitted. */
export function guideBaseFontSize(pad: CanvasSize, scale: number): number {
  return guideFontSize(pad, scale, 0);
}
