import { Drawer } from 'expo-router/drawer';
import type { ViewStyle } from 'react-native';
import { Platform, useWindowDimensions } from 'react-native';

import { AppDrawerContent } from '@/app-shell/AppDrawerContent';

/**
 * The app's only navigation shell.
 *
 * Web gets a plain CSS string rather than a computed number, and that is not
 * stylistic: `react-native-drawer-layout`'s `getDrawerWidthWeb` passes a
 * string straight through as real CSS, while a `useWindowDimensions()` number
 * renders `width: 0px` on a static web export -- the pre-render has no real
 * window and the 0 is never recomputed after hydration. App #1 hit this and
 * documented it; the same navigator is in use here.
 */
export default function AppLayout() {
  const { width } = useWindowDimensions();
  const drawerWidth: ViewStyle['width'] =
    Platform.OS === 'web'
      ? ('min(78%, 320px)' as ViewStyle['width'])
      : Math.min(width * 0.78, 320);

  return (
    <Drawer
      drawerContent={(props) => <AppDrawerContent {...props} />}
      screenOptions={{
        headerShown: false,
        drawerType: 'slide',
        drawerStyle: { width: drawerWidth },
        overlayColor: 'transparent',
        swipeEdgeWidth: 40,
      }}
    >
      <Drawer.Screen name="(content)" />
    </Drawer>
  );
}
