import { PairingScreen } from './adapters/PairingScreen';
import { PairingCodeSheet } from './adapters/PairingCodeSheet';
import { ChatToolActivity } from './adapters/ChatToolActivity';
import { ChatSurface, ChatImage, ChatImageViewer } from './adapters/ChatImages';
import { Composer, ComposerSendButton } from './adapters/Composer';
import { MessageBubble } from './adapters/MessageBubble';
import { UnpairedHome } from './adapters/UnpairedHome';
import { PairDesktopButton } from './adapters/PairDesktopButton';
import { BrandMark } from './adapters/BrandMark';
import type { AgentComponents } from '@lib/agent-ui-contracts';
import { NotificationChoices } from './core/NotificationChoices';
import { OnboardingChoice } from './adapters/OnboardingChoice';

export const orcaComponents: AgentComponents = {
  PairingScreen, PairingCodeSheet,
  Composer,
  ComposerSendButton,
  ChatToolActivity,
  ChatSurface, ChatImage, ChatImageViewer,
  MessageBubble,
  BrandMark,
  PairDesktopButton,
  UnpairedHome,
  NotificationChoices,
  OnboardingChoice,
};
