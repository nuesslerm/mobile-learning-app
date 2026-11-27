import { OrcaThemeProvider, orcaSkin } from '@lib/orca-skin';
import { ActiveSkinThemeProvider, SkinPreferenceProvider, SkinProvider } from '@lib/skin';
import { TamaguiThemeProvider, tamaguiSkin } from '@lib/tamagui-skin';
import { useMemo, useSyncExternalStore, type ReactNode } from 'react';

import { useSkinStore } from './skinStore';

const SKINS = {
  tamagui: { skin: tamaguiSkin, Theme: TamaguiThemeProvider },
  orca: { skin: orcaSkin, Theme: OrcaThemeProvider },
};

/** The one place the app names a skin: it reads the stored choice and provides that skin to every screen. */
export function SkinRoot({ children }: { children: ReactNode }) {
  const preference = useSkinStore((state) => state.skin);
  const select = useSkinStore((state) => state.setSkin);
  const choice = useMemo(() => ({ preference, select }), [preference, select]);
  const hydrated = useSyncExternalStore(useSkinStore.persist.onFinishHydration, useSkinStore.persist.hasHydrated, () => false);
  const { skin, Theme } = SKINS[preference];
  return (
    <SkinPreferenceProvider value={choice}>
      <ActiveSkinThemeProvider value={hydrated ? Theme : null}>
        <SkinProvider value={skin}>{children}</SkinProvider>
      </ActiveSkinThemeProvider>
    </SkinPreferenceProvider>
  );
}
