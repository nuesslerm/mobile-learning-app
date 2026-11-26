import type { SourceControlProps } from './SourceControl';
import type { HostWorkspaceListProps, WorkspaceActionsProps } from './HostWorkspaceList';
import type { WorkspaceOptionsProps } from './WorkspaceOptions';
import type { PairingScreenProps, PairingCodeSheetProps } from './PairingComponents';
import type { ComponentType, ReactNode } from 'react';
import type { ComposerOptionsProps, SessionTabsSheetProps, SessionHeaderProps, ChatMessageTextProps, ChatMessageMetaProps, ChatToolActivityProps, ChatImageProps, ChatImageViewerProps, ComposerProps, ComposerAddButtonProps, ComposerSuggestionListProps, ComposerSendButtonProps, MessageBubbleProps } from './ChatComponents';

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

/** The five tabs of the Orca home, in the order its bar shows them. */
export type HomeTabId = 'feed' | 'bots' | 'agents' | 'usage' | 'automations';

/**
 * The glyphs the home draws.
 *
 * The design system owns the ink and the drawing, so a host names the glyph it
 * wants instead of shipping one.
 */
export type HomeIconKind =
  | 'bots'
  | 'settings'
  | 'add'
  | 'search'
  | 'options'
  | 'back'
  | 'agents'
  | 'usage'
  | 'automations'
  | 'appearance'
  | 'terminal'
  | 'chatUi'
  | 'browser'
  | 'voice'
  | 'notifications'
  | 'troubleshooting'
  | 'about'
  | 'privacyPolicy'
  | 'support';

export interface HomeTabDescriptor {
  id: HomeTabId;
  /** The label under the glyph. */
  label: string;
  /** A count badge over the glyph; the bar draws it only above zero. */
  badge?: number;
}

export interface HomeTabBarProps {
  /** The tabs the bar shows, in the order it shows them. */
  tabs: readonly HomeTabDescriptor[];
  /** The tab the bar marks as selected. */
  active: HomeTabId;
  /** The device's bottom safe-area inset; the bar keeps a minimum padding under its row. */
  bottomInset: number;
  onSelect: (tab: HomeTabId) => void;
}

/** An icon action in a tab's header. */
export interface HomeTabHeaderAction {
  id: string;
  accessibilityLabel: string;
  /** The glyph the action draws. */
  icon: HomeIconKind;
  onPress: () => void;
  /** The surface the action opens is showing. */
  active?: boolean;
  disabled?: boolean;
}

export interface HomeTabHeaderProps {
  title: string;
  /** The header's icon actions; the original allows at most two. */
  actions?: readonly HomeTabHeaderAction[];
}

export interface HomeEmptyStateProps {
  /** The glyph that names the empty area. */
  icon: HomeIconKind;
  title: string;
  message?: string;
  /** The glyph's size; the original's empty states differ. */
  iconSize?: number;
  /** The glyph's stroke width; the original's agents robot draws lighter. */
  iconStrokeWidth?: number;
  /** The title's size; the original's empty states differ. */
  titleSize?: number;
  /** The body's size; the original's empty states differ. */
  bodySize?: number;
  /** The space between the glyph and the title. */
  iconGap?: number;
  /** The space between the title and the body. */
  titleGap?: number;
}

export interface HomeAgentSectionProps {
  title: string;
  rows: {id: string; title: string; place: string; age: string; unread: boolean; status: 'working' | 'waiting' | 'blocked' | 'done' | 'failed' | 'interrupted'}[];
  onSelect: (id: string) => void;
}

export interface HomeSearchFieldProps {
  value: string;
  onChangeText: (value: string) => void;
  placeholder: string;
  onClear: () => void;
}

/** A row of the home's options sheet. */
export interface HomeActionSheetAction {
  id: string;
  label: string;
  /** The row marks the choice it stands for. */
  checked?: boolean;
  disabled?: boolean;
  onPress: () => void;
}

export interface HomeActionSheetProps {
  visible: boolean;
  /** The sheet's rows, in the groups the original draws them in. */
  groups: readonly (readonly HomeActionSheetAction[])[];
  onClose: () => void;
}

export interface HomeSettingsRowProps {
  /** The glyph the row leads with; a row without one carries no icon slot. */
  icon?: HomeIconKind;
  label: string;
  /** The current value in grey, right of the label. */
  value?: string;
  /** The action the row offers on the right, e.g. "Update". */
  accessory?: string;
  /** The row opens another screen: it draws a chevron. */
  navigates?: boolean;
  /** The row is the picked option: it draws a check. */
  selected?: boolean;
  /** The row carries a switch; picking the row does not change it. */
  toggle?: {
    value: boolean;
    onValueChange: (value: boolean) => void;
    accessibilityLabel?: string;
  };
  disabled?: boolean;
  /** The row's accessible name; it defaults to the label, or the label and the value. */
  accessibilityLabel?: string;
  onPress?: () => void;
}

/**
 * Where a group's hairlines start, in points from the group's left edge.
 *
 * A group whose rows lead with an icon lines its hairlines up with the row
 * text: the row padding, the icon slot and the gap. A group without icons
 * starts them at the row padding.
 */
export const HOME_SETTINGS_SEPARATOR_INSET = 52;
export const HOME_SETTINGS_SEPARATOR_INSET_PLAIN = 16;

export interface HomeSettingsGroupProps {
  /** Where the group's hairlines start; it defaults to the icon group's inset. */
  separatorInset?: number;
  children: ReactNode;
}

/** A settings screen: the bar with its back button and title, then its rows. */
export interface HomeSettingsScreenProps {
  title: string;
  onBack: () => void;
  children: ReactNode;
}

/** Grey text under a group: what the group means, or why it is empty. */
export interface HomeSettingsNoteProps {
  children: ReactNode;
}

export interface PairedDesktopsProps {
  desktop?: { name: string; summary: string; offline?: boolean };
  onPair: () => void;
  onOpen: () => void;
}

export interface AgentComponents {
  PairedDesktops: ComponentType<PairedDesktopsProps>;
  SourceControl: ComponentType<SourceControlProps>;
  WorkspaceActions: ComponentType<WorkspaceActionsProps>;
  HostWorkspaceList: ComponentType<HostWorkspaceListProps>;
  WorkspaceOptions: ComponentType<WorkspaceOptionsProps>;
  PairingScreen: ComponentType<PairingScreenProps>;
  PairingCodeSheet: ComponentType<PairingCodeSheetProps>;
  SessionTabsSheet: ComponentType<SessionTabsSheetProps>;
  SessionHeader: ComponentType<SessionHeaderProps>;
  ChatMessageText: ComponentType<ChatMessageTextProps>;
  ChatMessageMeta: ComponentType<ChatMessageMetaProps>;
  ChatToolActivity: ComponentType<ChatToolActivityProps>;
  ChatSurface: ComponentType<{ children: ReactNode }>;
  ChatImage: ComponentType<ChatImageProps>;
  ChatImageViewer: ComponentType<ChatImageViewerProps>;
  MessageBubble: ComponentType<MessageBubbleProps>;
  Composer: ComponentType<ComposerProps>;
  ComposerAddButton: ComponentType<ComposerAddButtonProps>;
  ComposerSuggestionList: ComponentType<ComposerSuggestionListProps>;
  ComposerOptions: ComponentType<ComposerOptionsProps>;
  ComposerSendButton: ComponentType<ComposerSendButtonProps>;
  HomeSettingsGroup: ComponentType<HomeSettingsGroupProps>;
  HomeSettingsScreen: ComponentType<HomeSettingsScreenProps>;
  HomeSettingsNote: ComponentType<HomeSettingsNoteProps>;
  HomeSurface: ComponentType<{ children: ReactNode }>;
  HomeTabBar: ComponentType<HomeTabBarProps>;
  HomeTabHeader: ComponentType<HomeTabHeaderProps>;
  HomeEmptyState: ComponentType<HomeEmptyStateProps>;
  HomeAgentSection: ComponentType<HomeAgentSectionProps>;
  HomeSearchField: ComponentType<HomeSearchFieldProps>;
  HomeActionSheet: ComponentType<HomeActionSheetProps>;
  HomeSettingsRow: ComponentType<HomeSettingsRowProps>;
  BrandMark: ComponentType<{ size?: number }>;
  PairDesktopButton: ComponentType<PairDesktopButtonProps>;
  UnpairedHome: ComponentType<UnpairedHomeProps>;
  OnboardingChoice: ComponentType<OnboardingChoiceProps>;
  NotificationChoices: ComponentType<NotificationChoicesProps>;
}
