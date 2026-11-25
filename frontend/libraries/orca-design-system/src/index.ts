import { UnpairedHome } from './adapters/UnpairedHome';
import { PairDesktopButton } from './adapters/PairDesktopButton';
import { BrandMark } from './adapters/BrandMark';
import type { AgentComponents } from '@lib/agent-ui-contracts';
import { NotificationChoices } from './core/NotificationChoices';
import { OnboardingChoice } from './adapters/OnboardingChoice';

export const orcaComponents: AgentComponents = {
  BrandMark,
  PairDesktopButton,
  UnpairedHome,
  NotificationChoices,
  OnboardingChoice,
};
