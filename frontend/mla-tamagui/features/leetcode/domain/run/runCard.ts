import { customOutcomeOf } from './customOutcome';
import type { ErrorLocation, RunResponse } from './execution';

/** What the card on screen shows: a run of the bundled cases, or of one case the learner typed (with the inputs it ran on). */
export type RunCard = { mode: 'tests'; response: RunResponse } | { mode: 'custom'; response: RunResponse; inputs: Record<string, unknown> };

/** The line the run reported, when it did: the response's for the bundled cases, the typed case's own row for a custom card. */
export function errorLocationOf(card: RunCard): ErrorLocation | undefined {
  return card.mode === 'custom' ? customOutcomeOf(card.response, card.inputs).errorAt : card.response.error_at;
}
