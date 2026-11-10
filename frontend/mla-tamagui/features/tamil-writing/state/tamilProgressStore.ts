import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

import {
  MASTERY_MIN,
  clampMastery,
  nextMastery,
  type PracticeOutcome,
} from '../domain/mastery';

/**
 * What the learner has written, and how it went.
 *
 * Progress is the whole point of a course, so it outlives the process: a
 * letter's rung and the attempt counts persist to AsyncStorage, keyed by the
 * character ids in `domain/characters.ts` (which is why those ids are stable).
 */

const characterProgressSchema = z.object({
  mastery: z.number(),
  attempts: z.number(),
  lastPracticedAt: z.number(),
});

const persistedSchema = z.object({
  characters: z.record(z.string(), characterProgressSchema),
  totalAttempts: z.number(),
});

type CharacterProgress = z.infer<typeof characterProgressSchema>;

type TamilProgressState = {
  characters: Record<string, CharacterProgress>;
  totalAttempts: number;
  recordAttempt: (characterId: string, outcome: PracticeOutcome) => void;
  masteryOf: (characterId: string) => number;
  masteryMap: () => Record<string, number>;
  resetLesson: (characterIds: readonly string[]) => void;
  resetAll: () => void;
};

const EMPTY_PROGRESS: CharacterProgress = {
  mastery: MASTERY_MIN,
  attempts: 0,
  lastPracticedAt: 0,
};

export const useTamilProgressStore = create<TamilProgressState>()(
  persist(
    (set, get) => ({
      characters: {},
      totalAttempts: 0,

      recordAttempt: (characterId, outcome) => {
        if (!characterId) return;

        const current = get().characters[characterId] ?? EMPTY_PROGRESS;
        const mastery = nextMastery(current.mastery, outcome);

        Logger.info('tamil.practice.attempt', {
          characterId,
          outcome,
          from: current.mastery,
          to: mastery,
        });

        set((state) => ({
          totalAttempts: state.totalAttempts + 1,
          characters: {
            ...state.characters,
            [characterId]: {
              mastery,
              attempts: current.attempts + 1,
              lastPracticedAt: Date.now(),
            },
          },
        }));
      },

      masteryOf: (characterId) =>
        clampMastery(get().characters[characterId]?.mastery ?? MASTERY_MIN),

      masteryMap: () => {
        const entries = Object.entries(get().characters).map(
          ([id, progress]) => [id, clampMastery(progress.mastery)] as const
        );
        return Object.fromEntries(entries);
      },

      resetLesson: (characterIds) => {
        Logger.info('tamil.progress.reset_lesson', {
          characters: characterIds.length,
        });
        set((state) => {
          const characters = { ...state.characters };
          for (const id of characterIds) delete characters[id];
          return { characters };
        });
      },

      resetAll: () => {
        Logger.info('tamil.progress.reset_all', {
          characters: Object.keys(get().characters).length,
        });
        set({ characters: {}, totalAttempts: 0 });
      },
    }),
    {
      name: 'tamil-writing-progress',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({
        characters: state.characters,
        totalAttempts: state.totalAttempts,
      }),
      // Corrupt storage costs the learner their streak, not the screen.
      merge: (persisted, current) => {
        // Nothing stored yet (a fresh install): zustand still calls merge, with undefined.
        if (persisted === undefined) return current;
        const parsed = persistedSchema.safeParse(persisted);
        if (!parsed.success) {
          Logger.warn('tamil.progress.rehydrate_rejected', {
            issues: parsed.error?.issues.length ?? 0,
          });
          return { ...current, characters: {}, totalAttempts: 0 };
        }
        return {
          ...current,
          characters: parsed.data.characters,
          totalAttempts: parsed.data.totalAttempts,
        };
      },
      onRehydrateStorage: () => (state, err) => {
        if (err) Logger.withError(err).error('tamil.progress.rehydrate_failed');
        else if (state)
          Logger.info('tamil.progress.rehydrated', {
            characters: Object.keys(state.characters).length,
            totalAttempts: state.totalAttempts,
          });
      },
    }
  )
);
