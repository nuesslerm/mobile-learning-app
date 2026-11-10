import { useLocalSearchParams } from 'expo-router';

import { ThreadScreen } from '../ui/ThreadScreen';

export default function CodingAgentThreadRoute() {
  const params = useLocalSearchParams<{ threadId: string }>();

  return <ThreadScreen threadId={params.threadId} />;
}