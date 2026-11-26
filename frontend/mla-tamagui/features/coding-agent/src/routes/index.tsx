import { Stack, useLocalSearchParams } from 'expo-router';
import { T3MobileScreen } from '../ui/T3MobileScreen';

export default function CodingAgentHome() {
  const { workspace } = useLocalSearchParams<{ workspace?: string }>();
  return (
    <>
      <Stack.Screen
        options={{
          gestureEnabled: workspace !== '1',
          orientation: workspace === '1' ? 'all' : 'portrait',
        }}
      />
      <T3MobileScreen />
    </>
  );
}
