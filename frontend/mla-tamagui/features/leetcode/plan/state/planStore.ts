import { Logger } from '@lib/logging';
import type { PlanStep, PlanStepOrigin } from '@lib/voice-edit-dsl';
import { splitBlock } from '@lib/voice-edit-dsl';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

/**
 * Voice/typed plan for a problem: an ordered list of steps that may later be
 * "realised" into the code buffer one by one. A step is deliberately small --
 * one sentence -- so a realise is one fenced edit, and the user can reorder,
 * split, or merge steps while the plan is still pending.
 */

const stepStatusSchema = z.enum(['pending', 'realized', 'stale']);

const planStepSchema = z.object({
  id: z.string().min(1),
  text: z.string(),
  status: stepStatusSchema,
  lines: z.object({ startLine: z.number(), endLine: z.number() }).nullable(),
  // Plans persisted before steps carried an origin read back as typed ones.
  origin: z.enum(['voice', 'text']).default('text'),
});

const persistedSchema = z.object({
  plans: z.record(z.string(), z.array(planStepSchema)),
});

export type PlanState = {
  plans: Record<string, PlanStep[]>;
  addBlock: (problemId: string, text: string, origin?: PlanStepOrigin) => void;
  setStepText: (problemId: string, id: string, text: string) => void;
  splitStep: (problemId: string, id: string) => void;
  mergeWithNext: (problemId: string, id: string) => void;
  moveStep: (problemId: string, id: string, direction: -1 | 1) => void;
  removeStep: (problemId: string, id: string) => void;
  markRealized: (
    problemId: string,
    id: string,
    lines: { startLine: number; endLine: number }
  ) => void;
  markStaleFrom: (problemId: string, index: number) => void;
  clearPlan: (problemId: string) => void;
  stepsFor: (problemId: string) => PlanStep[];
};

function createId(): string {
  return `p${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

function newStep(text: string, origin: PlanStepOrigin = 'text'): PlanStep {
  return { id: createId(), text, status: 'pending', lines: null, origin };
}

function updateSteps(
  set: (fn: (state: PlanState) => Partial<PlanState>) => void,
  get: () => PlanState,
  problemId: string,
  fn: (steps: PlanStep[]) => PlanStep[]
): void {
  set((state) => ({
    plans: { ...state.plans, [problemId]: fn(state.plans[problemId] ?? []) },
  }));
}

export const usePlanStore = create<PlanState>()(
  persist(
    (set, get) => ({
      plans: {},

      addBlock: (problemId, text, origin = 'text') => {
        // One utterance, one step: talking freely is the point, and splitting
        // is a tap the user makes afterwards on the step they want split.
        const block = text.trim();
        if (!problemId || block.length === 0) return;
        Logger.info('leetcode.plan.add_block', { problemId, origin });
        updateSteps(set, get, problemId, (steps) => [
          ...steps,
          newStep(block, origin),
        ]);
      },

      setStepText: (problemId, id, text) => {
        updateSteps(set, get, problemId, (steps) =>
          steps.map((s) => (s.id === id ? { ...s, text } : s))
        );
      },

      splitStep: (problemId, id) => {
        updateSteps(set, get, problemId, (steps) => {
          const index = steps.findIndex((s) => s.id === id);
          if (index === -1) return steps;
          const parts = splitBlock(steps[index].text);
          if (parts.length < 2) return steps;
          const splitSteps = parts.map((part) =>
            newStep(part, steps[index].origin)
          );
          // Keep the original id on the first fragment so callers holding it
          // (e.g. a highlighted step) stay pointed at the same sentence.
          splitSteps[0] = { ...splitSteps[0], id };
          return [
            ...steps.slice(0, index),
            ...splitSteps,
            ...steps.slice(index + 1),
          ];
        });
      },

      mergeWithNext: (problemId, id) => {
        updateSteps(set, get, problemId, (steps) => {
          const index = steps.findIndex((s) => s.id === id);
          if (index === -1 || index === steps.length - 1) return steps;
          const next = steps[index + 1];
          return [
            ...steps.slice(0, index),
            { ...steps[index], text: `${steps[index].text} ${next.text}` },
            ...steps.slice(index + 2),
          ];
        });
      },

      moveStep: (problemId, id, direction) => {
        updateSteps(set, get, problemId, (steps) => {
          const index = steps.findIndex((s) => s.id === id);
          const target = index + direction;
          if (index === -1 || target < 0 || target >= steps.length) {
            return steps;
          }
          const moved = [...steps];
          [moved[index], moved[target]] = [moved[target], moved[index]];
          return moved;
        });
      },

      removeStep: (problemId, id) => {
        updateSteps(set, get, problemId, (steps) =>
          steps.filter((s) => s.id !== id)
        );
      },

      markRealized: (problemId, id, lines) => {
        Logger.info('leetcode.plan.mark_realized', { problemId, id, lines });
        updateSteps(set, get, problemId, (steps) =>
          steps.map((s) =>
            s.id === id ? { ...s, status: 'realized', lines } : s
          )
        );
      },

      markStaleFrom: (problemId, index) => {
        updateSteps(set, get, problemId, (steps) =>
          steps.map((s, i) =>
            i >= index && s.status === 'realized'
              ? { ...s, status: 'stale' }
              : s
          )
        );
      },

      clearPlan: (problemId) => {
        set((state) => {
          const plans = { ...state.plans };
          delete plans[problemId];
          return { plans };
        });
      },

      stepsFor: (problemId) => get().plans[problemId] ?? [],
    }),
    {
      name: 'leetcode-plan',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({ plans: state.plans }),
      // Corrupt or out-of-date storage falls back to empty plans rather than
      // crashing the Plan tab.
      merge: (persisted, current) => {
        const parsed = persistedSchema.safeParse(persisted);
        if (!parsed.success) {
          // Nothing stored (a fresh install) arrives as undefined: not a rejection.
          if (persisted != null) {
            Logger.warn('leetcode.plan.rehydrate_rejected', {
              issues: parsed.error?.issues.length ?? 0,
            });
          }
          return { ...current, plans: {} };
        }
        return { ...current, plans: parsed.data.plans };
      },
      onRehydrateStorage: () => (_state, err) => {
        if (err)
          Logger.withError(err).error('leetcode.plan.rehydrate_failed');
      },
    }
  )
);
