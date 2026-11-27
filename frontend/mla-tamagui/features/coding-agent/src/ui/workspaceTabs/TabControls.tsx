import {
  useSkin,
  useSkinTheme,
  type WorkspaceTabControlProps,
} from '@lib/skin';

export function useTabColors() {
  return useSkinTheme().tabs;
}

export function TabControl(props: WorkspaceTabControlProps) {
  const { WorkspaceTabControl } = useSkin();
  return <WorkspaceTabControl {...props} />;
}
