import { LoadSkiaWeb, WithSkiaWeb } from '@/lib/skiaWeb';
import { useSyncExternalStore } from 'react';
import { Platform } from 'react-native';
import { Text, View } from '@lib/tamagui-design-system/primitives';

/**
 * Throwaway probe for the agent-dashboards plan unit c1: prove victory-native
 * (Skia) renders on BOTH Expo Go and the web export before any dashboard is
 * built on it.
 *
 * The web path is a CanvasKit WASM bundle loaded by LoadSkiaWeb. On the
 * static-render (SSR) pass the route is evaluated in Node, where CanvasKit
 * cannot load (no canvaskit.wasm next to the server bundle) -- so the server
 * renders the same fallback the client's first paint shows, and WithSkiaWeb
 * mounts only after hydration, letting CanvasKit load in the browser where the
 * WASM is real. A `mounted` flag keeps the server HTML and the client's first
 * render byte-identical (no hydration mismatch, React error #418), swapping in
 * WithSkiaWeb only inside an effect. The component itself lives outside app/,
 * which Expo Router's dev mode requires for deferred loading.
 *
 * LoadSkiaWeb starts as early as possible on the client (module scope, not
 * behind React) so the ~8MB WASM fetch/init overlaps hydration instead of
 * serialising behind it; WithSkiaWeb's lazy loader awaits the same shared
 * promise. The testID sits on this screen root (the probe's handle); the Skia
 * charts mount inside once CanvasKit is up.
 *
 * Hydration is read with useSyncExternalStore rather than a setState-in-effect
 * flag: the server snapshot is false and the client snapshot true, which is
 * exactly the question being asked, and it renders once instead of twice.
 */
const SKIA_WASM_BASE =
  'https://cdn.jsdelivr.net/npm/canvaskit-wasm@0.41.0/bin/full';

if (Platform.OS === 'web' && typeof window !== 'undefined') {
  void LoadSkiaWeb({
    locateFile: (file) => `${SKIA_WASM_BASE}/${file}`,
  });
}

const FALLBACK = (
  <Text p="$4" testID="ChartSpikeLoading">
    Loading Skia…
  </Text>
);

/** Never fires: the value flips once, when the client takes over rendering. */
const subscribeNever = () => () => {};

function useHydrated(): boolean {
  return useSyncExternalStore(
    subscribeNever,
    () => true,
    () => false
  );
}

export default function ChartSpikeScreen() {
  const hydrated = useHydrated();

  return (
    <View testID="ChartSpike">
      {hydrated ? (
        <WithSkiaWeb
          // native-verified: n/a, WithSkiaWeb is the web-only CanvasKit loader and this spike route renders only there
          getComponent={() => import('@/components/charts-spike/ChartSpike')}
          fallback={FALLBACK}
          opts={{
            locateFile: (file) => `${SKIA_WASM_BASE}/${file}`,
          }}
        />
      ) : (
        FALLBACK
      )}
    </View>
  );
}