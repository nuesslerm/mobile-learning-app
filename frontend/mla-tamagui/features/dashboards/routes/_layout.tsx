import { Stack } from 'expo-router';

import { DashboardsContainer } from '../di/DashboardsContainer';

export default function DashboardsLayout() {
  return (
    <DashboardsContainer>
      <Stack screenOptions={{ headerShown: false }} />
    </DashboardsContainer>
  );
}
