import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

/**
 * Version history for a problem's code buffer.
 *
 * Before this store the editor buffer was plain `useState` and nothing else:
 * it died on unmount, a language switch overwrote it outright, and Reset
 * replaced it with the starter code -- which is why ResetCodeDialog says "This
 * cannot be undone." It said that because it was true.
 *
 * Snapshots are taken at the moments a buffer is about to be replaced or has
 * just been transformed, not on every keystroke: a keystroke-level history is
 * CodeMirror's undo stack, which already exists inside the WebView. This is the
 * coarse, durable layer above it -- the one that survives leaving the screen.
 */

const SNAPSHOT_REASONS = [
  'format',
  'run',
  'reset',
  'language-switch',
  'voice-edit',
  'navigate-away',
] as const;

export type SnapshotReason = (typeof SNAPSHOT_REASONS)[number];

export type CodeSnapshot = {
  id: string;
  problemId: string;
  language: string;
  code: string;
  reason: SnapshotReason;
  createdAt: number;
};

// Per problem, not global. A global cap would let one heavily-edited problem
// evict every other problem's history.
const MAX_SNAPSHOTS_PER_PROBLEM = 20;

const snapshotSchema = z.object({
  id: z.string().min(1),
  problemId: z.string().min(1),
  language: z.string().min(1),
  code: z.string(),
  reason: z.enum(SNAPSHOT_REASONS),
  createdAt: z.number(),
});

const persistedSchema = z.object({
  snapshots: z.array(snapshotSchema),
});

type CodeHistoryState = {
  snapshots: CodeSnapshot[];
  record: (input: Omit<CodeSnapshot, 'id' | 'createdAt'>) => void;
  forProblem: (problemId: string) => CodeSnapshot[];
  clearProblem: (problemId: string) => void;
};

function createId(): string {
  return `s${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

export const useCodeHistoryStore = create<CodeHistoryState>()(
  persist(
    (set, get) => ({
      snapshots: [],

      record: ({ problemId, language, code, reason }) => {
        if (!problemId || !code.trim()) return;

        const existing = get().snapshots;
        // Consecutive identical buffers are noise, not history -- pressing Run
        // three times should leave one entry, not three.
        const latestForProblem = existing.find((s) => s.problemId === problemId);
        if (latestForProblem?.code === code) {
          return;
        }

        const snapshot: CodeSnapshot = {
          id: createId(),
          problemId,
          language,
          code,
          reason,
          createdAt: Date.now(),
        };

        const forThisProblem = existing.filter((s) => s.problemId === problemId);
        const others = existing.filter((s) => s.problemId !== problemId);
        const kept = [snapshot, ...forThisProblem].slice(
          0,
          MAX_SNAPSHOTS_PER_PROBLEM
        );

        Logger.info('leetcode.history.record', {
          problemId,
          language,
          reason,
          length: code.length,
          keptForProblem: kept.length,
        });

        // Newest first, and this problem's entries ahead of the rest so
        // `forProblem`'s find() above stays an O(1)-ish hit on the common path.
        set({ snapshots: [...kept, ...others] });
      },

      forProblem: (problemId) =>
        get()
          .snapshots.filter((s) => s.problemId === problemId)
          .sort((a, b) => b.createdAt - a.createdAt),

      clearProblem: (problemId) => {
        Logger.info('leetcode.history.clear_problem', { problemId });
        set((state) => ({
          snapshots: state.snapshots.filter((s) => s.problemId !== problemId),
        }));
      },
    }),
    {
      name: 'leetcode-code-history',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({ snapshots: state.snapshots }),
      // Corrupt or out-of-date storage falls back to an empty history rather
      // than taking the editor screen down with it.
      merge: (persisted, current) => {
        const parsed = persistedSchema.safeParse(persisted);
        if (!parsed.success) {
          // Nothing stored (a fresh install) arrives as undefined: not a rejection.
          if (persisted != null) {
            Logger.warn('leetcode.history.rehydrate_rejected', {
              issues: parsed.error?.issues.length ?? 0,
            });
          }
          return { ...current, snapshots: [] };
        }
        return { ...current, snapshots: parsed.data.snapshots };
      },
      onRehydrateStorage: () => (_state, err) => {
        if (err)
          Logger.withError(err).error('leetcode.history.rehydrate_failed');
      },
    }
  )
);
