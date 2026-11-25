import type { ReactNode } from 'react';

export interface MessageBubbleProps {
  role: 'user' | 'assistant';
  children: ReactNode;
  footer?: ReactNode;
  onLongPress?: () => void;
}

export interface ComposerSendButtonProps {
  label: string;
  canSend: boolean;
  sending?: boolean;
  onPress: () => void;
  onLongPress?: () => void;
}

export interface ComposerProps {
  value: string;
  placeholder: string;
  sendLabel: string;
  canSend: boolean;
  sending?: boolean;
  leadingControls?: ReactNode;
  trailingControls?: ReactNode;
  onChangeText: (text: string) => void;
  onSend: () => void;
}

export interface ChatImageProps {
  label: string;
  uri?: string;
  unavailable?: boolean;
  onOpen: () => void;
}
export interface ChatImageViewerProps {
  label: string;
  uri: string;
  topInset: number;
  rightInset: number;
  onClose: () => void;
}

export interface ChatToolActivityProps {
  summary: string;
  working: boolean;
  rows: readonly {id:string; name:string; input:string; output:string; status:string}[];
  children?: ReactNode;
}

export interface ChatMessageTextProps { text: string }
export interface ChatMessageMetaProps {
  time?: string;
  accessibleTime?: string;
  onCopy?: () => Promise<boolean>;
}

export interface SessionHeaderProps {
  workspace: string;
  title: string;
  connected: boolean;
  tabCount: number;
  topInset: number;
  onBack: () => void;
  onReconnect: () => void;
  onShowTabs: () => void;
}

export interface SessionTabsSheetProps {
  visible: boolean;
  workspace: string;
  sessions: readonly { id: string; title: string }[];
  activeId: string;
  bottomInset: number;
  error?: string;
  onClose: () => void;
  onRetry: () => void;
  onSelect: (id: string) => void;
}
