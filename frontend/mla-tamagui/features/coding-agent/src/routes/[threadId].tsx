import { useLocalSearchParams } from 'expo-router';
import { T3MobileScreen } from '../ui/T3MobileScreen';
import { CodingAgentThread } from '../ui/MobileThreadScreen';

export default function CodingAgentThreadRoute() {
  const { threadId, paired, presentation, transport } = useLocalSearchParams<{ threadId: string; paired?: string; presentation?: string; transport?: string }>();
  if (presentation === 'orca' && transport === 't3') return <T3MobileScreen key={threadId} threadId={threadId} />;
  return <CodingAgentThread key={(threadId ?? '') + (paired ?? '')} id={threadId} orca={presentation === 'orca'} />;
}
