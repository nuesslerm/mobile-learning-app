import type { ReactNode } from 'react';

/** One pressable key or tool in a workspace tab's control rows. */
export interface WorkspaceTabControlProps {
  label: string;
  children?: ReactNode;
  terminal?: boolean;
  browser?: 'tool' | 'key';
  display?: string;
  onPress: () => void;
  selected?: boolean;
  disabled?: boolean;
}
