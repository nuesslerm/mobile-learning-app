import type { MenuActionNode } from '@lib/tamagui-design-system';
import { describe, expect, it, vi } from 'vitest';

import { editorSettingsMenu } from '../../editor-settings/editorSettingsMenu';
import { DEFAULT_EDITOR_PREFERENCES } from '../../../domain/editor-settings/editorPreferences';
import { editorLanguageMenu } from '../editorLanguageMenu';
import { editorHeaderSpec, type EditorHeaderInput } from '../editorHeaderSpec';

function inputs(patch: Partial<EditorHeaderInput> = {}): EditorHeaderInput {
  return {
    title: 'Two Sum',
    subtitle: 'Arrays · Easy',
    historyCount: 0,
    isLoggedIn: false,
    username: null,
    busy: false,
    settings: settingsMenu(),
    onBack: vi.fn(),
    onInfo: vi.fn(),
    onHistory: vi.fn(),
    onLogin: vi.fn(),
    ...patch,
  };
}

function settingsMenu() {
  return editorSettingsMenu(
    DEFAULT_EDITOR_PREFERENCES,
    { onFontSize: vi.fn(), onTabSize: vi.fn(), onWhitespace: vi.fn(), onWrap: vi.fn(), onSymbolBar: vi.fn(), onFormat: vi.fn() },
    'Python'
  );
}

describe('the editor header spec', () => {
  it('given the screen inputs, when built, then the groups are the settings piece alone and then Info with More, and More holds Version history and Login but no Format', () => {
    const input = inputs({ historyCount: 2, busy: true, settings: settingsMenu() });

    const spec = editorHeaderSpec(input);

    expect(spec).toMatchObject({ testID: 'ProblemEditorHeader', title: 'Two Sum', subtitle: 'Arrays · Easy', backAccessibilityLabel: 'Back to the problem list', busy: true });
    expect(spec.actions).toBeUndefined();
    expect(spec.groups?.map((group) => group.map((member) => member.testID))).toEqual([['EditorSettingsButton'], ['ProblemEditorInfoButton', 'ProblemEditorMenuButton']]);
    const more = spec.groups?.[1]?.[1];
    const rows = more && 'items' in more ? more.items.map((row) => (row.kind === undefined || row.kind === 'action' ? [row.testID, row.label, row.detail] : [])) : [];
    expect(rows).toEqual([
      ['ProblemEditorMenuHistory', 'Version history', '2 saved'],
      ['ProblemEditorMenuLogin', 'Login', 'LeetCode'],
    ]);
    spec.onBack?.();
    expect(input.onBack).toHaveBeenCalledTimes(1);
  });

  it('given a signed-in person, when built, then the login row is Log out with the username', () => {
    const spec = editorHeaderSpec(inputs({ isLoggedIn: true, username: 'flow-user', settings: settingsMenu() }));

    const more = spec.groups?.[1]?.[1];
    const login = more && 'items' in more ? more.items.find((row) => row.testID === 'ProblemEditorMenuLogin') : undefined;

    expect(login).toMatchObject({ label: 'Log out', detail: 'flow-user' });
  });

  it('given the native workspace has edits, when the header is built, then Undo and the language stay visible and problem actions move under the save status', () => {
    const onUndo = vi.fn();
    const input = inputs({
      onUndo,
      nativeWorkspace: { dirty: true, backgroundColor: '#000000', language: { ...settingsMenu(), testID: 'ProblemEditorLanguageMenu' } },
    });

    const spec = editorHeaderSpec(input);

    expect(spec.nativeTitle).toBe('hidden');
    expect(spec.leadingStatus).toMatchObject({ label: 'Unsaved', menu: { accessibilityLabel: 'Problem actions' } });
    expect(spec.leadingStatus?.menu?.items.map((item) => item.kind === 'section' ? item.title : item.label)).toEqual(['Problem description', 'Version history', 'Login']);
    expect(spec.groups?.map((group) => group.map((member) => member.testID))).toEqual([['ProblemEditorUndo'], ['ProblemEditorLanguageMenu', 'EditorSettingsButton']]);
    const undo = spec.groups?.[0]?.[0];
    if (undo && 'onPress' in undo) undo.onPress();
    expect(onUndo).toHaveBeenCalledOnce();
  });

  it('given no run in flight, when built, then busy is false', () => {
    expect(editorHeaderSpec(inputs({ settings: settingsMenu() })).busy).toBe(false);
  });
});

describe('the editor language menu', () => {
  it('given the available runners, when the native menu opens, then it preserves the recorded order and changes only to enabled languages', () => {
    const onChange = vi.fn();
    const menu = editorLanguageMenu('python3', ['python3', 'javascript'], onChange);
    const rows = menu.items.filter((row): row is MenuActionNode => row.kind === undefined || row.kind === 'action');

    expect(menu.label).toBe('Python3');
    expect(rows.map((row) => row.label)).toEqual([
      'C++', 'Java', 'Python3', 'Python', 'JavaScript', 'TypeScript', 'C#', 'C', 'Go', 'Kotlin', 'Swift', 'Rust',
    ]);
    expect(rows.filter((row) => !row.disabled).map((row) => row.label)).toEqual(['Python3', 'JavaScript']);
    expect(rows.filter((row) => row.checked).map((row) => row.label)).toEqual(['Python3']);
    for (const row of rows) row.onPress();
    expect(onChange).toHaveBeenCalledExactlyOnceWith('javascript');
  });
});
