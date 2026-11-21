import { Stack } from 'expo-router';

import { TamilWritingContainer } from '../di/TamilWritingContainer';

export default function TamilWritingLayout() {
  return (
    <TamilWritingContainer>
      <Stack screenOptions={{ headerShown: false }} />
    </TamilWritingContainer>
  );
}
