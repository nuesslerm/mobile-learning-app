import { useLocalSearchParams } from 'expo-router';
import { CodingAgentThread } from '../ui/MobileThreadScreen';

export default function CodingAgentThreadRoute() {
  const { threadId, paired } = useLocalSearchParams<{ threadId: string; paired?: string }>();
  return <CodingAgentThread key={(threadId ?? '') + (paired ?? '')} id={threadId} />;
}
