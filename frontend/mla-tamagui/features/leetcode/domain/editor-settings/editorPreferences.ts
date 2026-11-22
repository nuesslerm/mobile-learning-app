export const FONT_SIZES = [12, 13, 14, 15, 16, 18, 20] as const;
export const TAB_SIZES = [2, 4, 8] as const;

export type FontSize = (typeof FONT_SIZES)[number];
export type TabSize = (typeof TAB_SIZES)[number];

export type EditorPreferences = {
  fontSize: FontSize;
  tabSize: TabSize;
  wrap: boolean;
  whitespace: boolean;
};

export const DEFAULT_FONT_SIZE: FontSize = 14;
export const DEFAULT_TAB_SIZE: TabSize = 4;
export const DEFAULT_WRAP = true;
export const DEFAULT_WHITESPACE = false;

export const DEFAULT_EDITOR_PREFERENCES: EditorPreferences = {
  fontSize: DEFAULT_FONT_SIZE,
  tabSize: DEFAULT_TAB_SIZE,
  wrap: DEFAULT_WRAP,
  whitespace: DEFAULT_WHITESPACE,
};

const LINE_HEIGHT_RATIO = 1.5;

export const isFontSize = (value: unknown): value is FontSize => FONT_SIZES.some((size) => size === value);
export const isTabSize = (value: unknown): value is TabSize => TAB_SIZES.some((size) => size === value);

export const lineHeightFor = (fontSize: number): number => Math.round(fontSize * LINE_HEIGHT_RATIO);
export const fontSizeLabel = (fontSize: number): string => `${fontSize}px`;

/** Each field is read on its own: one that is not a listed value (or a boolean) takes its default and the others survive. */
export function parseEditorPreferences(value: unknown): EditorPreferences {
  const stored = typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : {};
  return {
    fontSize: isFontSize(stored.fontSize) ? stored.fontSize : DEFAULT_FONT_SIZE,
    tabSize: isTabSize(stored.tabSize) ? stored.tabSize : DEFAULT_TAB_SIZE,
    wrap: typeof stored.wrap === 'boolean' ? stored.wrap : DEFAULT_WRAP,
    whitespace: typeof stored.whitespace === 'boolean' ? stored.whitespace : DEFAULT_WHITESPACE,
  };
}
