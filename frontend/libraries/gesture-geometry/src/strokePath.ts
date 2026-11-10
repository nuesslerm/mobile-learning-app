/**
 * Turning a handful of path points into the pointer samples a stroke is made
 * of.
 *
 * Kept pure and separate from the session so the geometry -- which is the part
 * that can silently be wrong -- is unit-testable without a browser. `drag()`
 * walks a straight line between two points; this is what a gesture that is
 * actually a SHAPE needs, and a straight line cannot exercise: direction
 * changes, curvature, and the sample density a recognizer sees along the way.
 */

export type StrokePoint = { x: number; y: number };

/** `fraction`: 0..1 of the element's box. `px`: offset from its top-left. */
export type StrokeUnits = 'fraction' | 'px';

export type ElementBox = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export const DEFAULT_SAMPLES_PER_SEGMENT = 12;

/**
 * Path points -> absolute page coordinates.
 *
 * Fractions are the default because a path that describes a letter should
 * survive a different device preset: the same 0..1 path traces the same shape
 * on a 393pt phone and a 1024pt tablet, where pixel offsets would run off the
 * element.
 */
export function toPageCoordinates(
  points: readonly StrokePoint[],
  box: ElementBox,
  units: StrokeUnits = 'fraction'
): StrokePoint[] {
  return points.map((point) =>
    units === 'px'
      ? { x: box.x + point.x, y: box.y + point.y }
      : { x: box.x + point.x * box.width, y: box.y + point.y * box.height }
  );
}

/** Straight segments between consecutive points, sampled evenly. */
export function polylineSamples(
  points: readonly StrokePoint[],
  samplesPerSegment: number = DEFAULT_SAMPLES_PER_SEGMENT
): StrokePoint[] {
  assertPath(points, samplesPerSegment);
  const samples: StrokePoint[] = [at(points, 0)];
  for (let i = 0; i < points.length - 1; i += 1) {
    const from = at(points, i);
    const to = at(points, i + 1);
    for (let step = 1; step <= samplesPerSegment; step += 1) {
      const t = step / samplesPerSegment;
      samples.push({
        x: from.x + (to.x - from.x) * t,
        y: from.y + (to.y - from.y) * t,
      });
    }
  }
  return samples;
}

/**
 * A Catmull-Rom spline through the points -- every given point is ON the
 * curve, which is what makes a path readable as "these are the places the
 * stroke goes through" rather than a set of control handles to reason about.
 *
 * The endpoints are duplicated so the first and last spans have the four
 * points the basis needs.
 */
export function curveSamples(
  points: readonly StrokePoint[],
  samplesPerSegment: number = DEFAULT_SAMPLES_PER_SEGMENT
): StrokePoint[] {
  assertPath(points, samplesPerSegment);
  // Two points have no curvature to interpolate; a spline through them is the
  // straight line, so say so directly rather than computing it.
  if (points.length === 2) return polylineSamples(points, samplesPerSegment);

  const samples: StrokePoint[] = [at(points, 0)];
  for (let i = 0; i < points.length - 1; i += 1) {
    const p1 = at(points, i);
    const p2 = at(points, i + 1);
    const p0 = points[i - 1] ?? p1;
    const p3 = points[i + 2] ?? p2;
    for (let step = 1; step <= samplesPerSegment; step += 1) {
      samples.push(catmullRomAt(p0, p1, p2, p3, step / samplesPerSegment));
    }
  }
  return samples;
}

function catmullRomAt(
  p0: StrokePoint,
  p1: StrokePoint,
  p2: StrokePoint,
  p3: StrokePoint,
  t: number
): StrokePoint {
  const t2 = t * t;
  const t3 = t2 * t;
  const axis = (a: number, b: number, c: number, d: number): number =>
    0.5 *
    (2 * b + (c - a) * t + (2 * a - 5 * b + 4 * c - d) * t2 + (-a + 3 * b - 3 * c + d) * t3);
  return {
    x: axis(p0.x, p1.x, p2.x, p3.x),
    y: axis(p0.y, p1.y, p2.y, p3.y),
  };
}

/**
 * Deliberate distortions of a path, for testing what a validator REJECTS.
 *
 * A validator is only as good as the wrong inputs it has seen: a check that
 * accepts the correct stroke proves nothing on its own, because a check that
 * accepts everything passes it too. These make the specific wrong strokes --
 * backwards, displaced, too small, cut short, shaky -- reproducible.
 */
export type StrokeTransform = {
  /** Walk the path end-to-start. */
  reverse?: boolean;
  /** Shift every point, in the same units as the path. */
  offset?: StrokePoint;
  /** Scale about the path's own centre, so the shape stays and the size does not. */
  scale?: number;
  /** Keep only this fraction (0..1] of the path, from its start. */
  truncate?: number;
  /** Peak perpendicular wobble, in the path's units. Deterministic: the same
   * `wobbleSeed` always produces the same shake. */
  wobble?: number;
  wobbleSeed?: number;
};

export type StrokeSampleOptions = {
  curve?: boolean;
  samplesPerSegment?: number;
};

export function strokeSamples(
  points: readonly StrokePoint[],
  options: StrokeSampleOptions = {}
): StrokePoint[] {
  const curve = options.curve ?? false;
  const samplesPerSegment =
    options.samplesPerSegment ?? DEFAULT_SAMPLES_PER_SEGMENT;
  return curve
    ? curveSamples(points, samplesPerSegment)
    : polylineSamples(points, samplesPerSegment);
}

/** Indexing a validated path: the bounds are already known, so a miss is a bug. */
function at(points: readonly StrokePoint[], index: number): StrokePoint {
  const point = points[index];
  if (!point) throw new Error(`stroke: no path point at index ${index}`);
  return point;
}

function assertPath(
  points: readonly StrokePoint[],
  samplesPerSegment: number
): void {
  if (points.length < 2) {
    throw new Error(
      `stroke: a path needs at least 2 points, got ${points.length}`
    );
  }
  if (!Number.isInteger(samplesPerSegment) || samplesPerSegment < 1) {
    throw new Error(
      `stroke: samplesPerSegment must be a positive integer, got ${samplesPerSegment}`
    );
  }
  for (const point of points) {
    if (!Number.isFinite(point.x) || !Number.isFinite(point.y)) {
      throw new Error(`stroke: path point is not finite: ${JSON.stringify(point)}`);
    }
  }
}

/** Centre of a path's bounding box. Scaling about it keeps the shape put. */
function pathCentre(points: readonly StrokePoint[]): StrokePoint {
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  for (const point of points) {
    minX = Math.min(minX, point.x);
    minY = Math.min(minY, point.y);
    maxX = Math.max(maxX, point.x);
    maxY = Math.max(maxY, point.y);
  }
  return { x: (minX + maxX) / 2, y: (minY + maxY) / 2 };
}

/** A small deterministic PRNG, so a "shaky" stroke is the SAME shaky stroke. */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Applies the distortions in a fixed order: truncate, reverse, scale, offset,
 * wobble. The order matters and is not arbitrary -- truncating after reversing
 * would cut the other end, and wobbling before scaling would scale the wobble.
 */
export function transformPath(
  points: readonly StrokePoint[],
  transform: StrokeTransform = {}
): StrokePoint[] {
  let path = [...points];

  const truncate = transform.truncate;
  if (truncate !== undefined) {
    if (!(truncate > 0) || truncate > 1) {
      throw new Error(
        `stroke: truncate must be in (0, 1], got ${truncate}`
      );
    }
    path = truncateByLength(path, truncate);
  }

  if (transform.reverse) path = path.reverse();

  const scale = transform.scale;
  if (scale !== undefined) {
    if (!Number.isFinite(scale) || scale === 0) {
      throw new Error(`stroke: scale must be a non-zero number, got ${scale}`);
    }
    const centre = pathCentre(path);
    path = path.map((point) => ({
      x: centre.x + (point.x - centre.x) * scale,
      y: centre.y + (point.y - centre.y) * scale,
    }));
  }

  const offset = transform.offset;
  if (offset) {
    path = path.map((point) => ({
      x: point.x + offset.x,
      y: point.y + offset.y,
    }));
  }

  const wobble = transform.wobble;
  if (wobble !== undefined && wobble !== 0) {
    const random = mulberry32(transform.wobbleSeed ?? 1);
    path = path.map((point, index) => {
      // The endpoints stay put: a wobbly stroke should still start and end
      // where it was aimed, or every rejection is just "wrong start".
      if (index === 0 || index === path.length - 1) return point;
      return {
        x: point.x + (random() * 2 - 1) * wobble,
        y: point.y + (random() * 2 - 1) * wobble,
      };
    });
  }

  return path;
}

/**
 * Keeps the first `fraction` of a path BY LENGTH, interpolating the point it
 * stops at.
 *
 * Not by point count, which was the first attempt and is wrong wherever it
 * matters: a two-point stroke truncated to a quarter still has two points, so
 * "cut short" handed back the whole stroke and a validator that correctly
 * accepted it looked broken.
 */
export function truncateByLength(
  points: readonly StrokePoint[],
  fraction: number
): StrokePoint[] {
  const first = points[0];
  if (!first || points.length < 2) return [...points];

  let total = 0;
  for (let i = 1; i < points.length; i += 1) {
    const from = points[i - 1];
    const to = points[i];
    if (from && to) total += Math.hypot(to.x - from.x, to.y - from.y);
  }
  if (total === 0) return [...points];

  const target = total * fraction;
  const kept: StrokePoint[] = [first];
  let walked = 0;

  for (let i = 1; i < points.length; i += 1) {
    const from = points[i - 1];
    const to = points[i];
    if (!from || !to) continue;
    const span = Math.hypot(to.x - from.x, to.y - from.y);
    if (walked + span < target) {
      kept.push(to);
      walked += span;
      continue;
    }
    const t = span === 0 ? 0 : (target - walked) / span;
    kept.push({
      x: from.x + (to.x - from.x) * t,
      y: from.y + (to.y - from.y) * t,
    });
    break;
  }

  // A single point is not a path; keep the next one so the pointer still moves.
  if (kept.length < 2) {
    const second = points[1];
    if (second) kept.push(second);
  }
  return kept;
}
