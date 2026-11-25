import { useResolvedScheme } from '@/app-shell/NativeChromeBridge';
import { useScreenHeaderOptions } from '@lib/tamagui-design-system';
import { router, Stack } from 'expo-router';
import { useDrawerProgress } from 'expo-router/drawer';
import { Platform, StyleSheet } from 'react-native';
import Animated, { useAnimatedStyle } from 'react-native-reanimated';

/**
 * The inner Stack every drawer destination lives in.
 *
 * `index` (the problem list) carries its own collapsing header, so it opts out
 * of this Stack's header rather than stacking two.
 */
export default function ContentLayout() {
  const drawerProgress = useDrawerProgress();
  const scheme = useResolvedScheme();
  const ios = Platform.OS === 'ios';
  const dark = scheme === 'dark';
  const pageStyle = useAnimatedStyle(() => ({
    borderTopStartRadius: (ios ? 66 : 42) * drawerProgress.value,
    borderBottomStartRadius: (ios ? 66 : 42) * drawerProgress.value,
  }));
  const shadowStyle = useAnimatedStyle(() => ({
    shadowOpacity: 0.18 * drawerProgress.value,
  }));
  const shadeStyle = useAnimatedStyle(() => ({
    opacity: ios && dark
      ? 0.72 * Math.min(drawerProgress.value * 2, 1)
      : 0.24 * drawerProgress.value,
  }));
  const edgeStyle = useAnimatedStyle(() => ({
    opacity: 0.39 * drawerProgress.value,
  }));
  const serviceReportHeader = useScreenHeaderOptions({
    glass: true,
    testID: 'ServiceReportHeader',
    title: 'Service Report',
    onBack: () => router.back(),
  });
  return (
    <Animated.View style={[styles.frame, ios && shadowStyle]}>
      <Animated.View style={[styles.page, pageStyle]}>
        <ContentStack serviceReportHeader={serviceReportHeader} />
        <Animated.View
          pointerEvents="none"
          style={[StyleSheet.absoluteFill, ios && dark ? styles.darkShade : styles.shade, shadeStyle]}
        />
      </Animated.View>
      {ios && dark && (
        <Animated.View pointerEvents="none" style={[StyleSheet.absoluteFill, styles.edge, pageStyle, edgeStyle]} />
      )}
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
      <Stack.Screen name="blind75" options={{ headerShown: false }} />
      <Stack.Screen name="showcase" options={{ headerShown: false, title: 'Design system' }} />
      <Stack.Screen name="coding-agent" options={{ headerShown: false }} />
      {/* The Tamil screens carry their own headers; the tamil/ layout mounts the feature's container and owns the nested Stack. */}
      <Stack.Screen name="tamil" options={{ headerShown: false }} />
      <Stack.Screen name="dashboards" options={{ headerShown: false }} />
      <Stack.Screen
        name="service-report"
        options={serviceReportHeader}
      />
    </Stack>
  );
}

const styles = StyleSheet.create({
  frame: { flex: 1, shadowColor: '#000', shadowRadius: 4, shadowOffset: { width: -2, height: 0 } },
  page: { flex: 1, overflow: 'hidden', borderCurve: 'continuous' },
  shade: { backgroundColor: '#000' },
  darkShade: { backgroundColor: '#1d1d1d' },
  edge: { borderColor: '#646464', borderWidth: StyleSheet.hairlineWidth, borderCurve: 'continuous' },
});
