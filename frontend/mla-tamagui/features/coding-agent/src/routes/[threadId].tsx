import { useLocalSearchParams } from 'expo-router';
import { CodingAgentThread } from '../ui/MobileThreadScreen';

export default function CodingAgentThreadRoute() {
  const { threadId, paired, presentation } = useLocalSearchParams<{ threadId: string; paired?: string; presentation?: string }>();
  return <CodingAgentThread key={(threadId ?? '') + (paired ?? '')} id={threadId} orca={presentation === 'orca'} />;
}
