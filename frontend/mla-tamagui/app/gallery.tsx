import { AppearancePreferenceContextProvider, SkinGallery, SkinThemeBoundary, type AppearancePreferenceValue } from '@lib/skin';
import { Stack, router, useLocalSearchParams } from 'expo-router';
import { useMemo } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

/**
 * Every entry of the skin contract under the selected skin: `/gallery` lists the sections, `/gallery?section=<name>` shows one.
 * `scheme=light|dark` pins the appearance, so one device can show both.
 */
export default function GalleryRoute() {
  const { section, scheme } = useLocalSearchParams<{ section?: string; scheme?: string }>();
  const { top, bottom } = useSafeAreaInsets();
  const appearance = useMemo<AppearancePreferenceValue>(
    () => ({ preference: scheme === 'light' || scheme === 'dark' ? scheme : 'system', select: () => undefined }),
    [scheme]
  );
  return (
    <AppearancePreferenceContextProvider value={appearance}>
      <SkinThemeBoundary>
        <Stack.Screen options={{ headerShown: false, title: 'Skin gallery' }} />
        <SkinGallery section={section} topInset={top} bottomInset={bottom} onOpen={(next) => router.setParams({ section: next })} />
      </SkinThemeBoundary>
    </AppearancePreferenceContextProvider>
  );
}
