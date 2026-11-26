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

export interface ComposerSuggestion {
  kind: 'file' | 'skill' | 'command';
  token: string;
  description?: string;
  hint?: string;
  aliases?: readonly string[];
}

export interface ComposerSuggestions {
  readonly commands?: readonly ComposerSuggestion[];
  files(query: string): Promise<readonly ComposerSuggestion[]>;
  catalog(): Promise<readonly ComposerSuggestion[]>;
}

export interface ComposerAddButtonProps {
  onSelect: (kind: ComposerSuggestion['kind']) => void;
  onDismiss: () => void;
}

export interface ComposerSuggestionListProps {
  source: ComposerSuggestions;
  sigil: string;
  query: string;
  kind: ComposerSuggestion['kind'] | null;
  onPick: (row: ComposerSuggestion) => void;
}

export interface ComposerProps {
  suggestions?: ComposerSuggestions;
  notice?: string;
  onPickSuggestion?: (row: ComposerSuggestion, leading: boolean) => boolean;
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
  actions?: Partial<Record<'copy' | 'save' | 'share', () => Promise<void>>>;
  bottomInset?: number;
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

export interface ChatTextPreference {
  size: number;
  font: 'system' | 'serif' | 'mono';
  lineSpacing: 'compact' | 'default' | 'relaxed';
}

export interface ChatMessageTextProps {
  text: string;
  tone?: 'user' | 'assistant';
  allowCodeWrap?: boolean;
  copyCode?: (text: string) => Promise<boolean>;
}
export interface ChatMessageMetaProps {
  time?: string;
  accessibleTime?: string;
  onCopy?: () => Promise<boolean>;
}

export interface SessionHeaderProps {
  onOpenSourceControl?: () => void;
  onOpenTextSettings?: () => void;
  files?: {
    listDirectory(path: string): Promise<{ entries: {path: string; kind: 'file' | 'directory'}[]; truncated: boolean }>;
    readFile(path: string): Promise<{contents: string; truncated: boolean}>;
  };
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

export interface ComposerOptionsProps {
  visible: boolean;
  effort?: {
    choices: readonly { slug: string; name: string }[];
    current?: string;
    select: (value: string) => Promise<void>;
  };
  onVisibleChange: (visible: boolean) => void;
  provider: string;
  selectedModel?: string;
  permission?: string;
  permissions: readonly { slug: string; name: string }[];
  loadModels: () => Promise<readonly { slug: string; name: string }[]>;
  setModel: (model: string) => Promise<void>;
  setPermission: (mode: string) => Promise<void>;
}
