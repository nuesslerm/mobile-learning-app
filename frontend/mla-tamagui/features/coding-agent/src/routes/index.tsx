import { useLocalSearchParams } from 'expo-router';
import { CodingAgentHome } from '../ui/MobileWorkspacesScreen';
import { OrcaHome } from '../ui/OrcaHome';
import { T3MobileScreen } from '../ui/T3MobileScreen';

export default function CodingAgentHomeRoute() {
  const { presentation, transport } = useLocalSearchParams<{ presentation?: string; transport?: string }>();
  if (presentation === 'orca' && transport === 't3') return <T3MobileScreen />;
  if (presentation === 'orca') return <OrcaHome />;
  return <CodingAgentHome />;
}
