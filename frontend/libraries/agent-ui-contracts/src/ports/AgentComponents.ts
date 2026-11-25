import type { ComponentType } from 'react';
import type { ComposerProps, ComposerSendButtonProps, MessageBubbleProps } from './ChatComponents';

export interface OnboardingChoiceProps {
  label: string;
  accessibilityLabel?: string;
  primary?: boolean;
  busy: boolean;
  disabled: boolean;
  onPress: () => void;
}

export interface NotificationChoicesProps {
  enableLabel: string;
  enableAccessibilityLabel: string;
  skipLabel: string;
  skipAccessibilityLabel: string;
  busyChoice: 'enable' | 'skip' | null;
  disabled: boolean;
  onChoice: (choice: 'enable' | 'skip') => void;
}

export interface UnpairedHomeProps {
  contentMaxWidth: number;
  isWideLayout: boolean;
  bottomInset: number;
  title: string;
  description: string;
  steps: readonly { id: string; title: string; description: string }[];
  pairLabel: string;
  onPairDesktop: () => void;
}

export interface PairDesktopButtonProps {
  label: string;
  onPress: () => void;
}

export interface AgentComponents {
  MessageBubble: ComponentType<MessageBubbleProps>;
  Composer: ComponentType<ComposerProps>;
  ComposerSendButton: ComponentType<ComposerSendButtonProps>;
  BrandMark: ComponentType<{ size?: number }>;
  PairDesktopButton: ComponentType<PairDesktopButtonProps>;
  UnpairedHome: ComponentType<UnpairedHomeProps>;
  OnboardingChoice: ComponentType<OnboardingChoiceProps>;
  NotificationChoices: ComponentType<NotificationChoicesProps>;
}
