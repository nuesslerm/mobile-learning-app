import { View } from '@lib/tamagui-design-system';
import { useGlobalSearchParams, usePathname } from 'expo-router';
import { Drawer } from 'expo-router/drawer';
import type { ViewStyle } from 'react-native';
import { Platform, useWindowDimensions } from 'react-native';

import { AppDrawerContent } from '@/app-shell/AppDrawerContent';
import { useResolvedScheme } from '@/app-shell/NativeChromeBridge';

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
  const path = usePathname();
  const { workspace } = useGlobalSearchParams<{ workspace?: string }>();
  const workspaceTabsOpen = path === '/coding-agent' && workspace === '1';
  const { width } = useWindowDimensions();
  const scheme = useResolvedScheme();
  const drawerWidth: ViewStyle['width'] =
    Platform.OS === 'web'
      ? ('min(75%, 360px)' as ViewStyle['width'])
      : Math.min(width * 0.75, 360);

  return (
    <View flex={1} bg={Platform.OS === 'ios' && scheme === 'dark' ? '#121212' : '$background'} style={{ zIndex: 0 }}>
      <Drawer
        drawerContent={(props) => <AppDrawerContent {...props} />}
        screenOptions={{
          headerShown: false,
          drawerType: 'back',
          drawerStyle: { width: drawerWidth },
          sceneStyle: { backgroundColor: 'transparent' },
          overlayColor: 'transparent',
          keyboardDismissMode: 'on-drag',
          swipeEnabled: !workspaceTabsOpen,
          swipeEdgeWidth: 40,
        }}
      >
        <Drawer.Screen name="(content)" />
      </Drawer>
    </View>
  );
}
