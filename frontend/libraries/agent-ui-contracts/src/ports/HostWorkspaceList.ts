import type { ReactNode } from 'react';

export interface HostWorkspaceListProps {
  title: string;
  platform?: string;
  group?: string;
  rows: { id: string; name: string; detail?: string; pinned?: boolean; unread?: boolean }[];
  search?: { value: string; onChange: (value: string) => void };
  options?: ReactNode;
  notice?: string;
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
