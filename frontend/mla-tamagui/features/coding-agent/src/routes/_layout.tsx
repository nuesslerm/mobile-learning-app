import { Stack } from 'expo-router';

import { CodingAgentContainer } from '../di/CodingAgentContainer';

export default function CodingAgentLayout() {
  return (
    <CodingAgentContainer>
      <Stack screenOptions={{ headerShown: false }} />
    </CodingAgentContainer>
  );
}
