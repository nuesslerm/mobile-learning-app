import { isLanguage, type CompletionConfig, type ConfigPatch, type EditorConfig } from '@lib/code-editor-api';

import type { EditorConfig as LegacyEditorConfig } from '../ports';

export type { LegacyEditorConfig };
export type LegacyConfigPatch = Partial<LegacyEditorConfig>;

const MAPPED = new Set(['language', 'wordWrap', 'showWhitespace', 'isDark', 'cursorColor', 'selectionColor', 'backgroundColor', 'gutterColor', 'borderColor', 'syntaxPalette', 'fontSize', 'lineHeight', 'tabSize']);

/** The typed-API form of an old patch, or null when it holds a key the typed API does not carry (`initialText`, a language it does not name, anything unknown). */
export function toConfigPatch(legacy: LegacyConfigPatch): ConfigPatch | null {
  if (Object.keys(legacy).some((key) => !MAPPED.has(key))) return null;
  if (legacy.language !== undefined && !isLanguage(legacy.language)) return null;
  const palette = {
    ...(legacy.isDark === undefined ? {} : { dark: legacy.isDark }),
    ...(legacy.cursorColor === undefined ? {} : { cursor: legacy.cursorColor }),
    ...(legacy.selectionColor === undefined ? {} : { selection: legacy.selectionColor }),
    ...(legacy.backgroundColor === undefined ? {} : { background: legacy.backgroundColor }),
    ...(legacy.gutterColor === undefined ? {} : { gutter: legacy.gutterColor }),
    ...(legacy.borderColor === undefined ? {} : { border: legacy.borderColor }),
    ...(legacy.syntaxPalette === undefined ? {} : { syntax: legacy.syntaxPalette }),
  };
  return {
    ...(legacy.language === undefined || !isLanguage(legacy.language) ? {} : { language: legacy.language }),
    ...(legacy.wordWrap === undefined ? {} : { wrap: legacy.wordWrap }),
    ...(legacy.showWhitespace === undefined ? {} : { whitespace: legacy.showWhitespace }),
    ...(legacy.tabSize === undefined ? {} : { tabSize: legacy.tabSize }),
    ...(legacy.fontSize === undefined ? {} : { fontSize: legacy.fontSize }),
    ...(legacy.lineHeight === undefined ? {} : { lineHeight: legacy.lineHeight }),
    ...(Object.keys(palette).length > 0 ? { palette } : {}),
  };
}

/** The old form of a typed-API patch. */
export function toLegacyPatch(patch: ConfigPatch): LegacyConfigPatch {
  const { palette } = patch;
  return {
    ...(patch.language === undefined ? {} : { language: patch.language }),
    ...(patch.wrap === undefined ? {} : { wordWrap: patch.wrap }),
    ...(patch.whitespace === undefined ? {} : { showWhitespace: patch.whitespace }),
    ...(patch.tabSize === undefined ? {} : { tabSize: patch.tabSize }),
    ...(patch.fontSize === undefined ? {} : { fontSize: patch.fontSize }),
    ...(patch.lineHeight === undefined ? {} : { lineHeight: patch.lineHeight }),
    ...(palette?.dark === undefined ? {} : { isDark: palette.dark }),
    ...(palette?.cursor === undefined ? {} : { cursorColor: palette.cursor }),
    ...(palette?.selection === undefined ? {} : { selectionColor: palette.selection }),
    ...(palette?.background === undefined ? {} : { backgroundColor: palette.background }),
    ...(palette?.gutter === undefined ? {} : { gutterColor: palette.gutter }),
    ...(palette?.border === undefined ? {} : { borderColor: palette.border }),
    ...(palette?.syntax === undefined ? {} : { syntaxPalette: palette.syntax }),
  };
}

export function toEditorConfig(legacy: LegacyEditorConfig, completion?: CompletionConfig): EditorConfig {
  return {
    language: isLanguage(legacy.language) ? legacy.language : 'python',
    wrap: legacy.wordWrap,
    whitespace: legacy.showWhitespace ?? false,
    tabSize: legacy.tabSize,
    fontSize: legacy.fontSize,
    lineHeight: legacy.lineHeight,
    palette: { dark: legacy.isDark, cursor: legacy.cursorColor, selection: legacy.selectionColor, background: legacy.backgroundColor, gutter: legacy.gutterColor, border: legacy.borderColor, ...(legacy.syntaxPalette === undefined ? {} : { syntax: legacy.syntaxPalette }) },
    ...(completion ? { completion } : {}),
  };
}
