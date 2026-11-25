import type {
  MenuActionNode,
  MenuConfirmNode,
  MenuNode,
  MenuSectionNode,
  MenuSubmenuNode,
} from '@lib/tamagui-design-system';
import { validateMenuTree } from '@lib/tamagui-design-system';
import { SlidersHorizontal } from '@lib/tamagui-design-system/icons';
import { describe, expect, it, vi } from 'vitest';

import {
  DEFAULT_EDITOR_PREFERENCES,
  type EditorPreferences,
} from '../../../domain/editor-settings/editorPreferences';
import {
  EDITOR_SETTINGS_IDS,
  editorSettingsMenu,
  type EditorSettingsHandlers,
} from '../editorSettingsMenu';

function handlers(): EditorSettingsHandlers {
  return {
    onFontSize: vi.fn(),
    onTabSize: vi.fn(),
    onWhitespace: vi.fn(),
    onWrap: vi.fn(),
    onSymbolBar: vi.fn(),
    onFormat: vi.fn(),
  };
}

function rootRows(
  preferences: EditorPreferences = DEFAULT_EDITOR_PREFERENCES,
  given: EditorSettingsHandlers = handlers()
): MenuNode[] {
  return editorSettingsMenu(preferences, given).items.flatMap((node) => {
    if (node.kind !== 'section') throw new Error(`root node ${node.testID} is not a section`);
    return node.items;
  });
}

function find(nodes: readonly MenuNode[], testID: string): MenuNode {
  const node = nodes.find((candidate) => candidate.testID === testID);
  if (!node) throw new Error(`no ${testID} node`);
  return node;
}

const action = (node: MenuNode) => node as MenuActionNode;
const submenu = (node: MenuNode) => node as MenuSubmenuNode;
const confirm = (node: MenuNode) => node as MenuConfirmNode;

function walk(nodes: readonly MenuNode[]): MenuNode[] {
  return nodes.flatMap((node) =>
    node.kind === 'section' || node.kind === 'submenu' ? [node, ...walk(node.items)] : [node]
  );
}

describe('editorSettingsMenu', () => {
  it('Given the defaults and a language, When the menu is built, Then the button is the sliders glyph named Editor settings and carries the language', () => {
    const menu = editorSettingsMenu(DEFAULT_EDITOR_PREFERENCES, handlers(), 'Python3');

    expect(menu).toMatchObject({
      testID: EDITOR_SETTINGS_IDS.button,
      accessibilityLabel: 'Editor settings',
      label: 'Python3',
      sfSymbol: 'slider.horizontal.3',
    });
    expect(menu.icon).toBe(SlidersHorizontal);
  });

  it('Given the defaults, When the menu is built, Then two sections hold the recording\'s six rows in order', () => {
    const sections = editorSettingsMenu(DEFAULT_EDITOR_PREFERENCES, handlers()).items as MenuSectionNode[];

    expect(sections.map((section) => section.kind)).toEqual(['section', 'section']);
    expect(sections.map((section) => section.items.map((row) => `${row.kind ?? 'action'}:${'label' in row ? row.label : ''}`))).toEqual([
      ['submenu:Font size: 14px', 'action:Show Whitespace', 'action:Line wrap'],
      ['submenu:Tabs: 4', 'action:Symbol Bar', 'confirm:Format Code'],
    ]);
  });

  it('Given the defaults, When the toggles are read, Then each is checked as its preference says and dismisses the menu', () => {
    const rows = rootRows();

    const whitespace = action(find(rows, EDITOR_SETTINGS_IDS.whitespace));
    const wrap = action(find(rows, EDITOR_SETTINGS_IDS.wrap));
    expect([whitespace.checked, wrap.checked]).toEqual([false, true]);
    expect(whitespace.keepOpen).toBeFalsy();
    expect(wrap.keepOpen).toBeFalsy();
  });

  it('Given the defaults, When the Font size and Tabs submenus are read, Then every option dismisses the menu and only the current one is checked', () => {
    const rows = rootRows();
    const font = submenu(find(rows, EDITOR_SETTINGS_IDS.fontSize));
    const tabs = submenu(find(rows, EDITOR_SETTINGS_IDS.tabSize));

    expect(font.items.map((option) => [option.testID, action(option).label, action(option).checked, action(option).keepOpen])).toEqual([
      ['EditorSettingsFontSize-12', '12px', false, undefined],
      ['EditorSettingsFontSize-13', '13px', false, undefined],
      ['EditorSettingsFontSize-14', '14px', true, undefined],
      ['EditorSettingsFontSize-15', '15px', false, undefined],
      ['EditorSettingsFontSize-16', '16px', false, undefined],
      ['EditorSettingsFontSize-18', '18px', false, undefined],
      ['EditorSettingsFontSize-20', '20px', false, undefined],
    ]);
    expect(tabs.items.map((option) => [option.testID, action(option).label, action(option).checked, action(option).keepOpen])).toEqual([
      ['EditorSettingsTabs-2', '2', false, undefined],
      ['EditorSettingsTabs-4', '4', true, undefined],
      ['EditorSettingsTabs-8', '8', false, undefined],
    ]);
  });

  it('Given the defaults, When Symbol Bar is read, Then it is a plain action with no check that closes the menu', () => {
    const symbolBar = action(find(rootRows(), EDITOR_SETTINGS_IDS.symbolBar));

    expect(symbolBar.kind ?? 'action').toBe('action');
    expect(symbolBar.checked).toBeUndefined();
    expect(symbolBar.keepOpen).toBeFalsy();
    expect(symbolBar.destructive).toBeFalsy();
  });

  it('Given the defaults, When Format Code is read, Then it is a confirm node with the recording\'s title, line and button', () => {
    const format = confirm(find(rootRows(), EDITOR_SETTINGS_IDS.format));

    expect(format).toMatchObject({
      kind: 'confirm',
      label: 'Format Code',
      title: 'Format current code?',
      message: 'Your code will be reformatted.',
      confirmLabel: 'Format',
    });
  });

  it('Given the menu, When every row of the root is read, Then each has an SF Symbol and a Lucide twin', () => {
    const rows = rootRows();

    expect(rows).toHaveLength(6);
    for (const row of rows) {
      const glyphs = row as MenuActionNode;
      expect(glyphs.sfSymbol, `${row.testID} sfSymbol`).toBeTruthy();
      expect(glyphs.icon, `${row.testID} icon`).toBeTruthy();
    }
    expect(rows.map((row) => (row as MenuActionNode).sfSymbol)).toEqual([
      'textformat.size',
      'space',
      'decrease.indent',
      'text.alignright',
      'keyboard',
      'wand.and.sparkles',
    ]);
  });

  it('Given 16px, tabs 8, wrap off and whitespace on, When the menu is built, Then the labels and checks follow', () => {
    const rows = rootRows({ fontSize: 16, tabSize: 8, wrap: false, whitespace: true });
    const font = submenu(find(rows, EDITOR_SETTINGS_IDS.fontSize));
    const tabs = submenu(find(rows, EDITOR_SETTINGS_IDS.tabSize));

    expect(font.label).toBe('Font size: 16px');
    expect(font.items.filter((option) => action(option).checked).map((option) => option.testID)).toEqual(['EditorSettingsFontSize-16']);
    expect(tabs.label).toBe('Tabs: 8');
    expect(tabs.items.filter((option) => action(option).checked).map((option) => option.testID)).toEqual(['EditorSettingsTabs-8']);
    expect(action(find(rows, EDITOR_SETTINGS_IDS.whitespace)).checked).toBe(true);
    expect(action(find(rows, EDITOR_SETTINGS_IDS.wrap)).checked).toBe(false);
  });

  it.each([
    ['the defaults', DEFAULT_EDITOR_PREFERENCES],
    ['16px, tabs 2, wrap off, whitespace on', { fontSize: 16, tabSize: 2, wrap: false, whitespace: true } satisfies EditorPreferences],
  ])('Given %s, When the tree is validated, Then every node id is unique', (_name, preferences) => {
    const menu = editorSettingsMenu(preferences, handlers());

    const ids = walk(menu.items).map((node) => node.testID);

    expect(validateMenuTree(menu.items)).toEqual({ valid: true, duplicateIds: [] });
    // two sections, six rows, seven font sizes and three tab sizes
    expect(ids).toHaveLength(18);
    expect(new Set(ids).size).toBe(18);
  });

  it('Given handlers, When each row is pressed, Then only its own handler runs, with the value the row stands for', () => {
    const given = handlers();
    const rows = rootRows(DEFAULT_EDITOR_PREFERENCES, given);

    action(find(rows, EDITOR_SETTINGS_IDS.whitespace)).onPress();
    expect(given.onWhitespace).toHaveBeenCalledExactlyOnceWith(true);
    action(find(rows, EDITOR_SETTINGS_IDS.wrap)).onPress();
    expect(given.onWrap).toHaveBeenCalledExactlyOnceWith(false);
    action(submenu(find(rows, EDITOR_SETTINGS_IDS.fontSize)).items[4]).onPress();
    expect(given.onFontSize).toHaveBeenCalledExactlyOnceWith(16);
    action(submenu(find(rows, EDITOR_SETTINGS_IDS.tabSize)).items[0]).onPress();
    expect(given.onTabSize).toHaveBeenCalledExactlyOnceWith(2);
    action(find(rows, EDITOR_SETTINGS_IDS.symbolBar)).onPress();
    expect(given.onSymbolBar).toHaveBeenCalledOnce();
    expect(given.onFormat).not.toHaveBeenCalled();
    confirm(find(rows, EDITOR_SETTINGS_IDS.format)).onConfirm();
    expect(given.onFormat).toHaveBeenCalledOnce();
  });
});
