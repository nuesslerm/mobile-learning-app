import { PairedDesktops } from './adapters/PairedDesktops';
import { WorkspaceActions } from './adapters/WorkspaceActions';
import { HostWorkspaceList } from './adapters/HostWorkspaceList';
import { WorkspaceOptions } from './adapters/WorkspaceOptions';
import { SessionTabsSheet } from './adapters/SessionTabsSheet';
import { SessionHeader } from './adapters/SessionHeader';
import { ChatMessageText } from './adapters/ChatMessageText';
import { ChatMessageMeta } from './adapters/ChatMessageMeta';
import { HomeSurface } from './adapters/HomeSurface';
import { HomeSettingsGroup } from './adapters/HomeSettingsGroup';
import { HomeSettingsNote } from './adapters/HomeSettingsNote';
import { HomeSettingsScreen } from './adapters/HomeSettingsScreen';

import { PairingScreen } from './adapters/PairingScreen';
import { PairingCodeSheet } from './adapters/PairingCodeSheet';
import { ChatToolActivity } from './adapters/ChatToolActivity';
import { ChatSurface, ChatImage, ChatImageViewer } from './adapters/ChatImages';
import { HomeTabBar } from './adapters/HomeTabBar';
import { HomeTabHeader } from './adapters/HomeTabHeader';
import { HomeEmptyState } from './adapters/HomeEmptyState';
import { HomeSearchField } from './adapters/HomeSearchField';
import { HomeActionSheet } from './adapters/HomeActionSheet';
import { HomeSettingsRow } from './adapters/HomeSettingsRow';
import { ComposerAddButton, ComposerSuggestionList } from './adapters/ComposerSuggestions';
import { ComposerOptions } from './adapters/ComposerOptions';
import { Composer, ComposerSendButton } from './adapters/Composer';
import { MessageBubble } from './adapters/MessageBubble';
import { UnpairedHome } from './adapters/UnpairedHome';
import { PairDesktopButton } from './adapters/PairDesktopButton';
import { BrandMark } from './adapters/BrandMark';
import type { AgentComponents } from '@lib/agent-ui-contracts';
import { NotificationChoices } from './core/NotificationChoices';
import { OnboardingChoice } from './adapters/OnboardingChoice';

export const orcaComponents: AgentComponents = {
  PairedDesktops, WorkspaceActions, HostWorkspaceList, WorkspaceOptions, ComposerOptions,
  PairingScreen, PairingCodeSheet,
  HomeSurface,
  HomeTabBar,
  HomeTabHeader,
  HomeEmptyState,
  HomeSearchField,
  HomeActionSheet,
  HomeSettingsRow, HomeSettingsGroup, HomeSettingsNote, HomeSettingsScreen,
  Composer,
  ComposerSendButton, ComposerAddButton, ComposerSuggestionList,
  SessionHeader, SessionTabsSheet, ChatMessageText, ChatMessageMeta, ChatToolActivity,
  ChatSurface, ChatImage, ChatImageViewer,
  MessageBubble,
  BrandMark,
  PairDesktopButton,
  UnpairedHome,
  NotificationChoices,
  OnboardingChoice,
};

export { homePalettes, useOrcaScheme } from './adapters/appearance';
export type { AppearancePreference } from './adapters/appearance';
export {
  AppearancePreferenceContextProvider,
  useAppearancePreference,
} from './adapters/appearance';

export { ChatTextContext, useChatText } from './adapters/chatText';
export { ChatSizeControl, ChatSpacingControl } from './adapters/ChatTextControls';
