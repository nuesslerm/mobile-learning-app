import { Logger } from '@lib/logging';
import { create } from 'zustand';

import {
  CODE_PALETTES,
  DEFAULT_PALETTE_ID,
  type CodePalette,
  type CodePaletteId,
} from './palettes';

type CodeEditorSettingsState = {
  paletteId: CodePaletteId;
  setPalette: (id: CodePaletteId) => void;
};
export const useCodeEditorSettingsStore = create<CodeEditorSettingsState>(
  (set) => ({
    paletteId: DEFAULT_PALETTE_ID,
    setPalette: (id) => {
      Logger.info('editor.palette.set', {
        paletteId: id,
      });
      set({
        paletteId: id,
      });
    },
  })
);
export function getPalette(paletteId: CodePaletteId): CodePalette {
  return CODE_PALETTES[paletteId];
}
