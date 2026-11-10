// MUST be first, and must be a side-effect import. Nothing else imports the
// logger, so if this moves below something that logs during module init, every
// Logger.* call silently no-ops -- and that absence looks exactly like the app
// simply being quiet. App #1 carries the same comment for the same reason.
import '../lib/logger';
// Second, so a throw while the imports below evaluate is still logged as app.crash.
import '../app-shell/crashHandlerBoot';

import { AgentationProvider } from '@lib/rn-agentation';
import {
  resolveActiveThemeName,
  useHeaderHeightStore,
  usePresetThemeStore,
} from '@lib/tamagui-design-system';
import { tamaguiConfig } from '@lib/tamagui-design-system/theme';
import { Stack } from 'expo-router';
import { Platform, useColorScheme } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';
import { FadeIn, FadeOut } from 'react-native-reanimated';
import { SafeAreaProvider } from 'react-native-safe-area-context';
import { Toaster } from 'sonner-native';
import { TamaguiProvider, Theme } from '@lib/tamagui-design-system/primitives';

import { AgentationMarkerLog } from '../app-shell/AgentationMarkerLog';
import { DebugFab } from '../app-shell/DebugFab';
import { RouteLogger } from '../app-shell/RouteLogger';

// Module scope keeps the options object identity stable across renders. The
// web driver resolves nodes from the document, so no inspectedRef is needed
// here (the old app passed one because it drove native node handles).
const AGENTATION_OPTIONS = {
  documentTreeDriverOptions: {
    // The debug sheet sits above app content in the DOM, so excluding it keeps
    // findDeepestAtPoint resolving to the element underneath.
    ignoredSelectors: ['#rn-agentation-debug-sheet'],
  },
};

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
      <GestureHandlerRootView style={{ flex: 1 }}>
        {/* defaultTheme is required, not cosmetic: without it Tamagui throws
            "Can't find Tamagui configuration" when a theme resolves late. */}
        <TamaguiProvider config={tamaguiConfig} defaultTheme={themeName}>
          <Theme name={themeName}>
            {/* Headers are ON here. The leetcode routes set their own titles
                and header buttons via Stack.Screen -- the editor's Info button
                is the only way into the problem description. Only the drawer
                group hides its header, because its screens carry their own. */}
            <Stack>
              {/* `title` even though the header is hidden: a child route's
                  back button labels itself from its PARENT's title, so without
                  it the button reads "(app)" -- the route-group directory name
                  leaking into the UI. */}
              <Stack.Screen
                name="(app)"
                options={{ headerShown: false, title: 'Blind 75' }}
              />
              {/* The leetcode group is a Stack of its own and its screens draw their
                  headers there; left on, this Stack paints a second bar captioned
                  with the group's name above them. */}
              <Stack.Screen name="leetcode" options={{ headerShown: false }} />
              {/* Full screens, so they keep a header -- but it has to be the
                  app's own, or the title falls back to the route pattern. */}
              <Stack.Screen
                name="coding-agent/[threadId]/settings"
                options={{
                  presentation: 'transparentModal',
                  headerShown: false,
                  animation: 'none',
                }}
              />
            </Stack>
            <RouteLogger />
            <DebugFab />
            {/* AgentationMarkerLog calls useAgentation, which throws
                  outright when no provider is above it -- and a throw here
                  takes the whole tree down, so every route renders blank
                  rather than just losing the marker log. */}
            <AgentationProvider options={AGENTATION_OPTIONS}>
              <AgentationMarkerLog />
            </AgentationProvider>
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
          </Theme>
        </TamaguiProvider>
      </GestureHandlerRootView>
    </SafeAreaProvider>
  );
}
