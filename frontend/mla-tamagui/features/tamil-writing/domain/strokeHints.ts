import type { Point } from './strokes';

/**
 * A sentence describing a stroke, read off the stroke itself.
 *
 * The first version of this course had hand-written hints beside hand-drawn
 * paths, and both were wrong in the same direction with nothing to catch it.
 * Now the paths come from the font, so the words come from the paths: a hint
 * cannot describe a stroke the learner is not being shown.
 *
 * It reads like a person wrote it because that is what a learner needs, but
 * every clause is measured -- where it begins, where it goes, and which way it
 * bends.
 */

export function describeStroke(path: readonly Point[]): string {
  const start = path[0];
  const end = path[path.length - 1];
  if (!start || !end || path.length < 2) return 'Touch the pad at the dot.';

  const span = Math.hypot(end.x - start.x, end.y - start.y);
  // A stroke that comes back to where it started is a loop, and "down and to
  // the right" would be a lie about it.
  if (span < 0.08) {
    return `From ${where(start)}, round and back to where you began.`;
  }

  const bend = curvature(path);
  const turn =
    Math.abs(bend) < 0.04
      ? ''
      : bend > 0
        ? ', curving clockwise'
        : ', curving anticlockwise';

  return `From ${where(start)}, ${heading(start, end)}${turn}.`;
}

function where(point: Point): string {
  const row = point.y < 0.38 ? 'the top' : point.y > 0.62 ? 'the bottom' : 'the middle';
  const column =
    point.x < 0.38 ? 'left' : point.x > 0.62 ? 'right' : 'centre';
  if (row === 'the middle' && column === 'centre') return 'the centre';
  if (column === 'centre') return `${row} centre`;
  return `${row} ${column}`;
}

function heading(start: Point, end: Point): string {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const vertical = Math.abs(dy) > Math.abs(dx) * 1.6;
  const horizontal = Math.abs(dx) > Math.abs(dy) * 1.6;

  if (vertical) return dy > 0 ? 'straight down' : 'straight up';
  if (horizontal) return dx > 0 ? 'across to the right' : 'back to the left';
  const down = dy > 0 ? 'down' : 'up';
  const side = dx > 0 ? 'right' : 'left';
  return `${down} and to the ${side}`;
}

/**
 * Signed mean turn per step: positive bends clockwise on screen (y downward),
 * negative anticlockwise. Near zero means a straight stroke.
 */
function curvature(path: readonly Point[]): number {
  let total = 0;
  let turns = 0;
  for (let i = 2; i < path.length; i += 1) {
    const a = path[i - 2];
    const b = path[i - 1];
    const c = path[i];
    if (!a || !b || !c) continue;
    const incoming = Math.atan2(b.y - a.y, b.x - a.x);
    const outgoing = Math.atan2(c.y - b.y, c.x - b.x);
    let delta = outgoing - incoming;
    while (delta > Math.PI) delta -= Math.PI * 2;
    while (delta < -Math.PI) delta += Math.PI * 2;
    total += delta;
    turns += 1;
  }
  return turns === 0 ? 0 : total / turns;
}
