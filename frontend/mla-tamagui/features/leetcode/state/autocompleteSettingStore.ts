import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

export const DEFAULT_AUTOCOMPLETE = true;
const autocompleteSchema = z.boolean();

type AutocompleteSettingState = {
  autocomplete: boolean;
  setAutocomplete: (autocomplete: boolean) => void;
};

/** Whether the code editor offers names as a person types, kept across launches; a stored value that is not a boolean falls back to the default. */
export const useAutocompleteSettingStore = create<AutocompleteSettingState>()(
  persist(
    (set) => ({
      autocomplete: DEFAULT_AUTOCOMPLETE,
      setAutocomplete: (autocomplete) => {
        Logger.info('leetcode.autocomplete.set', { autocomplete });
        set({ autocomplete });
      },
    }),
    {
      name: 'leetcode-autocomplete',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({ autocomplete: state.autocomplete }),
      merge: (persisted, current) => ({
        ...current,
        autocomplete: autocompleteSchema.safeParse((persisted as { autocomplete?: unknown } | undefined)?.autocomplete).data ?? DEFAULT_AUTOCOMPLETE,
      }),
      onRehydrateStorage: () => (_state, err) => {
        if (err) Logger.withError(err).error('leetcode.autocomplete.rehydrate_failed');
      },
    }
  )
);
