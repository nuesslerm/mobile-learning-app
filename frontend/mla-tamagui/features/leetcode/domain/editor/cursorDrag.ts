/**
 * The drag mapping of the cursor handle: where a finger's displacement from the grab puts the caret.
 *
 * The target is a function of the displacement and of the axis the gesture locked to, never integrated over velocity, so there is
 * no clock here and a drag is reversible: dragging back to where it started gives the caret it started with.
 */

import { columnOffset, stepChars, stepLines, stepWords } from './cursorMotion';

export type Tuning = {
  /** pixels of sideways travel per character inside the character zone */
  readonly CHAR_PX: number;
  /** how many characters of travel the character zone holds; sideways travel beyond it is words */
  readonly CHAR_ZONE: number;
  /** pixels of sideways travel per word beyond the character zone */
  readonly WORD_PX: number;
  /** pixels of vertical travel per line */
  readonly LINE_PX: number;
  /** the distance from the grab inside which nothing moves and the axis is not yet chosen */
  readonly SLOP_PX: number;
};

export const DEFAULT_TUNING: Tuning = { CHAR_PX: 10, CHAR_ZONE: 12, WORD_PX: 36, LINE_PX: 22, SLOP_PX: 8 };

/** the first lines of vertical travel go one at a time, then this many lines per step of travel */
const SLOW_LINES = 6;
const FAST_LINES = 3;

export type Axis = 'x' | 'y';
export type Delta = { readonly dx: number; readonly dy: number };
/** the selection at the grab, over the editor's own text */
export type CaretBase = { readonly text: string; readonly anchor: number; readonly head: number };
/** where the selection goes: the anchor stays where the grab found it, unless the grab was a caret, which moves whole */
export type CaretTarget = { readonly anchor: number; readonly head: number };

export type CaretGesture = {
  /** null while the finger is inside the slop and the axis is not chosen; after that always a target */
  move(delta: Delta): CaretTarget | null;
  axis(): Axis | null;
};

/** Whether a displacement is still inside the dead zone around the grab. */
export function insideSlop({ dx, dy }: Delta, tuning: Tuning = DEFAULT_TUNING): boolean {
  return Math.hypot(dx, dy) < tuning.SLOP_PX;
}

function lineSteps(travel: number, tuning: Tuning): number {
  const lines = Math.floor(travel / tuning.LINE_PX);
  return lines <= SLOW_LINES ? lines : SLOW_LINES + FAST_LINES * (lines - SLOW_LINES);
}

function sidewaysHead(base: CaretBase, dx: number, tuning: Tuning): number {
  const sign = dx < 0 ? -1 : 1;
  const travel = Math.abs(dx);
  const zone = tuning.CHAR_ZONE * tuning.CHAR_PX;
  const inZone = stepChars(base.text, base.head, sign * Math.min(tuning.CHAR_ZONE, Math.floor(travel / tuning.CHAR_PX)));
  const words = travel > zone ? Math.floor((travel - zone) / tuning.WORD_PX) : 0;
  return words === 0 ? inZone : stepWords(base.text, inZone, sign * words);
}

/** One gesture over the text read at the grab. With a selection at the grab the head moves and the anchor stays. */
export function createCaretGesture(base: CaretBase, tuning: Tuning = DEFAULT_TUNING): CaretGesture {
  let locked: Axis | null = null;
  const goalColumn = columnOffset(base.text, base.head);
  const collapsed = base.anchor === base.head;
  return {
    axis: () => locked,
    move({ dx, dy }) {
      if (locked === null) {
        if (insideSlop({ dx, dy }, tuning)) return null;
        locked = Math.abs(dx) >= Math.abs(dy) ? 'x' : 'y';
      }
      const head =
        locked === 'x'
          ? sidewaysHead(base, dx, tuning)
          : stepLines(base.text, base.head, (dy < 0 ? -1 : 1) * lineSteps(Math.abs(dy), tuning), goalColumn);
      return { anchor: collapsed ? head : base.anchor, head };
    },
  };
}
