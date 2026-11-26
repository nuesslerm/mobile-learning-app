// MUST be first, and must be a side-effect import. Nothing else imports the
// logger, so if this moves below something that logs during module init, every
// Logger.* call silently no-ops -- and that absence looks exactly like the app
// simply being quiet. App #1 carries the same comment for the same reason.
import '../lib/logger';
// Second, so a throw while the imports below evaluate is still logged as app.crash.
import '../app-shell/crashHandlerBoot';

import { appConfig } from '@lib/app-config';
import {
  resolveActiveThemeName,
  useHeaderHeightStore,
  usePresetThemeStore,
} from '@lib/tamagui-design-system';
import { tamaguiConfig } from '@lib/tamagui-design-system/theme';
import { Stack, ThemeProvider, useGlobalSearchParams, usePathname, useRootNavigationState, useRouter } from 'expo-router';
import { useEffect, useRef } from 'react';
import { Platform, useColorScheme } from 'react-native';
import { FadeIn, FadeOut } from 'react-native-reanimated';
import { SafeAreaProvider } from 'react-native-safe-area-context';
import { Toaster } from 'sonner-native';
import { FullWindowOverlay } from 'react-native-screens';
import { buoyEnabled } from '../app-shell/buoyEnabled';
import { TamaguiProvider, Theme } from '@lib/tamagui-design-system/primitives';

import { DeviceRunnerRoot, editorPageSource, PreparedEditorPages } from '@/features/leetcode';

import {
  NativeChromeBridge,
  useNavigationTheme,
} from '../app-shell/NativeChromeBridge';
import { NavigationTouchBoundary } from '../app-shell/NavigationTouchBoundary';
import { RouteLogger } from '../app-shell/RouteLogger';

// A require, not an import: production inlines the flag and Platform.OS, so a flagless or native build (the CI's EAS
// Update bakes the flag in for its seed) folds this branch away and the lab's web-only Alert never reaches the bundle.
if (process.env.EXPO_PUBLIC_DEVICE_LAB === '1' && Platform.OS === 'web') {
  // eslint-disable-next-line @typescript-eslint/no-require-imports -- see above
  require('@/lib/device-lab').installDeviceLab();
}

/**
 * The toast host, pushed clear of the screen header.
 *
 * sonner-native defaults to `offset: 0` (src/constants.ts), and its host mounts
 * above every screen -- so a toast landed ON the header. Measured: toast at
 * y16-90 over a header occupying y0-71, covering the problem title outright at
 * exactly the moment the toast is telling you what just happened to it.
 *
 * Computing the offset from the header's padding constants was tried first and
 * was wrong by 12px -- 59 against a real 71 -- because the header's content row
 * is whatever its title and subtitle need. So the header reports its measured
 * height and this reads it; nothing predicts anything.
 *
 * Falls back to a bare top inset while no header has laid out yet (a screen
 * with no ScreenHeader at all), which is the right answer there anyway.
 */
// Reanimated on web only runs predefined entering/exiting animations; sonner's custom worklets log a warning.
const WEB_TOAST_ANIMATION = { enter: FadeIn.duration(200), exit: FadeOut.duration(200) };
const PreviewTools = buoyEnabled
  ? (require('../app-shell/BuoyPreviewTools') as typeof import('../app-shell/BuoyPreviewTools')).BuoyPreviewTools
  : null;
const BuoyRouterStore = PreviewTools
  ? (require('../app-shell/buoyRouterStore') as typeof import('../app-shell/buoyRouterStore')).BuoyRouterStore
  : null;

function BuoyDevTools() {
  const { visualCapture } = useGlobalSearchParams<{ visualCapture?: string }>();
  if (!PreviewTools || visualCapture === '1') return null;
  const tools = <PreviewTools />;
  return Platform.OS === 'ios' ? <FullWindowOverlay>{tools}</FullWindowOverlay> : tools;
}

// On a bare Expo Go update link SDK 58 resolves an empty path and skips +native-intent.
function NativePreviewLaunch({ route }: { route: NonNullable<typeof appConfig.initialRoute> }) {
  const pathname = usePathname();
  const navigation = useRootNavigationState();
  const router = useRouter();
  const handled = useRef(false);
  useEffect(() => {
    if (!navigation?.key || handled.current) return;
    handled.current = true;
    if (pathname === '/') router.replace(route);
  }, [navigation?.key, pathname, route, router]);
  return null;
}

function HeaderClearToaster() {
  const headerHeight = useHeaderHeightStore((state) => state.height);
  return (
    <Toaster
      enableStacking
      offset={headerHeight + 8}
      animation={Platform.OS === 'web' ? WEB_TOAST_ANIMATION : undefined}
    />
  );
}

export default function RootLayout() {
  const navigationTheme = useNavigationTheme();
  const scheme = useColorScheme();
  const activeThemeId = usePresetThemeStore((s) => s.activeThemeId);
  const themeMode = usePresetThemeStore((s) => s.themeMode);
  const themeName = resolveActiveThemeName(
    activeThemeId,
    themeMode,
    scheme === 'dark' ? 'dark' : 'light'
  );

  return (
    <SafeAreaProvider>
      <NavigationTouchBoundary>
        {/* defaultTheme is required, not cosmetic: without it Tamagui throws
            "Can't find Tamagui configuration" when a theme resolves late. */}
        <TamaguiProvider config={tamaguiConfig} defaultTheme={themeName}>
          <Theme name={themeName}>
            {/* Headers are ON here. The leetcode routes set their own titles
                and header buttons via Stack.Screen -- the editor's Info button
                is the only way into the problem description. Only the drawer
                group hides its header, because its screens carry their own. */}
            <DeviceRunnerRoot>
            <PreparedEditorPages getSource={editorPageSource}>
            <ThemeProvider value={navigationTheme}>
            <Stack screenOptions={{ orientation: 'portrait' }}>
              {/* `title` even though the header is hidden: a child route's
                  back button labels itself from its PARENT's title, so without
                  it the button reads "(app)" -- the route-group directory name
                  leaking into the UI. */}
              <Stack.Screen
                name="(app)"
                options={{ headerShown: false, title: 'Blind 75' }}
              />
              <Stack.Screen
                name="settings"
                options={{ headerShown: false, presentation: 'modal' }}
              />
              {/* The leetcode group is a Stack of its own and its screens draw their
                  headers there; left on, this Stack paints a second bar captioned
                  with the group's name above them. */}
              <Stack.Screen name="leetcode" options={{ headerShown: false }} />
              {/* Full screens, so they keep a header -- but it has to be the
                  app's own, or the title falls back to the route pattern. */}
              <Stack.Screen
                name="coding-agent"
                options={{
                  presentation: 'transparentModal',
                  headerShown: false,
                  animation: 'none',
                }}
              />
            </Stack>
            </ThemeProvider>
            </PreparedEditorPages>
            </DeviceRunnerRoot>
            <NativeChromeBridge />
            <RouteLogger />
            {Platform.OS !== 'web' && appConfig.initialRoute && appConfig.initialRoute !== '/' &&
              <NativePreviewLaunch route={appConfig.initialRoute} />}
            {/* Defaults, deliberately -- including `fullWindowOverlay`, which
                sonner sets true and which renders the toasts into a separate
                iOS UIWindow above every presented view controller. That is the
                same escape hatch the overlay routes need, already correct out
                of the box, so a toast fired from inside a transparentModal
                route lands above it rather than underneath. */}
            {/* sonner-native ships `enableStacking: false`
                  (src/constants.ts), so its own default is a plain vertical
                  list -- not the stacked pile sonner is known for. Opt in. */}
            <HeaderClearToaster />
            {BuoyRouterStore && <BuoyRouterStore />}
            {PreviewTools && <BuoyDevTools />}
          </Theme>
        </TamaguiProvider>
      </NavigationTouchBoundary>
    </SafeAreaProvider>
  );
}
