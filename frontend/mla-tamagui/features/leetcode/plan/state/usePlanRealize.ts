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

import { VoiceEditTransportError, type ClientEditStat } from '../../data/voiceEditTransport';
import { useLeetCodePorts } from '../../di/LeetCodeContainer';
import { usePlanStore } from '../state/planStore';

import {
  compactVoiceError,
  errorStatus,
  newEditId,
} from '../../data/voiceEditIdentity';

export type PlanRealizeOutcome = 'applied' | 'invalid' | 'error';

export interface PlanRealizeArgs {
  problemId: string;
  language: string;
  getCode: () => string;
  applyEdit: (alt: EditAlternative) => { modifiedText: string };
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
  getCode,
  applyEdit,
}: PlanRealizeArgs): PlanRealizeResult {
  const { voice } = useLeetCodePorts();
  const [busyStepId, setBusyStepId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const realize = useCallback(
    async (stepId: string): Promise<PlanRealizeOutcome> => {
      const code = getCode();
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

        const { modifiedText } = applyEdit(applicable);
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
    [applyEdit, getCode, language, problemId, voice]
  );

  return { realize, busyStepId, error };
}
