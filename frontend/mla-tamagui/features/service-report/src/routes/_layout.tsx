import { Stack } from 'expo-router';

import { ServiceReportContainer } from '../di/ServiceReportContainer';

export default function ServiceReportLayout() {
  return (
    <ServiceReportContainer>
      <Stack screenOptions={{ headerShown: false }} />
    </ServiceReportContainer>
  );
}
