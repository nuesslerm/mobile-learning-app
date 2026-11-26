// features/coding-agent/src/domain/settingsCapabilities.ts
//
// The settings the original offers that this app does not back, yet.
//
// Each entry names what the original's screen offers and why it is open here.
// They are listed rather than faked: a row that pretends to work is worse than
// a row that says it cannot.

export interface SettingsCapability {
  /** What the original's screen offers. */
  label: string;
  /** Why this app cannot offer it. */
  reason: string;
}

export const TERMINAL_CAPABILITIES: readonly SettingsCapability[] = [
  {
    label: 'Restore the terminal when you leave the app',
    reason: 'This app runs no terminal on the phone.',
  },
  {
    label: 'Terminal text size',
    reason: 'This app runs no terminal on the phone.',
  },
  {
    label: 'Terminal shortcuts',
    reason: 'This app runs no terminal on the phone.',
  },
];

export const BROWSER_CAPABILITIES: readonly SettingsCapability[] = [
  {
    label: 'Choose where links in terminal output open',
    reason: 'This app shows no terminal output, so it has no such links.',
  },
];

export const VOICE_CAPABILITIES: readonly SettingsCapability[] = [
  {
    label: 'Enable voice dictation',
    reason: 'Dictation records on a paired desktop, which this app does not drive.',
  },
  {
    label: 'Dictation mode',
    reason: 'Dictation records on a paired desktop, which this app does not drive.',
  },
  {
    label: 'Speech model',
    reason: 'Speech models live on the paired desktop.',
  },
];
