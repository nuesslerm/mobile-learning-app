import { FONT_SIZES, TAB_SIZES, fontSizeLabel, type EditorPreferences } from './editorPreferences';

export type SettingsOption = { value: number; label: string; checked: boolean };

export type SettingsRow =
  | { kind: 'submenu'; id: 'fontSize' | 'tabSize'; label: string; options: SettingsOption[] }
  | { kind: 'toggle'; id: 'whitespace' | 'wrap'; label: string; checked: boolean }
  | { kind: 'action'; id: 'symbolBar'; label: string }
  | { kind: 'confirm'; id: 'format'; label: string; title: string; message: string; confirmLabel: string };

export type SettingsSection = { id: 'view' | 'tools'; rows: SettingsRow[] };

/** The rows of LeetCode's editor-settings menu for these preferences: no icons and no handlers, the ui ring adds both. */
export function settingsMenuRows(preferences: EditorPreferences): SettingsSection[] {
  return [
    {
      id: 'view',
      rows: [
        {
          kind: 'submenu',
          id: 'fontSize',
          label: `Font size: ${fontSizeLabel(preferences.fontSize)}`,
          options: FONT_SIZES.map((size) => ({ value: size, label: fontSizeLabel(size), checked: size === preferences.fontSize })),
        },
        { kind: 'toggle', id: 'whitespace', label: 'Show Whitespace', checked: preferences.whitespace },
        { kind: 'toggle', id: 'wrap', label: 'Line wrap', checked: preferences.wrap },
      ],
    },
    {
      id: 'tools',
      rows: [
        {
          kind: 'submenu',
          id: 'tabSize',
          label: `Tabs: ${preferences.tabSize}`,
          options: TAB_SIZES.map((size) => ({ value: size, label: String(size), checked: size === preferences.tabSize })),
        },
        { kind: 'action', id: 'symbolBar', label: 'Symbol Bar' },
        { kind: 'confirm', id: 'format', label: 'Format Code', title: 'Format current code?', message: 'Your code will be reformatted.', confirmLabel: 'Format' },
      ],
    },
  ];
}
