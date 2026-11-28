import type { ReactNode } from 'react';

export interface HostWorkspaceListProps {
  title: string;
  platform?: string;
  offline?: boolean;
  sections: {
    key: string;
    label?: string;
    icon?: 'pin' | 'folder';
    rows: { id: string; name: string; detail?: string; project?: string; projectInTitle?: boolean; unread?: boolean }[];
  }[];
  disclosure?: { keys: ReadonlySet<string>; ready: boolean; change: (key: string, collapsed: boolean) => void };
  search?: { value: string; onChange: (value: string) => void };
  options?: ReactNode;
  notice?: string;
  availability?: ReactNode;
  retry?: () => void;
  onBack: () => void;
  onActions?: (id: string) => void;
  onSelect: (id: string) => void;
}

export interface WorkspaceActionsProps {
  name: string;
  automaticName: string;
  branch: string | null;
  pinned: boolean;
  unread: boolean;
  saving: boolean;
  error?: string;
  onClose: () => void;
  onRename: (name: string) => void;
  onPin: () => void;
  onSourceControl: () => void;
  onToggleRead: () => void;
  statusPicker?: {
    choices?: { id: string; label: string }[];
    selected?: string;
    failed: boolean;
  };
  onOpenStatuses: () => void;
  onStatus: (id: string) => void;
}
