import { createFlowApp } from '@lib/flow-test-harness';
import { useLocalSearchParams } from 'expo-router';
import type { ComponentType } from 'react';
import { CodingAgentHome } from '../ui/MobileWorkspacesScreen';
import { CodingAgentThread } from '../ui/MobileThreadScreen';

function LegacyThread() {
  const { threadId, presentation, paired } = useLocalSearchParams<{ threadId: string; presentation?: string; paired?: string }>();
  return <CodingAgentThread key={threadId + (paired ?? '')} id={threadId} orca={presentation === 'orca'} />;
}

export const app = createFlowApp({
  routes: {
    ...import.meta.glob<{ default: ComponentType }>('../routes/**/*.tsx'),
    '../routes/index.tsx': { default: CodingAgentHome },
    '../routes/[threadId].tsx': { default: LegacyThread },
  },
  routesRoot: '../routes/',
  basePath: '/coding-agent',
  includeRootLayout: true,
});
