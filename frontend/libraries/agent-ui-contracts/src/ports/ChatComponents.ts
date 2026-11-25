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
