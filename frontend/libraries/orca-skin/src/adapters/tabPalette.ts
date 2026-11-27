import type { SkinTabColors } from '@lib/skin';

const shared = {
  focus: '#3875ff',
  danger: '#c42d36',
  recording: '#d00',
  scrim: 'rgba(0,0,0,0.74)',
  terminal: { background: '#191b26', foreground: '#e1e4ed', cursor: '#c3cbdf' },
};

// The tabs follow the device's scheme, not the app's appearance choice.
export const tabPalettes: Record<'light' | 'dark', SkinTabColors> = {
  dark: {
    bg: '#18181b',
    surface: '#252528',
    ink: '#fafafa',
    muted: '#aaa',
    border: '#47474a',
    headerInk: '#fafafa',
    ...shared,
  },
  light: {
    bg: '#fff',
    surface: '#f5f5f5',
    ink: '#171717',
    muted: '#606066',
    border: '#d4d4d4',
    headerInk: '#0a0a0a',
    ...shared,
  },
};
