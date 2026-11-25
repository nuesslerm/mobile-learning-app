import type { ComponentType } from 'react';

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

export interface AgentComponents {
  OnboardingChoice: ComponentType<OnboardingChoiceProps>;
  NotificationChoices: ComponentType<NotificationChoicesProps>;
}
