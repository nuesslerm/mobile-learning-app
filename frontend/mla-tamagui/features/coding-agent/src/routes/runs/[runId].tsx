import { useLocalSearchParams } from 'expo-router';

import { DispatchRunScreen } from '../../ui/DispatchRunScreen';

export default function CodingAgentRunRoute() {
  const params = useLocalSearchParams<{ runId: string }>();

  return <DispatchRunScreen runId={params.runId} />;
}