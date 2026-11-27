export interface SkinSyntaxColors {
  comment: string;
  keyword: string;
  string: string;
  number: string;
  type: string;
  function: string;
  variable: string;
  meta: string;
  tag: string;
}

export interface SkinColors {
  bgBase: string;
  bgPanel: string;
  bgGroup: string;
  bgRaised: string;
  borderSubtle: string;
  textPrimary: string;
  textSecondary: string;
  textMuted: string;
  surfaceBright: string;
  accentBlue: string;
  onAccent: string;
  statusGreen: string;
  statusAmber: string;
  statusRed: string;
  translucentFill: string;
  chatForegroundStrong: string;
  sheetSurface: string;
  userBorder: string;
  composerSurface: string;
  composerBorder: string;
  syntax: SkinSyntaxColors;
}

/** What the terminal, browser and note tabs paint with. */
export interface SkinTabColors {
  bg: string;
  surface: string;
  ink: string;
  muted: string;
  border: string;
  headerInk: string;
  focus: string;
  danger: string;
  recording: string;
  scrim: string;
  terminal: { background: string; foreground: string; cursor: string };
}

/** The colors a skin paints with; screens read them instead of a skin's own palette. */
export interface SkinTheme {
  scheme: 'light' | 'dark';
  colors: SkinColors;
  tabs: SkinTabColors;
}
