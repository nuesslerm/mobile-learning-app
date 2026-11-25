import type { ReactNode } from 'react';
import { View } from 'react-native';
import { homePalettes, useOrcaScheme } from './appearance';

export function HomeSurface({ children }: { children: ReactNode }) {
  const palette = homePalettes[useOrcaScheme()];
  return <View style={{ flex: 1, backgroundColor: palette.bgBase }}>{children}</View>;
}
