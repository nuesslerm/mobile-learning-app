import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

export const DEFAULT_CURSOR_HANDLE = false;
const cursorHandleSchema = z.boolean();

type CursorHandleSettingState = {
  cursorHandle: boolean;
  setCursorHandle: (cursorHandle: boolean) => void;
};

/** Whether the keyboard's action row carries the cursor pad, kept across launches; a stored value that is not a boolean falls back to the default. */
export const useCursorHandleSettingStore = create<CursorHandleSettingState>()(
  persist(
    (set) => ({
      cursorHandle: DEFAULT_CURSOR_HANDLE,
      setCursorHandle: (cursorHandle) => {
        Logger.info('leetcode.cursor_handle.set', { cursorHandle });
        set({ cursorHandle });
      },
    }),
    {
      name: 'leetcode-cursor-handle',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({ cursorHandle: state.cursorHandle }),
      merge: (persisted, current) => ({
        ...current,
        cursorHandle: cursorHandleSchema.safeParse((persisted as { cursorHandle?: unknown } | undefined)?.cursorHandle).data ?? DEFAULT_CURSOR_HANDLE,
      }),
      onRehydrateStorage: () => (_state, err) => {
        if (err) Logger.withError(err).error('leetcode.cursor_handle.rehydrate_failed');
      },
    }
  )
);
