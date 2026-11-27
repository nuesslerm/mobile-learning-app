import {
  useAgentComponents,
  useAgentTheme,
  type WorkspaceTabControlProps,
} from '@lib/agent-ui-contracts';

export function useTabColors() {
  return useAgentTheme().tabs;
}

export function TabControl(props: WorkspaceTabControlProps) {
  const { WorkspaceTabControl } = useAgentComponents();
  return <WorkspaceTabControl {...props} />;
}
