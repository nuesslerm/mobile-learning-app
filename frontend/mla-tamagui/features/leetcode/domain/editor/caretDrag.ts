import type { EditorClient, Selection as ApiSelection } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';

import { createCaretGesture, DEFAULT_TUNING, insideSlop, type CaretGesture, type CaretTarget, type Delta, type Tuning } from './cursorDrag';
import { moveBy, type Motion } from './cursorMotion';
import { createLatestWins, type LatestWins } from './latestWins';
import type { Selection } from './types';

export type CaretDrag = {
  /** a finger sample: the displacement from where the finger grabbed */
  move(delta: Delta): void;
  /** resolves with the selection the editor holds once every call has landed; null when nothing moved or the editor could not be read */
  end(): Promise<Selection | null>;
  /** drops what waits and makes no further call (the screen went away) */
  cancel(): void;
};

type Grab = { readonly gesture: CaretGesture; readonly latest: LatestWins<CaretTarget> };

const mainRange = (selection: ApiSelection) => selection.ranges[selection.main] ?? { anchor: 0, head: 0 };
const ordered = (selection: ApiSelection): Selection => {
  const { anchor, head } = mainRange(selection);
  return { start: Math.min(anchor, head), end: Math.max(anchor, head) };
};
const sameTarget = (a: CaretTarget, b: CaretTarget) => a.anchor === b.anchor && a.head === b.head;
const toHead = (target: CaretTarget) => (target.head === target.anchor ? undefined : target.head);

/**
 * A caret drag over the typed editor API. `focus` goes out at the grab; the editor's own text, selection and revision are read once, at
 * the first sample past the slop, and every select is made at that revision, so a keystroke typed meanwhile makes the next one stale
 * and ends the drag. One select is in flight at a time and carries the newest target (latest-wins).
 */
export function createCaretDrag(client: EditorClient, tuning: Tuning = DEFAULT_TUNING): CaretDrag {
  let moves = 0;
  let calls = 0;
  let outcome = 'tap';
  let stopped = false;
  let cancelled = false;
  let landed: Selection | null = null;
  let reading: Promise<void> | null = null;
  let grab: Grab | null = null;
  let finished: Promise<Selection | null> | null = null;
  const buffered: Delta[] = [];

  void client.focus();

  const stop = (why: string) => {
    stopped = true;
    outcome = why;
  };

  const select = (revision: number) => async (target: CaretTarget) => {
    calls += 1;
    const reply = await client.select(target.anchor, toHead(target), { at: revision, reveal: true });
    if (reply.ok) {
      landed = ordered(reply.value.selection);
      outcome = 'moved';
      return;
    }
    stop(reply.error.code);
    grab?.latest.cancel();
  };

  const push = (current: Grab, delta: Delta) => {
    const target = current.gesture.move(delta);
    if (target) current.latest.push(target);
  };

  const read = async () => {
    const reply = await client.getText();
    if (!reply.ok) {
      stop(reply.error.code);
      return;
    }
    if (stopped) return;
    const range = mainRange(reply.value.selection);
    grab = {
      gesture: createCaretGesture({ text: reply.value.text, anchor: range.anchor, head: range.head }, tuning),
      latest: createLatestWins(select(reply.revision as number), { equal: sameTarget }),
    };
    for (const delta of buffered.splice(0)) push(grab, delta);
  };

  return {
    move(delta) {
      if (stopped || finished) return;
      moves += 1;
      if (grab) {
        push(grab, delta);
        return;
      }
      buffered.push(delta);
      if (!reading && !insideSlop(delta, tuning)) reading = read();
    },
    end() {
      finished ??= (async () => {
        await reading;
        await grab?.latest.settled();
        Logger.info('editor.caret_drag', { moves, calls, outcome });
        if (cancelled || !grab) return null;
        if (!stopped) return landed;
        const fresh = await client.getText();
        return fresh.ok ? ordered(fresh.value.selection) : null;
      })();
      return finished;
    },
    cancel() {
      if (cancelled) return;
      cancelled = true;
      stop('cancelled');
      buffered.length = 0;
      grab?.latest.cancel();
    },
  };
}

/** One motion from the editor's own caret, as an accessibility action asks for it: the head moves and an anchor that is not the head stays. */
export async function moveCaretBy(client: EditorClient, motion: Motion): Promise<Selection | null> {
  void client.focus();
  const read = await client.getText();
  if (!read.ok) {
    Logger.info('editor.caret_move', { unit: motion.unit, count: motion.count, outcome: read.error.code });
    return null;
  }
  const { anchor, head } = mainRange(read.value.selection);
  const to = moveBy(read.value.text, head, motion);
  const target: CaretTarget = { anchor: anchor === head ? to : anchor, head: to };
  const reply = await client.select(target.anchor, toHead(target), { at: read.revision as number, reveal: true });
  Logger.info('editor.caret_move', { unit: motion.unit, count: motion.count, outcome: reply.ok ? 'moved' : reply.error.code });
  return reply.ok ? ordered(reply.value.selection) : null;
}
