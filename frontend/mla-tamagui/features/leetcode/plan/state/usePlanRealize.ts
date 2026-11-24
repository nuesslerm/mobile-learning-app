// Off by default, the code stays wired: see domain/featureFlags.ts.
import { Logger } from '@lib/logging';
import {
  buildStepRealizeRequest,
  linesProduced,
  planAnchor,
  restrictAlternativeToLines,
  validateAlternative,
  type EditAlternative,
} from '@lib/voice-edit-dsl';
import { useCallback, useState } from 'react';
import { Platform } from 'react-native';

import { VoiceEditTransportError } from '../../domain/ports';
import type { ClientEditStat } from '@lib/voice-edit-dsl';
import type { EditBasis, EditOutcome } from '../../domain/editor/voiceEditPlan';
import { useLeetCodePorts } from '../../state/ports';
import { usePlanStore } from '../state/planStore';

import {
  compactVoiceError,
  errorStatus,
  newEditId,
} from '../../domain/voiceEditIdentity';

export type PlanRealizeOutcome = 'applied' | 'invalid' | 'error';

/** The editor beside the plan tab: read for the plan, and applied to at the revision that read made. */
export interface PlanEditor {
  language: string;
  basis: () => Promise<EditBasis | null>;
  applyEdit: (alt: EditAlternative, basis: EditBasis) => Promise<EditOutcome>;
}

export interface PlanRealizeArgs extends PlanEditor {
  problemId: string;
}

const EDITOR_NOT_READY = 'The editor is not ready. Try again.';
const STEP_STALE = 'The code changed while the step was being planned. Realize it again.';
const STEP_NOT_APPLIED = 'The editor could not apply the step.';

function refusalCopy(outcome: Extract<EditOutcome, { ok: false }>): string {
  if (outcome.reason === 'stale') return STEP_STALE;
  return outcome.reason === 'not-ready' ? EDITOR_NOT_READY : STEP_NOT_APPLIED;
}

export interface PlanRealizeResult {
  realize: (stepId: string) => Promise<PlanRealizeOutcome>;
  busyStepId: string | null;
  error: string | null;
}

/**
 * Realises one plan step: fences the planner to the step's anchor, applies the
 * first valid answer through the caller's editor, and marks the step and its
 * successors in the plan store. One stats row per realise, best-effort.
 */
export function usePlanRealize({
  problemId,
  language,
  basis: readBasis,
  applyEdit,
}: PlanRealizeArgs): PlanRealizeResult {
  const { voice } = useLeetCodePorts();
  const [busyStepId, setBusyStepId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const realize = useCallback(
    async (stepId: string): Promise<PlanRealizeOutcome> => {
      // read per step: two steps realised in a row each plan against the editor's text, not a copy that lags
      const basis = await readBasis();
      if (!basis) {
        Logger.info('leetcode.plan.realize.editor_unavailable', { problemId, stepId });
        setError(EDITOR_NOT_READY);
        return 'error';
      }
      const code = basis.text;
      const steps = usePlanStore.getState().stepsFor(problemId);
      const index = steps.findIndex((step) => step.id === stepId);
      const step = steps[index];
      if (index === -1 || !step) {
        const message = 'Plan step not found.';
        Logger.warn('leetcode.plan.realize.step_missing', {
          problemId,
          stepId,
        });
        setError(message);
        return 'error';
      }

      const anchor = planAnchor(code, steps, index);
      const request = buildStepRealizeRequest(code, language, step, anchor);
      const editId = newEditId();
      // A step realises as what it was spoken or typed as, so the stats table
      // can tell the two paths apart.
      const source = step.origin === 'voice' ? 'plan-voice' : 'plan-text';
      const stats: ClientEditStat = {
        editId,
        source,
        problemId,
        platform: Platform.OS,
        transcribeWallMs: null,
        planWallMs: null,
        postConfirmWaitMs: null,
        speculativeHit: false,
        localFastPath: false,
        outcome: 'error',
        pickedIndex: null,
        alternatives: 0,
        transcriptChars: request.transcript.length,
        codeCharsBefore: code.length,
        codeCharsAfter: null,
        error: null,
      };

      setBusyStepId(stepId);
      setError(null);
      Logger.info('leetcode.plan.realize.start', {
        problemId,
        stepId,
        anchorKind: anchor.kind,
        anchorLine: anchor.line,
      });

      try {
        const response = await voice.plan({
          transcript: request.transcript,
          currentCode: code,
          language,
          source,
          editId,
        });
        stats.planWallMs = response.wallMs;
        stats.alternatives = response.alternatives.length;
        Logger.info('leetcode.plan.realize.response', {
          problemId,
          stepId,
          alternatives: response.alternatives.length,
          wallMs: response.wallMs,
        });

        const fence = { startLine: anchor.line, endLine: anchor.line };
        const mode =
          anchor.kind === 'insertAfterLine' ? 'insertAfter' : 'replace';
        const alternatives = response.alternatives.map((alt) =>
          restrictAlternativeToLines(
            validateAlternative(alt, code),
            code,
            fence,
            {
              mode,
            }
          )
        );
        const applicable = alternatives.find((alt) => alt.valid !== false);
        if (!applicable) {
          const reason = compactVoiceError(
            alternatives.find((alt) => alt.invalidReason)?.invalidReason ??
              'No valid alternative for this step.'
          );
          stats.error = reason;
          void voice.sendEditStats(stats);
          Logger.info('leetcode.plan.realize.invalid', {
            problemId,
            stepId,
            reason,
            editId,
          });
          setError(reason);
          return 'invalid';
        }

        const outcome = await applyEdit(applicable, basis);
        if (!outcome.ok) {
          const reason = refusalCopy(outcome);
          stats.error = reason;
          void voice.sendEditStats(stats);
          Logger.info('leetcode.plan.realize.refused', {
            problemId,
            stepId,
            reason: outcome.reason,
            code: outcome.code,
            editId,
          });
          setError(reason);
          return 'error';
        }
        const { modifiedText } = outcome;
        const lines = linesProduced(code, modifiedText, anchor);
        if (!lines) {
          const reason = 'The applied edit produced no new lines.';
          stats.error = reason;
          void voice.sendEditStats(stats);
          Logger.warn('leetcode.plan.realize.no_lines', {
            problemId,
            stepId,
            anchorKind: anchor.kind,
            editId,
          });
          setError(reason);
          return 'invalid';
        }

        usePlanStore.getState().markRealized(problemId, stepId, lines);
        usePlanStore.getState().markStaleFrom(problemId, index + 1);
        stats.outcome = 'auto';
        stats.codeCharsAfter = modifiedText.length;
        void voice.sendEditStats(stats);
        Logger.info('leetcode.plan.realize.applied', {
          problemId,
          stepId,
          startLine: lines.startLine,
          endLine: lines.endLine,
        });
        return 'applied';
      } catch (err) {
        const message = compactVoiceError(
          err instanceof VoiceEditTransportError
            ? `Server error (${err.status}): ${err.message}`
            : err instanceof Error
              ? err.message
              : String(err)
        );
        stats.error = message;
        void voice.sendEditStats(stats);
        Logger.withError(err)
          .withMetadata({
            problemId,
            stepId,
            editId,
            status: errorStatus(err),
          })
          .error('leetcode.plan.realize.error');
        setError(message);
        return 'error';
      } finally {
        setBusyStepId(null);
      }
    },
    [applyEdit, readBasis, language, problemId, voice]
  );

  return { realize, busyStepId, error };
}
