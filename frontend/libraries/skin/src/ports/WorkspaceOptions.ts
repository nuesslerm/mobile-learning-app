export interface WorkspaceOptionRow {
  label: string;
  description?: string;
  value?: string;
  selected?: boolean;
  kind?: 'page' | 'radio' | 'checkbox' | 'switch';
  onPress: () => void;
}

export interface WorkspaceOptionGroup {
  label?: string;
  segments?: boolean;
  rows: WorkspaceOptionRow[];
}

export interface WorkspaceOptionsProps {
  visible: boolean;
  page?: string;
  groups: WorkspaceOptionGroup[];
  pageGroups: WorkspaceOptionGroup[];
  loading: boolean;
  saving: boolean;
  error?: string;
  resetEnabled: boolean;
  onOpen: () => void;
  onClose: () => void;
  onBack: () => void;
  onReset: () => void;
  onRetry: () => void;
}
