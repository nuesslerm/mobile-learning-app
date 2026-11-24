import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

import {
  DEFAULT_EDITOR_PREFERENCES,
  parseEditorPreferences,
  type EditorPreferences,
  type FontSize,
  type TabSize,
} from '../../domain/editor-settings/editorPreferences';

type EditorPreferencesState = EditorPreferences & {
  setFontSize: (fontSize: FontSize) => void;
  setTabSize: (tabSize: TabSize) => void;
  setWrap: (wrap: boolean) => void;
  setWhitespace: (whitespace: boolean) => void;
};

/** The editor's font size, tab size, wrap and whitespace, per device and never per problem, kept across launches; a stored field that is not a listed value falls back to its default alone. */
export const useEditorPreferencesStore = create<EditorPreferencesState>()(
  persist(
    (set) => ({
      ...DEFAULT_EDITOR_PREFERENCES,
      setFontSize: (fontSize) => {
        Logger.info('leetcode.editor_prefs.set', { field: 'fontSize', value: fontSize });
        set({ fontSize });
      },
      setTabSize: (tabSize) => {
        Logger.info('leetcode.editor_prefs.set', { field: 'tabSize', value: tabSize });
        set({ tabSize });
      },
      setWrap: (wrap) => {
        Logger.info('leetcode.editor_prefs.set', { field: 'wrap', value: wrap });
        set({ wrap });
      },
      setWhitespace: (whitespace) => {
        Logger.info('leetcode.editor_prefs.set', { field: 'whitespace', value: whitespace });
        set({ whitespace });
      },
    }),
    {
      name: 'leetcode-editor-prefs',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({ fontSize: state.fontSize, tabSize: state.tabSize, wrap: state.wrap, whitespace: state.whitespace }),
      // Nothing stored keeps what a person already changed while the read was in flight.
      merge: (persisted, current) => (persisted === undefined ? current : { ...current, ...parseEditorPreferences(persisted) }),
      onRehydrateStorage: () => (_state, err) => {
        if (err) Logger.withError(err).error('leetcode.editor_prefs.rehydrate_failed');
      },
    }
  )
);
