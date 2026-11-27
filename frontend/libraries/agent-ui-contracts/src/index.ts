export type {
  AgentComponents,
  PairedDesktopsProps,
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
  HomeSearchFieldProps, HomeAgentSectionProps,
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

export type { ChatTextPreference, ChatToolActivityProps, ChatImageProps, ChatImageViewerProps, ComposerSuggestion, ComposerSuggestions, ComposerProps, ComposerAddButtonProps, ComposerSuggestionListProps, ComposerSendButtonProps, MessageBubbleProps } from './ports/ChatComponents';
export type { PairingScreenProps, PairingCodeSheetProps } from './ports/PairingComponents';
export type { ComposerOptionsProps, ChatMessageTextProps, ChatMessageMetaProps, SessionHeaderProps, NewTabSheetProps, SessionTabsSheetProps } from './ports/ChatComponents';

export type { WorkspaceOptionGroup, WorkspaceOptionRow, WorkspaceOptionsProps } from './ports/WorkspaceOptions';

export type { HostWorkspaceListProps, WorkspaceActionsProps } from './ports/HostWorkspaceList';

export type { SourceControlProps, SourceCommit, SourceFile, SourceCodeLine, SourceFileReview } from './ports/SourceControl';

export type { AgentTheme, AgentColors, AgentSyntaxColors, AgentTabColors } from './ports/AgentTheme';
export { AgentThemeProvider, useAgentTheme } from './core/AgentThemeContext';
export type { AppearancePreference, AppearancePreferenceValue } from './core/appearance';
export { AppearancePreferenceContextProvider, useAppearancePreference } from './core/appearance';
export { ChatTextContext, useChatText } from './core/chatText';
export type { BottomSheetProps } from './ports/BottomSheet';
export type { WorkspaceTabControlProps } from './ports/WorkspaceTabComponents';
