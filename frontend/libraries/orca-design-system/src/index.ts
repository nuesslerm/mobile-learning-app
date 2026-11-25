import type { AgentComponents } from '@lib/agent-ui-contracts';
import { NotificationChoices } from './core/NotificationChoices';
import { OnboardingChoice } from './adapters/OnboardingChoice';

export const orcaComponents: AgentComponents = {
  NotificationChoices,
  OnboardingChoice,
};
