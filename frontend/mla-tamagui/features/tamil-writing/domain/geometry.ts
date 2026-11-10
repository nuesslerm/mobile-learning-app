import type { Point } from './strokes';

/**
 * Path simplification, the one piece of the old skeleton pipeline still in
 * use.
 *
 * Deriving stroke paths from the letter's shape lived here -- thinning,
 * tracing, chaining -- and is gone: the shape cannot say where a pen started
 * or when it lifted, and every attempt to guess produced counts that were
 * plainly wrong. That work is now `scripts/tamil-strokes/extract-upenn-strokes.py`,
 * which reads it off real writing animations. What stays in TypeScript is the
 * measuring (`glyphGeometry.ts`) and this.
 */

/** Drops the jitter a pixel walk leaves, keeping the shape. */
export function simplify(path: readonly Point[], tolerance: number): Point[] {
  if (path.length <= 2) return [...path];
  const first = path[0];
  const last = path[path.length - 1];
  if (!first || !last) return [...path];

  let worst = 0;
  let worstIndex = 0;
  for (let i = 1; i < path.length - 1; i += 1) {
    const point = path[i];
    if (!point) continue;
    const deviation = perpendicularDistance(point, first, last);
    if (deviation > worst) {
      worst = deviation;
      worstIndex = i;
    }
  }

  if (worst <= tolerance) return [first, last];
  return [
    ...simplify(path.slice(0, worstIndex + 1), tolerance).slice(0, -1),
    ...simplify(path.slice(worstIndex), tolerance),
  ];
}

function perpendicularDistance(point: Point, a: Point, b: Point): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const span = Math.hypot(dx, dy);
  if (span === 0) return Math.hypot(point.x - a.x, point.y - a.y);
  return Math.abs(dy * point.x - dx * point.y + b.x * a.y - b.y * a.x) / span;
}

/**
 * Joins ridges that are one pen stroke cut into pieces.
 *
 * Thinning splits a letter wherever ridges meet, so a single stroke through a
 * junction comes back as two or three fragments. A hand does not lift there.
 * Two fragments are rejoined when their ends are close AND the second carries
 * on in roughly the direction the first was going -- the turn test is what
 * stops a T-junction from being welded into a hairpin.
 */
