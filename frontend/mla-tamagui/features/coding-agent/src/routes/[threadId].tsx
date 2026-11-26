import { useLocalSearchParams } from 'expo-router';
import { T3MobileScreen } from '../ui/T3MobileScreen';

export default function CodingAgentThreadRoute() {
  const { threadId } = useLocalSearchParams<{ threadId: string }>();
  return <T3MobileScreen threadId={threadId} />;
}
