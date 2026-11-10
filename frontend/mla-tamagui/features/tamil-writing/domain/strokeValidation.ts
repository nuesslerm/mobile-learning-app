import type { ReferenceStroke } from './strokeGuide';
import { distance, type CanvasSize, type Point, type Stroke } from './strokes';

/**
 * Marking one stroke against the stroke it was supposed to be.
 *
 * Four things can be wrong with a stroke, and they need different words back:
 * it can be too short to be the stroke at all, it can start in the wrong
 * place, it can run the right shape the wrong way round, or it can wander off
 * the path. "Try again" tells a learner none of that.
 *
 * Everything here works in pad fractions, so the tolerances mean the same
 * thing on any screen.
 */

type StrokeRejection =
  | 'too-short'
  | 'wrong-start'
  | 'backwards'
  | 'off-path';

export type StrokeAssessment = {
  ok: boolean;
  reason?: StrokeRejection;
  /** What to show the learner. */
  message: string;
  /** 0..1. 1 is on the line the whole way. */
  score: number;
  /** Mean departure from the reference path, in pad fractions. */
  deviation: number;
};

/** A stroke shorter than this fraction of the reference is not that stroke. */
const MIN_LENGTH_RATIO = 0.6;
/** How far from the reference's first point a stroke may begin. */
export const START_TOLERANCE = 0.2;
/**
 * How far from the reference's last point a stroke may end.
 *
 * Length alone does not catch a stroke stopped half way: a half stroke that
 * sits exactly on the line has near-zero deviation, because every point it
 * drew was right -- it just stopped. Found live: the top half of அ's vertical
 * scored 89%.
 */
export const END_TOLERANCE = 0.2;
/** Mean departure from the path that still counts as following it. */
export const PATH_TOLERANCE = 0.12;
/** Deviation at which the score reaches zero. */
const ZERO_SCORE_DEVIATION = 0.3;
/** Points each path is resampled to before they are compared. */
const COMPARISON_SAMPLES = 24;

export function pathLength(points: readonly Point[]): number {
  let total = 0;
  for (let i = 1; i < points.length; i += 1) {
    const from = points[i - 1];
    const to = points[i];
    if (from && to) total += distance(from, to);
  }
  return total;
}

/** Pad pixels -> pad fractions, so tolerances are screen-independent. */
export function normalizeStroke(
  stroke: Stroke,
  canvas: CanvasSize
): Point[] {
  if (canvas.width <= 0 || canvas.height <= 0) return [];
  return stroke.map((point) => ({
    x: point.x / canvas.width,
    y: point.y / canvas.height,
  }));
}

/** `count` points spread evenly along the path by arc length. */
export function resample(points: readonly Point[], count: number): Point[] {
  const first = points[0];
  if (!first || count < 2) return first ? [first] : [];
  const total = pathLength(points);
  if (total === 0) return new Array(count).fill(first) as Point[];

  const step = total / (count - 1);
  const out: Point[] = [first];
  let segment = 1;
  let walked = 0;

  for (let i = 1; i < count - 1; i += 1) {
    const target = step * i;
    while (segment < points.length) {
      const from = points[segment - 1];
      const to = points[segment];
      if (!from || !to) break;
      const span = distance(from, to);
      if (walked + span >= target || segment === points.length - 1) {
        const t = span === 0 ? 0 : (target - walked) / span;
        out.push({
          x: from.x + (to.x - from.x) * t,
          y: from.y + (to.y - from.y) * t,
        });
        break;
      }
      walked += span;
      segment += 1;
    }
  }

  const last = points[points.length - 1];
  if (last) out.push(last);
  return out;
}

function distanceToSegment(point: Point, a: Point, b: Point): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const lengthSquared = dx * dx + dy * dy;
  if (lengthSquared === 0) return distance(point, a);
  const t = Math.max(
    0,
    Math.min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / lengthSquared)
  );
  return distance(point, { x: a.x + dx * t, y: a.y + dy * t });
}

function distanceToPath(point: Point, path: readonly Point[]): number {
  let best = Number.POSITIVE_INFINITY;
  for (let i = 1; i < path.length; i += 1) {
    const from = path[i - 1];
    const to = path[i];
    if (from && to) best = Math.min(best, distanceToSegment(point, from, to));
  }
  const only = path[0];
  if (best === Number.POSITIVE_INFINITY && only) return distance(point, only);
  return best;
}

/**
 * Mean departure between two paths, measured BOTH ways and the worse one kept.
 * One way alone is not enough: a short scribble sitting on top of the
 * reference scores perfectly if only the scribble's points are measured.
 */
export function meanDeviation(
  a: readonly Point[],
  b: readonly Point[]
): number {
  const left = resample(a, COMPARISON_SAMPLES);
  const right = resample(b, COMPARISON_SAMPLES);
  if (left.length < 2 || right.length < 2) return Number.POSITIVE_INFINITY;

  const mean = (from: Point[], to: Point[]): number =>
    from.reduce((sum, point) => sum + distanceToPath(point, to), 0) /
    from.length;

  return Math.max(mean(left, right), mean(right, left));
}

/**
 * Was the shape walked the wrong way round?
 *
 * Compared position by position along both paths, not end to end. Endpoints
 * alone cannot see it on a closed stroke -- ஆ has a loop whose start and end
 * are the same point, so a reversed trace scored identically and was accepted.
 */
function runsBackwards(
  drawn: readonly Point[],
  target: readonly Point[]
): boolean {
  const left = resample(drawn, COMPARISON_SAMPLES);
  const right = resample(target, COMPARISON_SAMPLES);
  if (left.length !== right.length || left.length < 2) return false;

  const reversed = [...right].reverse();
  let forwards = 0;
  let backwards = 0;
  for (let i = 0; i < left.length; i += 1) {
    const point = left[i];
    const ahead = right[i];
    const behind = reversed[i];
    if (!point || !ahead || !behind) continue;
    forwards += distance(point, ahead);
    backwards += distance(point, behind);
  }
  // A clear margin, not a hair: a symmetrical stroke sits near both, and
  // calling that backwards would refuse a correct one at random.
  return backwards < forwards * 0.8;
}

const MESSAGES: Record<StrokeRejection, string> = {
  'too-short': 'Too short — follow the whole stroke to its end.',
  'wrong-start': 'Start from the marked dot.',
  backwards: 'Right shape, wrong way — follow the arrow.',
  'off-path': 'Off the line — keep to the guide stroke.',
};

export function assessStroke(
  drawn: Stroke,
  reference: ReferenceStroke,
  canvas: CanvasSize
): StrokeAssessment {
  const path = normalizeStroke(drawn, canvas);
  const target = reference.points;
  const drawnStart = path[0];
  const drawnEnd = path[path.length - 1];
  const targetStart = target[0];
  const targetEnd = target[target.length - 1];

  if (!drawnStart || !drawnEnd || !targetStart || !targetEnd || path.length < 2) {
    return reject('too-short', 0, Number.POSITIVE_INFINITY);
  }

  if (pathLength(path) < pathLength(target) * MIN_LENGTH_RATIO) {
    return reject('too-short', 0, meanDeviation(path, target));
  }

  const deviation = meanDeviation(path, target);

  // Checked before the start point, and deliberately: a backwards stroke also
  // starts in the wrong place, and "start from the dot" is the wrong advice
  // for someone who traced the shape correctly in reverse.
  if (runsBackwards(path, target)) {
    return reject('backwards', scoreFor(deviation), deviation);
  }

  if (distance(drawnStart, targetStart) > START_TOLERANCE) {
    return reject('wrong-start', scoreFor(deviation), deviation);
  }

  if (distance(drawnEnd, targetEnd) > END_TOLERANCE) {
    return reject('too-short', scoreFor(deviation), deviation);
  }

  if (deviation > PATH_TOLERANCE) {
    return reject('off-path', scoreFor(deviation), deviation);
  }

  return {
    ok: true,
    message: praise(deviation),
    score: scoreFor(deviation),
    deviation,
  };
}

function reject(
  reason: StrokeRejection,
  score: number,
  deviation: number
): StrokeAssessment {
  return { ok: false, reason, message: MESSAGES[reason], score, deviation };
}

function scoreFor(deviation: number): number {
  if (!Number.isFinite(deviation)) return 0;
  return Math.max(0, Math.min(1, 1 - deviation / ZERO_SCORE_DEVIATION));
}

function praise(deviation: number): string {
  if (deviation < PATH_TOLERANCE / 3) return 'Right on the line.';
  if (deviation < (PATH_TOLERANCE * 2) / 3) return 'Good stroke.';
  return 'Close enough — a little loose.';
}
