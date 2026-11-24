import type {
  HeaderMenuSpec,
  MenuActionNode,
  MenuNode,
  SfSymbolName,
} from '@lib/tamagui-design-system';
import {
  CaseSensitive,
  IndentIncrease,
  Keyboard,
  SlidersHorizontal,
  Space,
  Wand2,
  WrapText,
} from '@lib/tamagui-design-system/icons';

import {
  isFontSize,
  isTabSize,
  type EditorPreferences,
  type FontSize,
  type TabSize,
} from '../../domain/editor-settings/editorPreferences';
import {
  settingsMenuRows,
  type SettingsRow,
} from '../../domain/editor-settings/settingsMenu';

export const EDITOR_SETTINGS_IDS = {
  button: 'EditorSettingsButton',
  viewSection: 'EditorSettingsViewSection',
  toolsSection: 'EditorSettingsToolsSection',
  fontSize: 'EditorSettingsFontSize',
  whitespace: 'EditorSettingsWhitespace',
  wrap: 'EditorSettingsWrap',
  tabSize: 'EditorSettingsTabs',
  symbolBar: 'EditorSettingsSymbolBar',
  format: 'EditorSettingsFormat',
} as const;

const SECTION_IDS = {
  view: EDITOR_SETTINGS_IDS.viewSection,
  tools: EDITOR_SETTINGS_IDS.toolsSection,
} as const;

const GLYPHS = {
  button: { icon: SlidersHorizontal, sfSymbol: 'slider.horizontal.3' },
  fontSize: { icon: CaseSensitive, sfSymbol: 'textformat.size' },
  whitespace: { icon: Space, sfSymbol: 'space' },
  wrap: { icon: WrapText, sfSymbol: 'decrease.indent' },
  tabSize: { icon: IndentIncrease, sfSymbol: 'text.alignright' },
  symbolBar: { icon: Keyboard, sfSymbol: 'keyboard' },
  format: { icon: Wand2, sfSymbol: 'wand.and.sparkles' },
} as const satisfies Record<
  Exclude<keyof typeof EDITOR_SETTINGS_IDS, 'viewSection' | 'toolsSection'>,
  { icon: unknown; sfSymbol: SfSymbolName }
>;

export type EditorSettingsHandlers = {
  onFontSize: (size: FontSize) => void;
  onTabSize: (size: TabSize) => void;
  onWhitespace: (on: boolean) => void;
  onWrap: (on: boolean) => void;
  onSymbolBar: () => void;
  onFormat: () => void;
};

type SubmenuRow = Extract<SettingsRow, { kind: 'submenu' }>;

function optionNodes<T extends number>(
  row: SubmenuRow,
  guard: (value: unknown) => value is T,
  pick: (value: T) => void
): MenuActionNode[] {
  return row.options.flatMap((option) => {
    const value = option.value;
    if (!guard(value)) return [];
    return [
      {
        testID: `${EDITOR_SETTINGS_IDS[row.id]}-${value}`,
        label: option.label,
        checked: option.checked,
        onPress: () => pick(value),
      },
    ];
  });
}

function toNode(row: SettingsRow, handlers: EditorSettingsHandlers): MenuNode {
  switch (row.kind) {
    case 'submenu':
      return {
        kind: 'submenu',
        testID: EDITOR_SETTINGS_IDS[row.id],
        label: row.label,
        ...GLYPHS[row.id],
        items:
          row.id === 'fontSize'
            ? optionNodes(row, isFontSize, handlers.onFontSize)
            : optionNodes(row, isTabSize, handlers.onTabSize),
      };
    case 'toggle':
      return {
        testID: EDITOR_SETTINGS_IDS[row.id],
        label: row.label,
        ...GLYPHS[row.id],
        checked: row.checked,
        onPress: () =>
          (row.id === 'wrap' ? handlers.onWrap : handlers.onWhitespace)(
            !row.checked
          ),
      };
    case 'action':
      return {
        testID: EDITOR_SETTINGS_IDS.symbolBar,
        label: row.label,
        ...GLYPHS.symbolBar,
        onPress: handlers.onSymbolBar,
      };
    case 'confirm':
      return {
        kind: 'confirm',
        testID: EDITOR_SETTINGS_IDS.format,
        label: row.label,
        title: row.title,
        message: row.message,
        confirmLabel: row.confirmLabel,
        ...GLYPHS.format,
        onConfirm: handlers.onFormat,
      };
  }
}

/** The recording's settings menu for these preferences as the design system's menu tree; `label` is the language shown beside the button's glyph. */
export function editorSettingsMenu(
  preferences: EditorPreferences,
  handlers: EditorSettingsHandlers,
  label?: string
): HeaderMenuSpec {
  return {
    testID: EDITOR_SETTINGS_IDS.button,
    accessibilityLabel: 'Editor settings',
    label,
    ...GLYPHS.button,
    items: settingsMenuRows(preferences).map((section) => ({
      kind: 'section',
      testID: SECTION_IDS[section.id],
      items: section.rows.map((row) => toNode(row, handlers)),
    })),
  };
}
