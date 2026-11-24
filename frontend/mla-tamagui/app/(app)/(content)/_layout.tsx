import { useScreenHeaderOptions } from '@lib/tamagui-design-system';
import { router, Stack } from 'expo-router';
import { useDrawerProgress } from 'expo-router/drawer';
import { StyleSheet } from 'react-native';
import Animated, { useAnimatedStyle } from 'react-native-reanimated';

/**
 * The inner Stack every drawer destination lives in.
 *
 * `index` (the problem list) carries its own collapsing header, so it opts out
 * of this Stack's header rather than stacking two.
 */
export default function ContentLayout() {
  const drawerProgress = useDrawerProgress();
  const pageStyle = useAnimatedStyle(() => ({
    borderRadius: 28 * drawerProgress.value,
    transform: [{ scale: 1 - 0.06 * drawerProgress.value }],
  }));
  const shadeStyle = useAnimatedStyle(() => ({
    opacity: 0.24 * drawerProgress.value,
  }));
  const serviceReportHeader = useScreenHeaderOptions({
    glass: true,
    testID: 'ServiceReportHeader',
    title: 'Service Report',
    onBack: () => router.back(),
  });
  return (
    <Animated.View style={[styles.page, pageStyle]}>
      <ContentStack serviceReportHeader={serviceReportHeader} />
      <Animated.View
        pointerEvents="none"
        style={[StyleSheet.absoluteFill, styles.shade, shadeStyle]}
      />
    </Animated.View>
  );
}

function ContentStack({
  serviceReportHeader,
}: {
  serviceReportHeader: ReturnType<typeof useScreenHeaderOptions>;
}) {
  return (
    <Stack>
      <Stack.Screen name="index" options={{ headerShown: false }} />
      <Stack.Screen name="showcase" options={{ headerShown: false, title: 'Design system' }} />
      <Stack.Screen name="coding-agent" options={{ headerShown: false }} />
      {/* The Tamil screens carry their own headers; the tamil/ layout mounts the feature's container and owns the nested Stack. */}
      <Stack.Screen name="tamil" options={{ headerShown: false }} />
      {/* settings and service-report are files in THIS group, so their
          screens register here. Registered on the root Stack instead, expo-router
          reports `No route named "settings" exists in nested children` and the
          route is unreachable -- while typecheck and a grep for the name both
          still pass. */}
      <Stack.Screen
        name="settings/index"
        options={{ headerShown: false }}
      />
      <Stack.Screen
        name="settings/profile"
        options={{ headerShown: false }}
      />
      <Stack.Screen
        name="settings/appearance"
        options={{ headerShown: false }}
      />
      <Stack.Screen
        name="settings/notifications"
        options={{ headerShown: false }}
      />
      <Stack.Screen
        name="settings/storage"
        options={{ headerShown: false }}
      />
      <Stack.Screen name="settings/about" options={{ headerShown: false }} />
      <Stack.Screen name="dashboards" options={{ headerShown: false }} />
      <Stack.Screen
        name="service-report"
        options={serviceReportHeader}
      />
    </Stack>
  );
}

const styles = StyleSheet.create({
  page: { flex: 1, overflow: 'hidden', borderCurve: 'continuous' },
  shade: { backgroundColor: '#000' },
});
