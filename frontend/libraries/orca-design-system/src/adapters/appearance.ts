import { useColorScheme } from 'react-native';

export const homePalettes = {
  dark: {
    bgBase: '#0a0a0a',
    bgPanel: '#171717',
    bgRaised: '#262626',
    borderSubtle: 'rgba(255, 255, 255, 0.07)',
    textPrimary: '#fafafa',
    textSecondary: '#acacac',
    surfaceBright: '#e5e5e5',
    accentBlue: '#3276f2',
    onAccent: '#ffffff',
  },
  light: {
    bgBase: '#ffffff',
    bgPanel: '#fafafa',
    bgRaised: '#f5f5f5',
    borderSubtle: '#e5e5e5',
    textPrimary: '#0a0a0a',
    textSecondary: '#696969',
    surfaceBright: '#171717',
    accentBlue: '#225be3',
    onAccent: '#ffffff',
  },
} as const;

export function useOrcaScheme() {
  return useColorScheme() === 'light' ? 'light' : 'dark';
}
