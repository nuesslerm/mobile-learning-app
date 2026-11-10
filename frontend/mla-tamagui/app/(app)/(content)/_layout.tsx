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
      {/* Both Tamil screens carry their own header: the course screen a
          collapsing hero, the lesson screen a ScreenHeader whose title is the
          lesson's own Tamil name. */}
      <Stack.Screen name="tamil/index" options={{ headerShown: false }} />
      <Stack.Screen name="tamil/[lessonId]" options={{ headerShown: false }} />
      <Stack.Screen name="tamil/review" options={{ headerShown: false }} />
      <Stack.Screen name="tamil/tracks/index" options={{ headerShown: false }} />
      <Stack.Screen name="tamil/tracks/[trackId]" options={{ headerShown: false }} />
      {/* Left unregistered, expo-router draws its own bar with the route's
          file name -- "tamil/write/[characterId]" sat above the screen's
          real header. */}
      <Stack.Screen name="tamil/write/[subjectId]" options={{ headerShown: false }} />
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
      <Stack.Screen
        name="dashboards/index"
        options={{ headerShown: false }}
      />
      <Stack.Screen
        name="dashboards/infra"
        options={{ headerShown: false }}
      />
      <Stack.Screen
        name="dashboards/agents"
        options={{ headerShown: false }}
      />
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
