import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

import type { RunnerChoice } from '../domain/ports';

export const DEFAULT_RUNNER: RunnerChoice = 'device';
const runnerChoiceSchema = z.enum(['backend', 'device']);

type RunnerSettingState = {
  runner: RunnerChoice;
  setRunner: (runner: RunnerChoice) => void;
};

/** Where Run Local executes, kept across launches; a stored value that is not one of the two falls back to the default. */
export const useRunnerSettingStore = create<RunnerSettingState>()(
  persist(
    (set) => ({
      runner: DEFAULT_RUNNER,
      setRunner: (runner) => {
        Logger.info('leetcode.runner.set', { runner });
        set({ runner });
      },
    }),
    {
      name: 'leetcode-runner',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({ runner: state.runner }),
      merge: (persisted, current) => ({
        ...current,
        runner: runnerChoiceSchema.safeParse((persisted as { runner?: unknown } | undefined)?.runner).data ?? DEFAULT_RUNNER,
      }),
      onRehydrateStorage: () => (_state, err) => {
        if (err) Logger.withError(err).error('leetcode.runner.rehydrate_failed');
      },
    }
  )
);

/** Resolves once the persisted setting has been read, so the first run after a cold start cannot see the default. */
export function whenRunnerSettingHydrated(): Promise<void> {
  const { persist: api } = useRunnerSettingStore;
  if (api.hasHydrated()) return Promise.resolve();
  return new Promise((resolve) => {
    const stop = api.onFinishHydration(() => {
      stop();
      resolve();
    });
  });
}
