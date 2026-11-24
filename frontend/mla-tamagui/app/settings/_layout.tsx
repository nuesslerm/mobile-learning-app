import { Stack } from 'expo-router';

export const unstable_settings = { anchor: 'index' };

export default function SettingsLayout() {
  return <Stack screenOptions={{ headerShown: false }} />;
}
