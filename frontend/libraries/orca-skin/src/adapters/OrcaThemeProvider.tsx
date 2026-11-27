import { SkinThemeProvider } from '@lib/skin';
import { useMemo, type ReactNode } from 'react';
import { useColorScheme } from 'react-native';
import { useOrcaScheme } from './appearance';
import { useChatPalette } from './chatPalette';
import { tabPalettes } from './tabPalette';

export function OrcaThemeProvider({ children }: { children: ReactNode }) {
  const scheme = useOrcaScheme();
  const colors = useChatPalette();
  const tabs = tabPalettes[useColorScheme() === 'dark' ? 'dark' : 'light'];
  const theme = useMemo(() => ({ scheme, colors, tabs }), [scheme, colors, tabs]);
  return <SkinThemeProvider value={theme}>{children}</SkinThemeProvider>;
}
