import { router, Stack } from 'expo-router';

import { ScreenHeader } from '@lib/tamagui-design-system';

import { LeetCodeLiveContainer } from '../di/LeetCodeContainer';
import { LeetCodeFixtureOverrides } from '../di/fixtureOverrides';
import { ExpoAudioRecorderProvider } from '../data/ExpoAudioRecorderAdapter';

export default function LeetCodeLayout() {
  return (
    <LeetCodeFixtureOverrides>
      <ExpoAudioRecorderProvider>
        <LeetCodeLiveContainer>
          <Stack>
            {/* Overlays are routes, so the screen that opens one does not
              have to share a React tree with it. Each carries its own sheet
              or dialog chrome, so `headerShown: false`: the stack header
              would otherwise paint an untitled bar -- captioned with the
              route pattern, [id]/description -- across the top of the screen
              the overlay is supposed to be floating over. */}
            <Stack.Screen
              name="[id]/description"
              options={{
                presentation: 'transparentModal',
                headerShown: false,
                // The overlay owns its own entrance animation (gorhom's sheet
                // slide, the dialog's fade). Without this the screen-level
                // transition doubles up and the sheet snaps partway open.
                animation: 'none',
              }}
            />
            <Stack.Screen
              name="[id]/rename"
              options={{
                presentation: 'transparentModal',
                headerShown: false,
                // The overlay owns its own entrance animation (gorhom's sheet
                // slide, the dialog's fade). Without this the screen-level
                // transition doubles up and the sheet snaps partway open.
                animation: 'none',
              }}
            />
            <Stack.Screen
              name="[id]/reset-code"
              options={{
                presentation: 'transparentModal',
                headerShown: false,
                // The overlay owns its own entrance animation (gorhom's sheet
                // slide, the dialog's fade). Without this the screen-level
                // transition doubles up and the sheet snaps partway open.
                animation: 'none',
              }}
            />
            <Stack.Screen
              name="[id]/unsaved"
              options={{
                presentation: 'transparentModal',
                headerShown: false,
                animation: 'none',
              }}
            />
            <Stack.Screen
              name="[id]/history"
              options={{
                presentation: 'transparentModal',
                headerShown: false,
                // The overlay owns its own entrance animation (gorhom's sheet
                // slide, the dialog's fade). Without this the screen-level
                // transition doubles up and the sheet snaps partway open.
                animation: 'none',
              }}
            />
            {/* Full screens, so they keep a header -- but it has to be the
              app's own, or the title falls back to the route pattern. */}
            <Stack.Screen
              name="login"
              options={{
                header: () => (
                  <ScreenHeader
                    testID="LeetCodeLoginHeader"
                    title="Sign in"
                    subtitle="LeetCode"
                    onBack={() => router.back()}
                  />
                ),
              }}
            />
            <Stack.Screen
              name="toolbar"
              options={{
                header: () => (
                  <ScreenHeader
                    testID="ToolbarConfigHeader"
                    title="Toolbar"
                    subtitle="Editor shortcuts"
                    onBack={() => router.back()}
                  />
                ),
              }}
            />
          </Stack>
        </LeetCodeLiveContainer>
      </ExpoAudioRecorderProvider>
    </LeetCodeFixtureOverrides>
  );
}