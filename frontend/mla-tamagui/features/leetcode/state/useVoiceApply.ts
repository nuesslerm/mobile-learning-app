import { Logger } from '@lib/logging';
import type { EditAlternative } from '@lib/voice-edit-dsl';
import { useCallback } from 'react';

import { planVoiceEdit, type EditBasis, type EditOutcome, type EditRefusal, type EditTarget } from '../domain/editor/voiceEditPlan';
import { EDITOR_NOT_READY, type UseVoiceEditResult } from './useVoiceEdit';

export const STALE_EDIT = 'The code changed while the edit was being planned. Say it again.';

const REFUSAL_COPY: Record<EditRefusal, string> = {
  stale: STALE_EDIT,
  'not-ready': EDITOR_NOT_READY,
  'invalid-plan': 'The editor could not apply the edit.',
  failed: 'The editor could not apply the edit.',
};

type Applying = Pick<UseVoiceEditResult, 'state' | 'markApplied' | 'dismiss' | 'fail'>;

export type VoiceApply = {
  /** Applies an alternative of the turn at the basis its plan was made for, then closes the turn; a refusal ends it with the banner. */
  apply: (alt: EditAlternative) => Promise<EditOutcome>;
  /** The same plan and apply without the turn, for a caller that holds its own basis (the plan tab). */
  applyAt: (alt: EditAlternative, basis: EditBasis, label: string) => Promise<EditOutcome>;
};

/**
 * An edit applies at the revision its plan was made for: a keystroke since then refuses it with nothing applied and no undo step, so
 * the person says it again instead of getting an edit planned for text that has moved. History keeps the text that was replaced,
 * and only for an edit that landed.
 */
export function useVoiceApply({
  target,
  voiceEdit,
  snapshot,
  problemId,
}: {
  target: EditTarget;
  voiceEdit: Applying;
  snapshot: (reason: 'voice-edit', code: string) => void;
  problemId?: string | null;
}): VoiceApply {
  const applyAt = useCallback(
    async (alt: EditAlternative, basis: EditBasis, label: string): Promise<EditOutcome> => {
      const planned = planVoiceEdit(basis, alt, label);
      const outcome = planned.ok ? await target.applyEdit(planned.edit) : planned;
      if (outcome.ok) snapshot('voice-edit', basis.text);
      else Logger.info('editor_v2.voice_apply.refused', { reason: outcome.reason, code: outcome.code, message: outcome.message, label, problemId });
      return outcome;
    },
    [target, snapshot, problemId]
  );

  const apply = useCallback(
    async (alt: EditAlternative): Promise<EditOutcome> => {
      const { basis, phase, alternatives } = voiceEdit.state;
      // The commands themselves, because "the edit came out wrong" has been undiagnosable from this log more than once: the planner is
      // nondeterministic, so the offending command cannot be reproduced by asking it again. Offsets are what matter; the text is the
      // user's code and is logged as a length.
      Logger.info('editor_v2.voice_apply', {
        alternativeId: alt.id,
        confidence: alt.confidence,
        problemId,
        codeLength: basis?.text.length,
        commands: alt.commands.map((c) =>
          Object.fromEntries(Object.entries(c).map(([k, v]) => (typeof v === 'string' && k !== 'type' ? [`${k}Length`, v.length] : [k, v])))
        ),
      });
      const outcome: EditOutcome = basis ? await applyAt(alt, basis, 'voice-edit') : { ok: false, reason: 'not-ready', code: 'no-basis' };
      if (!outcome.ok) {
        voiceEdit.fail(REFUSAL_COPY[outcome.reason]);
        return outcome;
      }
      const pickedIndex = phase === 'choosing' ? alternatives.findIndex((a) => a.id === alt.id) : -1;
      voiceEdit.markApplied({ ...(pickedIndex >= 0 ? { pickedIndex } : {}), codeAfter: outcome.modifiedText });
      voiceEdit.dismiss();
      return outcome;
    },
    [applyAt, voiceEdit, problemId]
  );

  return { apply, applyAt };
}
