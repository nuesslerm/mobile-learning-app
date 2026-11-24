import type { EditorClient, EditorErrorCode, Revision } from '@lib/code-editor-api';

import type { BlockModel } from './blockModel';
import { labelOf, stepOf } from './blockStep';
import type { DropPlan } from './dropPlan';

/** How long each parse of a drop may take: the engine's own default is made for one keystroke, and a drop parses the text twice. */
export const DROP_BUDGET_MS = 1000;

export type DropOutcome =
  | { readonly kind: 'moved'; readonly revision: Revision }
  /** the engine found nothing to change */
  | { readonly kind: 'noop' }
  /** the text is not the one the rows were drawn from: nothing was applied and the drop is not tried again */
  | { readonly kind: 'outdated'; readonly code: EditorErrorCode }
  /** the engine would not make the edit: nothing was applied; `reason` is the not-a-node reason (`damaged`, `shares-line`) or the error code */
  | { readonly kind: 'refused'; readonly reason: string }
  | { readonly kind: 'unavailable'; readonly code: EditorErrorCode };

const OUTDATED: ReadonlySet<EditorErrorCode> = new Set<EditorErrorCode>(['stale', 'structure-changed', 'instance-mismatch', 'range-lost', 'text-mismatch', 'out-of-range']);
const UNAVAILABLE: ReadonlySet<EditorErrorCode> = new Set<EditorErrorCode>(['not-ready', 'transport']);

/** One call, made at the revision and instance of the tree the rows were drawn from: the engine refuses it as stale rather than mapping it across what was typed since. */
export async function applyDrop(client: EditorClient, model: BlockModel, plan: DropPlan): Promise<DropOutcome> {
  const step = stepOf(plan, model);
  if (!step) return { kind: 'noop' };
  const reply = await client.transact([step], { at: model.revision, instance: model.instance, label: labelOf(step), budgetMs: DROP_BUDGET_MS });
  if (reply.ok) return reply.value.changes.length === 0 ? { kind: 'noop' } : { kind: 'moved', revision: reply.revision };
  const { error } = reply;
  if (OUTDATED.has(error.code)) return { kind: 'outdated', code: error.code };
  if (UNAVAILABLE.has(error.code)) return { kind: 'unavailable', code: error.code };
  return { kind: 'refused', reason: error.code === 'not-a-node' ? (error.reason ?? 'not-a-node') : error.code };
}

const REFUSALS: Readonly<Record<string, string>> = {
  damaged: 'That part of the code has a syntax error, so it stays text.',
  'shares-line': 'Another statement is on that line, so it stays text.',
  'no-statement': 'There is no statement there to use.',
  'not-in-block': 'That statement is not in a block.',
  'would-not-parse': 'That would break the code, so nothing changed.',
  'parse-timeout': 'The code took too long to read. Try again.',
  unsupported: 'This editor cannot move blocks.',
};

/** The line the view shows for an outcome, or null when there is nothing to say (the text moved, or there was nothing to move). */
export function describeOutcome(outcome: DropOutcome): string | null {
  switch (outcome.kind) {
    case 'moved':
    case 'noop':
      return null;
    case 'outdated':
      return 'The code changed, so the blocks were redrawn. Try again.';
    case 'unavailable':
      return 'The editor is not ready.';
    case 'refused':
      return REFUSALS[outcome.reason] ?? 'That could not be done.';
  }
}
