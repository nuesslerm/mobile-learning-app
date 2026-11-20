import type { ConfigPatch, EditorConfig, Language } from '@lib/code-editor-api';

/** The config the page and the old `init` / `updateConfig` messages speak. */
export type LegacyEditorConfig = {
  initialText: string;
  isDark: boolean;
  fontSize: number;
  lineHeight: number;
  wordWrap: boolean;
  language: string;
  cursorColor: string;
  selectionColor: string;
  backgroundColor: string;
  gutterColor: string;
  borderColor: string;
  tabSize: number;
};
export type LegacyConfigPatch = Partial<LegacyEditorConfig>;

const MAPPED = new Set(['wordWrap', 'isDark', 'cursorColor', 'selectionColor', 'backgroundColor', 'gutterColor', 'borderColor', 'fontSize', 'lineHeight', 'tabSize']);

/** The typed-API form of an old patch, or null when it holds a key the typed API does not carry (`initialText`, `language`, anything unknown). */
export function toConfigPatch(legacy: LegacyConfigPatch): ConfigPatch | null {
  if (Object.keys(legacy).some((key) => !MAPPED.has(key))) return null;
  const palette = {
    ...(legacy.isDark === undefined ? {} : { dark: legacy.isDark }),
    ...(legacy.cursorColor === undefined ? {} : { cursor: legacy.cursorColor }),
    ...(legacy.selectionColor === undefined ? {} : { selection: legacy.selectionColor }),
    ...(legacy.backgroundColor === undefined ? {} : { background: legacy.backgroundColor }),
    ...(legacy.gutterColor === undefined ? {} : { gutter: legacy.gutterColor }),
    ...(legacy.borderColor === undefined ? {} : { border: legacy.borderColor }),
  };
  return {
    ...(legacy.wordWrap === undefined ? {} : { wrap: legacy.wordWrap }),
    ...(legacy.tabSize === undefined ? {} : { tabSize: legacy.tabSize }),
    ...(legacy.fontSize === undefined ? {} : { fontSize: legacy.fontSize }),
    ...(legacy.lineHeight === undefined ? {} : { lineHeight: legacy.lineHeight }),
    ...(Object.keys(palette).length > 0 ? { palette } : {}),
  };
}

/** The old form of a typed-API patch. The language is never part of it: this editor cannot switch language after `init`. */
export function toLegacyPatch(patch: ConfigPatch): LegacyConfigPatch {
  const { palette } = patch;
  return {
    ...(patch.wrap === undefined ? {} : { wordWrap: patch.wrap }),
    ...(patch.tabSize === undefined ? {} : { tabSize: patch.tabSize }),
    ...(patch.fontSize === undefined ? {} : { fontSize: patch.fontSize }),
    ...(patch.lineHeight === undefined ? {} : { lineHeight: patch.lineHeight }),
    ...(palette?.dark === undefined ? {} : { isDark: palette.dark }),
    ...(palette?.cursor === undefined ? {} : { cursorColor: palette.cursor }),
    ...(palette?.selection === undefined ? {} : { selectionColor: palette.selection }),
    ...(palette?.background === undefined ? {} : { backgroundColor: palette.background }),
    ...(palette?.gutter === undefined ? {} : { gutterColor: palette.gutter }),
    ...(palette?.border === undefined ? {} : { borderColor: palette.border }),
  };
}

export function toEditorConfig(legacy: LegacyEditorConfig): EditorConfig {
  return {
    language: (legacy.language === 'python' ? 'python' : 'cpp') satisfies Language,
    wrap: legacy.wordWrap,
    tabSize: legacy.tabSize,
    fontSize: legacy.fontSize,
    lineHeight: legacy.lineHeight,
    palette: { dark: legacy.isDark, cursor: legacy.cursorColor, selection: legacy.selectionColor, background: legacy.backgroundColor, gutter: legacy.gutterColor, border: legacy.borderColor },
  };
}
