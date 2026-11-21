import type { TreeSnapshot } from '../ports/host';

export type TreeJobClock = {
  now(): number;
  /** lets the page's other events (typing, replies) run before the next slice */
  yieldToEvents(): Promise<void>;
  /** the engine's head revision: a document that moved under the parse ends the job */
  head(): number;
  /** the longest one slice of parsing may take */
  readonly sliceMs: number;
};

export type TreeJobResult = { readonly complete: boolean; readonly parsedTo: number };

/**
 * Parses towards `upto` in slices of at most `sliceMs`, giving the page's events a turn between slices, until the tree covers `upto`,
 * the budget is spent, or the document moved. The first slice runs at once, so a small file costs no turn at all; the rest is paced.
 * It never fails for lack of time: it returns what it reached, with `complete: false`.
 */
export async function runTreeJob(source: TreeSnapshot, upto: number, budgetMs: number, clock: TreeJobClock): Promise<TreeJobResult> {
  const base = clock.head();
  const deadline = clock.now() + budgetMs;
  let done = source.work(upto, Math.min(clock.sliceMs, budgetMs));
  while (!done && clock.now() < deadline && clock.head() === base) {
    await clock.yieldToEvents();
    if (clock.head() !== base) break;
    done = source.work(upto, Math.min(clock.sliceMs, Math.max(0, deadline - clock.now())));
  }
  const parsedTo = source.parsedTo();
  return { complete: done || parsedTo >= upto, parsedTo };
}
