import type { ReactNode } from 'react';

export interface HostWorkspaceListProps {
  title: string;
  platform?: string;
  group?: string;
  rows: { id: string; name: string; detail?: string }[];
  search?: { value: string; onChange: (value: string) => void };
  options?: ReactNode;
  notice?: string;
  retry?: () => void;
  onBack: () => void;
  onSelect: (id: string) => void;
}
