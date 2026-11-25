import type { Stroke } from '../domain/strokes';

export function traceGuideValue(stroke: Stroke): string {
  // XCTest limits accessibility values to 512 characters. Keep both endpoints and sample in drawing order.
  const count = Math.min(stroke.length, 48);
  const points = count < 2 ? [] : Array.from({ length: count }, (_, i) => {
    const { x, y } = stroke[Math.round(i * (stroke.length - 1) / (count - 1))]!;
    return [Math.round(x), Math.round(y)];
  });
  return JSON.stringify({ guide: points.length ? [points] : [] });
}
