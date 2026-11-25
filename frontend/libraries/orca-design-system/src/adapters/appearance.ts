import { useColorScheme } from 'react-native';

export const homePalettes = {
  dark: {
    bgBase: '#0a0a0a',
    bgPanel: '#171717',
    bgGroup: '#262626',
    bgRaised: '#262626',
    borderSubtle: 'rgba(255, 255, 255, 0.07)',
    textPrimary: '#fafafa',
    textSecondary: '#acacac',
    textMuted: '#898989',
    surfaceBright: '#e5e5e5',
    accentBlue: '#3276f2',
    onAccent: '#ffffff',
    statusAmber: '#eab308',
  },
  light: {
    bgBase: '#ffffff',
    bgPanel: '#fafafa',
    bgGroup: '#ffffff',
    bgRaised: '#f5f5f5',
    borderSubtle: '#e5e5e5',
    textPrimary: '#0a0a0a',
    textSecondary: '#696969',
    textMuted: '#696969',
    surfaceBright: '#171717',
    accentBlue: '#225be3',
    onAccent: '#ffffff',
    statusAmber: '#ca8a04',
  },
} as const;

/** The Orca home's spacing scale. */
export const homeSpacing = {
  xs: 4,
  sm: 8,
  md: 12,
  lg: 16,
  xl: 24,
} as const;

/** The Orca home's shapes: a pill for labels and round buttons, a surface for what holds content. */
export const homeRadii = {
  control: 12,
  pill: 999,
  surface: 16,
  floating: 24,
  tile: 10,
  mark: 4,
} as const;

export function useOrcaScheme() {
  return useColorScheme() === 'light' ? 'light' : 'dark';
}
