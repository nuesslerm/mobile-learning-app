export type {
  Skin,
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
  HomeSettingsSectionTitleProps,
  HomeSettingsSpacerProps,
  ChatTextPreviewProps,
  OptionSheetProps,
} from './ports/Skin';
export {
  HOME_SETTINGS_SEPARATOR_INSET,
  HOME_SETTINGS_SEPARATOR_INSET_PLAIN,
} from './ports/Skin';
export {
  SkinProvider,
  useSkin,
} from './core/SkinContext';

export type { ChatTextPreference, ChatToolActivityProps, ChatImageProps, ChatImageViewerProps, ComposerSuggestion, ComposerSuggestions, ComposerProps, ComposerAddButtonProps, ComposerSuggestionListProps, ComposerSendButtonProps, MessageBubbleProps } from './ports/ChatComponents';
export type { PairingScreenProps, PairingCodeSheetProps } from './ports/PairingComponents';
export type { ComposerOptionsProps, ChatMessageTextProps, ChatMessageMetaProps, SessionHeaderProps, NewTabSheetProps, SessionTabsSheetProps } from './ports/ChatComponents';

export type { WorkspaceOptionGroup, WorkspaceOptionRow, WorkspaceOptionsProps } from './ports/WorkspaceOptions';

export type { HostWorkspaceListProps, WorkspaceActionsProps } from './ports/HostWorkspaceList';

export type { SourceControlProps, SourceCommit, SourceFile, SourceCodeLine, SourceFileReview } from './ports/SourceControl';

export type { SkinTheme, SkinColors, SkinSyntaxColors, SkinTabColors } from './ports/SkinTheme';
export { SkinThemeProvider, useSkinTheme } from './core/SkinThemeContext';
export type { AppearancePreference, AppearancePreferenceValue } from './core/appearance';
export { AppearancePreferenceContextProvider, useAppearancePreference } from './core/appearance';
export { ChatTextContext, useChatText } from './core/chatText';
export type { BottomSheetProps } from './ports/BottomSheet';
export type { WorkspaceTabControlProps } from './ports/WorkspaceTabComponents';
export type { SkinPreference, SkinPreferenceValue } from './core/skinPreference';
export { ActiveSkinThemeProvider, SkinPreferenceProvider, SkinThemeBoundary, useSkinPreference } from './core/skinPreference';
export { BRAND_MARK } from './core/brandMark';
export { useComposerOptions } from './core/composerOptions';
export { ADD_CHOICES, fileToken, suggestionTrigger, useComposerEditing, useComposerSuggestionRows } from './core/composer';
export { GALLERY_ENTRIES, GALLERY_SECTIONS, SkinGallery } from './core/gallery';
export type { GallerySection } from './core/gallery';
export type { LayoutProps, Space, Surface, TextProps } from './ports/Layout';
export type { ButtonProps, NoticeProps, SpinnerProps, SwitchProps, TextFieldProps } from './ports/Controls';
