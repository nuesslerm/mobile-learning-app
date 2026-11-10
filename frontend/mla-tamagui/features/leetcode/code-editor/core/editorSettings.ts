import { Logger } from '@lib/logging';
import { create } from 'zustand';

import {
  CODE_PALETTES,
  DEFAULT_PALETTE_ID,
  type CodePalette,
  type CodePaletteId,
} from '../highlighting/palettes';
/**
 * Self-contained editor settings — palette, font size, and word wrap.
 *
 * Mirrors the reference app's `CodeEditorSettingsStore` API (the subset the
 * WebView editor consumes) without the design-system / DI dependencies.
 */

const LINE_HEIGHT_RATIO = 1.5;
const DEFAULT_FONT_SIZE = 14;
const MIN_FONT_SIZE = 10;
const MAX_FONT_SIZE = 28;
type CodeEditorSettingsState = {
  paletteId: CodePaletteId;
  fontSize: number;
  wordWrap: boolean;
  setPalette: (id: CodePaletteId) => void;
  setFontSize: (size: number) => void;
  setWordWrap: (wrap: boolean) => void;
};
export const useCodeEditorSettingsStore = create<CodeEditorSettingsState>(
  (set) => ({
    paletteId: DEFAULT_PALETTE_ID,
    fontSize: DEFAULT_FONT_SIZE,
    wordWrap: true,
    setPalette: (id) => {
      Logger.info('editor.palette.set', {
        paletteId: id,
      });
      set({
        paletteId: id,
      });
    },
    setFontSize: (size) => {
      const fontSize = Math.max(MIN_FONT_SIZE, Math.min(MAX_FONT_SIZE, size));
      Logger.info('editor.font_size.set', {
        requested: size,
        fontSize,
      });
      set({
        fontSize,
      });
    },
    setWordWrap: (wrap) => {
      Logger.info('editor.word_wrap.set', {
        wordWrap: wrap,
      });
      set({
        wordWrap: wrap,
      });
    },
  })
);
export function getCodeFont(fontSize: number) {
  return {
    fontSize,
    lineHeight: Math.round(fontSize * LINE_HEIGHT_RATIO),
  };
}
export function getPalette(paletteId: CodePaletteId): CodePalette {
  return CODE_PALETTES[paletteId];
}
