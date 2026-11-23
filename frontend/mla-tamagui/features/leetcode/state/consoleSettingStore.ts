import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

export const DEFAULT_CONSOLE = false;
const consoleSchema = z.boolean();

type ConsoleSettingState = {
  enabled: boolean;
  setEnabled: (enabled: boolean) => void;
};

/** Whether the problem screen offers the console (Testcase and Run Result tabs) in place of the Run Local card, kept across launches; a stored value that is not a boolean falls back to off. */
export const useConsoleSettingStore = create<ConsoleSettingState>()(
  persist(
    (set) => ({
      enabled: DEFAULT_CONSOLE,
      setEnabled: (enabled) => {
        Logger.info('leetcode.console.set', { enabled });
        set({ enabled });
      },
    }),
    {
      name: 'leetcode-console',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({ enabled: state.enabled }),
      merge: (persisted, current) => ({
        ...current,
        enabled: consoleSchema.safeParse((persisted as { enabled?: unknown } | undefined)?.enabled).data ?? DEFAULT_CONSOLE,
      }),
      onRehydrateStorage: () => (_state, err) => {
        if (err) Logger.withError(err).error('leetcode.console.rehydrate_failed');
      },
    }
  )
);
