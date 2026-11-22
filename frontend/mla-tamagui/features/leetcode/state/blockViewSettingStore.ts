import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

export const DEFAULT_BLOCK_VIEW = false;
const blockViewSchema = z.boolean();

type BlockViewSettingState = {
  blockView: boolean;
  setBlockView: (blockView: boolean) => void;
};

/** Whether the editor screen shows the block view of the code, kept across launches; a stored value that is not a boolean falls back to the default. */
export const useBlockViewSettingStore = create<BlockViewSettingState>()(
  persist(
    (set) => ({
      blockView: DEFAULT_BLOCK_VIEW,
      setBlockView: (blockView) => {
        Logger.info('leetcode.block_view.set', { blockView });
        set({ blockView });
      },
    }),
    {
      name: 'leetcode-block-view',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({ blockView: state.blockView }),
      merge: (persisted, current) => ({
        ...current,
        blockView: blockViewSchema.safeParse((persisted as { blockView?: unknown } | undefined)?.blockView).data ?? DEFAULT_BLOCK_VIEW,
      }),
      onRehydrateStorage: () => (_state, err) => {
        if (err) Logger.withError(err).error('leetcode.block_view.rehydrate_failed');
      },
    }
  )
);
