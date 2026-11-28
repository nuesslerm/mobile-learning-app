import { OrcaThemeProvider, orcaSkin } from '@lib/orca-skin';
import { ActiveSkinThemeProvider, SkinProvider } from '@lib/skin';
import { TamaguiThemeProvider, tamaguiSkin } from '@lib/tamagui-skin';
import type { ReactNode } from 'react';

const SKINS = {
  tamagui: { skin: tamaguiSkin, Theme: TamaguiThemeProvider },
  orca: { skin: orcaSkin, Theme: OrcaThemeProvider },
};

/** Stands in for the app root in this feature's tests: the skin `FLOW_SKIN` names, the Tamagui one by default. */
export function SkinHarness({ children }: { children: ReactNode }) {
  const { skin, Theme } = SKINS[process.env.FLOW_SKIN === 'orca' ? 'orca' : 'tamagui'];
  return (
    <ActiveSkinThemeProvider value={Theme}>
      <SkinProvider value={skin}>{children}</SkinProvider>
    </ActiveSkinThemeProvider>
  );
}
