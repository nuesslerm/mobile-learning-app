import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

import {
  moveGroup,
  resetGroups,
  setGroupChars,
  symbolGroupsSchema,
  type SymbolGroup,
  type SymbolGroupId,
} from '../../domain/editor-settings/symbolGroups';

export const RAIL_STYLES = ['classic', 'symbols'] as const;
export type RailStyle = (typeof RAIL_STYLES)[number];

export const DEFAULT_RAIL_STYLE: RailStyle = 'symbols';
export const DEFAULT_SYMBOL_BAR_SHOWN = true;

const shownSchema = z.boolean();
const railStyleSchema = z.enum(RAIL_STYLES);

type SymbolBarState = {
  shown: boolean;
  groups: readonly SymbolGroup[];
  railStyle: RailStyle;
  setShown: (shown: boolean) => void;
  setGroup: (id: SymbolGroupId, chars: string) => void;
  move: (from: number, to: number) => void;
  reset: () => void;
  setRailStyle: (railStyle: RailStyle) => void;
};

/** The symbol bar's switch, its eight groups and which rail shows them, kept across launches; a stored field that does not parse falls back alone. */
export const useSymbolBarStore = create<SymbolBarState>()(
  persist(
    (set, get) => ({
      shown: DEFAULT_SYMBOL_BAR_SHOWN,
      groups: resetGroups(),
      railStyle: DEFAULT_RAIL_STYLE,
      setShown: (shown) => {
        Logger.info('leetcode.symbol_bar.set_shown', { shown });
        set({ shown });
      },
      setGroup: (id, chars) => {
        const groups = setGroupChars(get().groups, id, chars);
        Logger.info('leetcode.symbol_bar.set_group', { id, length: Array.from(groups.find((group) => group.id === id)?.chars ?? '').length });
        set({ groups });
      },
      move: (from, to) => {
        const groups = moveGroup(get().groups, from, to);
        if (groups === get().groups) return;
        Logger.info('leetcode.symbol_bar.move', { from, to });
        set({ groups });
      },
      reset: () => {
        Logger.info('leetcode.symbol_bar.reset', {});
        set({ groups: resetGroups() });
      },
      setRailStyle: (railStyle) => {
        Logger.info('leetcode.symbol_bar.set_rail_style', { railStyle });
        set({ railStyle });
      },
    }),
    {
      name: 'leetcode-symbol-bar',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({ shown: state.shown, groups: state.groups, railStyle: state.railStyle }),
      merge: (persisted, current) => {
        const stored = (persisted ?? {}) as { shown?: unknown; groups?: unknown; railStyle?: unknown };
        return {
          ...current,
          shown: shownSchema.safeParse(stored.shown).data ?? DEFAULT_SYMBOL_BAR_SHOWN,
          groups: symbolGroupsSchema.safeParse(stored.groups).data ?? resetGroups(),
          railStyle: railStyleSchema.safeParse(stored.railStyle).data ?? DEFAULT_RAIL_STYLE,
        };
      },
      onRehydrateStorage: () => (_state, err) => {
        if (err) Logger.withError(err).error('leetcode.symbol_bar.rehydrate_failed');
      },
    }
  )
);
