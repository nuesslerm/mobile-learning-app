/**
 * Pure geometry for the tracing pad: turning finger samples into an SVG path,
 * and reading back whether what was drawn actually sits on the guide glyph.
 *
 * No React and no react-native-svg here -- the canvas renders `pointsToPath`'s
 * output, and these functions are what the tests can reach.
 */

export type Point = { x: number; y: number };
export type Stroke = readonly Point[];

/** Samples closer together than this are finger jitter, not shape. */
const MIN_SAMPLE_DISTANCE = 1.5;

export function distance(a: Point, b: Point): number {
  return Math.hypot(b.x - a.x, b.y - a.y);
}

/**
 * Appends unless the new sample is within `minDistance` of the last one.
 * Returns the same array reference when nothing was added so a render can skip.
 */
export function appendPoint(
  stroke: Stroke,
  point: Point,
  minDistance: number = MIN_SAMPLE_DISTANCE
): Stroke {
  const last = stroke[stroke.length - 1];
  if (last && distance(last, point) < minDistance) return stroke;
  return [...stroke, point];
}

/**
 * An SVG path for one stroke. A single sample becomes a dot (a zero-length
 * line), which is what a tap on the pad should leave -- and what writing a
 * புள்ளி actually is.
 */
export function pointsToPath(points: Stroke): string {
  if (points.length === 0) return '';
  const [first, ...rest] = points;
  if (rest.length === 0) {
    return `M ${round(first.x)} ${round(first.y)} L ${round(first.x)} ${round(first.y)}`;
  }
  const segments = rest.map((point) => `L ${round(point.x)} ${round(point.y)}`);
  return `M ${round(first.x)} ${round(first.y)} ${segments.join(' ')}`;
}

function round(value: number): number {
  return Math.round(value * 100) / 100;
}

/**
 * The same path, drawn as curves instead of straight segments.
 *
 * Every point becomes a quadratic control point and the curve passes through
 * the midpoints between them -- the standard way to draw freehand ink. Without
 * it a stroke is a polygon: the guide paths come from a pixel skeleton and the
 * learner's own samples are 1.5px apart, so both render as visible facets, and
 * a letterform made of facets does not look like handwriting.
 */
export function pointsToSmoothPath(points: Stroke): string {
  const first = points[0];
  if (!first) return '';
  if (points.length < 3) return pointsToPath(points);

  const segments: string[] = [`M ${round(first.x)} ${round(first.y)}`];
  for (let i = 1; i < points.length - 1; i += 1) {
    const control = points[i];
    const next = points[i + 1];
    if (!control || !next) continue;
    const midX = (control.x + next.x) / 2;
    const midY = (control.y + next.y) / 2;
    segments.push(
      `Q ${round(control.x)} ${round(control.y)} ${round(midX)} ${round(midY)}`
    );
  }

  const last = points[points.length - 1];
  const penultimate = points[points.length - 2];
  if (last && penultimate) {
    segments.push(
      `Q ${round(penultimate.x)} ${round(penultimate.y)} ${round(last.x)} ${round(last.y)}`
    );
  }
  return segments.join(' ');
}

export type BoundingBox = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  width: number;
  height: number;
};

export function boundingBox(strokes: readonly Stroke[]): BoundingBox | null {
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  let seen = false;

  for (const stroke of strokes) {
    for (const point of stroke) {
      seen = true;
      if (point.x < minX) minX = point.x;
      if (point.y < minY) minY = point.y;
      if (point.x > maxX) maxX = point.x;
      if (point.y > maxY) maxY = point.y;
    }
  }

  if (!seen) return null;
  return { minX, minY, maxX, maxY, width: maxX - minX, height: maxY - minY };
}

export function inkLength(strokes: readonly Stroke[]): number {
  let total = 0;
  for (const stroke of strokes) {
    for (let i = 1; i < stroke.length; i += 1) {
      total += distance(stroke[i - 1], stroke[i]);
    }
  }
  return total;
}

export type FitVerdict =
  | 'empty'
  | 'too-small'
  | 'too-large'
  | 'off-left'
  | 'off-right'
  | 'off-top'
  | 'off-bottom'
  | 'good';

export type CanvasSize = { width: number; height: number };

/**
 * Objective feedback the app can give without knowing the glyph's outline:
 * whether the writing fills the guide box and sits centred on it. Says nothing
 * about whether the letter is *correct* -- that judgement stays with the
 * learner, which is why the practice screen asks.
 */
export function describeFit(
  strokes: readonly Stroke[],
  canvas: CanvasSize
): FitVerdict {
  const box = boundingBox(strokes);
  if (!box || canvas.width <= 0 || canvas.height <= 0) return 'empty';

  const widthRatio = box.width / canvas.width;
  const heightRatio = box.height / canvas.height;
  const filled = Math.max(widthRatio, heightRatio);

  if (filled < 0.25) return 'too-small';
  if (widthRatio > 0.98 || heightRatio > 0.98) return 'too-large';

  const centerX = (box.minX + box.maxX) / 2 / canvas.width;
  const centerY = (box.minY + box.maxY) / 2 / canvas.height;

  // Generous, deliberately: the guide glyph is centred as a line box, and a
  // Tamil letter does not sit dead centre inside its own line box. A tighter
  // band would nag about placement that matches the guide.
  if (centerX < 0.3) return 'off-left';
  if (centerX > 0.7) return 'off-right';
  if (centerY < 0.3) return 'off-top';
  if (centerY > 0.7) return 'off-bottom';

  return 'good';
}

export const FIT_MESSAGES: Record<FitVerdict, string> = {
  empty: 'Trace over the guide letter with your finger.',
  'too-small': 'Bigger — fill the guide letter.',
  'too-large': 'Slightly smaller — stay inside the box.',
  'off-left': 'Shift right — the guide letter is further over.',
  'off-right': 'Shift left — the guide letter is further over.',
  'off-top': 'Lower — the guide letter sits further down.',
  'off-bottom': 'Higher — the guide letter sits further up.',
  good: 'Good size and placement.',
};

/**
 * The learner's ink as a standalone SVG, black on white.
 *
 * What gets sent for marking: a picture of what they wrote and nothing else --
 * no template, no guide, no grid. A model shown the answer next to the attempt
 * marks the answer.
 */
export function strokesToSvg(
  strokes: readonly Stroke[],
  canvas: CanvasSize,
  strokeWidth = 8
): string {
  const width = Math.max(1, Math.round(canvas.width));
  const height = Math.max(1, Math.round(canvas.height));
  const paths = strokes
    .filter((stroke) => stroke.length >= 2)
    .map(
      (stroke) =>
        `<path d="${pointsToSmoothPath(stroke)}" fill="none" stroke="#000" ` +
        `stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round"/>`
    )
    .join('');
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" ` +
    `viewBox="0 0 ${width} ${height}">` +
    `<rect width="${width}" height="${height}" fill="#fff"/>${paths}</svg>`
  );
}
