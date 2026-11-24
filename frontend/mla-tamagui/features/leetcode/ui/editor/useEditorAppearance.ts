import type { EditorConfig } from '../../domain/ports';
import { getPalette, useCodeEditorSettingsStore } from '../../state/editor/editorSettings';
import { DEFAULT_PALETTE_ID } from '../../state/editor/palettes';

export function useEditorAppearance(canvas: boolean) {
  const paletteId = useCodeEditorSettingsStore((state) => state.paletteId);
  const palette = getPalette(paletteId);
  const contrast = canvas && paletteId === DEFAULT_PALETTE_ID;

  const syntaxPalette: EditorConfig['syntaxPalette'] = canvas ? (contrast ? 'contrast' : 'default') : undefined;

  return {
    palette,
    paletteId,
    backgroundColor: contrast ? '#000000' : palette.background,
    syntaxPalette,
    isDark: palette.appearance === 'dark',
  };
}
