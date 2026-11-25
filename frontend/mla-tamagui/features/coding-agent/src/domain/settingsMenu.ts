// features/coding-agent/src/domain/settingsMenu.ts
//
// The Orca settings menu, as its own reference carries it: the rows of the
// home's Settings tab, their glyphs and the screen each one opens.
//
// Ported from `references/orca/mobile/src/settings/mobile-settings-menu-items.ts`
// and `references/orca/mobile/src/settings/settings-menu-screen.tsx` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).
// Pure data, no IO: the tab reads this table and pushes what it names.

import type { HomeIconKind } from '@lib/agent-ui-contracts';

/** The screens the settings menu opens, named as Orca names them. */
export type OrcaSettingsDestination =
  | '/coding-agent/appearance-settings'
  | '/coding-agent/terminal-settings'
  | '/coding-agent/native-chat-settings'
  | '/coding-agent/browser-settings'
  | '/coding-agent/voice-settings'
  | '/coding-agent/notifications'
  | '/coding-agent/troubleshoot'
  | '/coding-agent/about';

export interface OrcaSettingsMenuItem {
  id: string;
  label: string;
  icon: HomeIconKind;
  /** The screen the row opens. */
  destination: OrcaSettingsDestination;
}

/** A row that leaves the app: it opens a URL instead of a screen. */
export interface OrcaSettingsExternalItem {
  id: string;
  label: string;
  icon: HomeIconKind;
  url: string;
}

/** The menu's first group: the settings screens themselves. */
export const ORCA_SETTINGS_MENU: readonly OrcaSettingsMenuItem[] = [
  { id: 'appearance', label: 'Appearance', icon: 'appearance', destination: '/coding-agent/appearance-settings' },
  { id: 'terminal', label: 'Terminal', icon: 'terminal', destination: '/coding-agent/terminal-settings' },
  { id: 'nativeChat', label: 'Chat UI', icon: 'chatUi', destination: '/coding-agent/native-chat-settings' },
  { id: 'browser', label: 'Browser', icon: 'browser', destination: '/coding-agent/browser-settings' },
  { id: 'voice', label: 'Voice', icon: 'voice', destination: '/coding-agent/voice-settings' },
  { id: 'notifications', label: 'Notifications', icon: 'notifications', destination: '/coding-agent/notifications' },
  { id: 'troubleshooting', label: 'Troubleshooting', icon: 'troubleshooting', destination: '/coding-agent/troubleshoot' },
  { id: 'about', label: 'About', icon: 'about', destination: '/coding-agent/about' },
];

/**
 * The menu's last group: the links that leave the app.
 *
 * They are Orca's own destinations; an MLA release needs its own privacy
 * policy and support address.
 */
export const ORCA_SETTINGS_EXTERNAL: readonly OrcaSettingsExternalItem[] = [
  { id: 'privacyPolicy', label: 'Privacy Policy', icon: 'privacyPolicy', url: 'https://orca.dev/privacy' },
  { id: 'support', label: 'Support', icon: 'support', url: 'https://github.com/stablyai/orca/issues' },
];
