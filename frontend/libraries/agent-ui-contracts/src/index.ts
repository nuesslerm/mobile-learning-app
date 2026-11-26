export type {
  AgentComponents,
  PairDesktopButtonProps,
  UnpairedHomeProps,
  NotificationChoicesProps,
  OnboardingChoiceProps,
  HomeTabId,
  HomeTabDescriptor,
  HomeTabBarProps,
  HomeIconKind,
  HomeTabHeaderAction,
  HomeTabHeaderProps,
  HomeEmptyStateProps,
  HomeSearchFieldProps,
  HomeActionSheetAction,
  HomeActionSheetProps,
  HomeSettingsRowProps,
  HomeSettingsGroupProps,
  HomeSettingsScreenProps,
  HomeSettingsNoteProps,
} from './ports/AgentComponents';
export {
  HOME_SETTINGS_SEPARATOR_INSET,
  HOME_SETTINGS_SEPARATOR_INSET_PLAIN,
} from './ports/AgentComponents';
export {
  AgentComponentsProvider,
  useAgentComponents,
} from './core/AgentComponentsContext';

export type { ChatToolActivityProps, ChatImageProps, ChatImageViewerProps, ComposerProps, ComposerSendButtonProps, MessageBubbleProps } from './ports/ChatComponents';
export type { PairingScreenProps, PairingCodeSheetProps } from './ports/PairingComponents';
export type { ComposerOptionsProps, ChatMessageTextProps, ChatMessageMetaProps, SessionHeaderProps, SessionTabsSheetProps } from './ports/ChatComponents';

export type { WorkspaceOptionGroup, WorkspaceOptionRow, WorkspaceOptionsProps } from './ports/WorkspaceOptions';

export type { HostWorkspaceListProps } from './ports/HostWorkspaceList';
