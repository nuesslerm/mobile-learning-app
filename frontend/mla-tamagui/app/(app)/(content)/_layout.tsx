import { ScreenHeader } from '@lib/tamagui-design-system';
import { router, Stack } from 'expo-router';

/**
 * The inner Stack every drawer destination lives in.
 *
 * `index` (the problem list) carries its own collapsing header, so it opts out
 * of this Stack's header rather than stacking two.
 */
export default function ContentLayout() {
  return (
    <Stack>
      <Stack.Screen name="index" options={{ headerShown: false }} />
      <Stack.Screen name="showcase" options={{ title: 'Design system' }} />
      <Stack.Screen
        name="coding-agent/index"
        options={{ headerShown: false }}
      />
      <Stack.Screen
        name="coding-agent/[threadId]"
        options={{ headerShown: false }}
      />
      <Stack.Screen
        name="coding-agent/runs/[runId]"
        options={{ headerShown: false }}
      />
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
        options={{
          header: () => (
            <ScreenHeader
              testID="ServiceReportHeader"
              title="Service Report"
              onBack={() => router.back()}
            />
          ),
        }}
      />
    </Stack>
  );
}
