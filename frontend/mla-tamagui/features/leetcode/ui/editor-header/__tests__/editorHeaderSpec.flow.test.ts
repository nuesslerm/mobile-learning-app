import { describe, expect, it, vi } from 'vitest';

import { editorSettingsMenu } from '../../editor-settings/editorSettingsMenu';
import { DEFAULT_EDITOR_PREFERENCES } from '../../../domain/editor-settings/editorPreferences';
import { editorHeaderSpec, type EditorHeaderInput } from '../editorHeaderSpec';

function inputs(patch: Partial<EditorHeaderInput> = {}): EditorHeaderInput {
  return {
    title: 'Two Sum',
    subtitle: 'Arrays · Easy',
    historyCount: 0,
    isLoggedIn: false,
    username: null,
    busy: false,
    glass: false,
    onBack: vi.fn(),
    onInfo: vi.fn(),
    onFormat: vi.fn(),
    onHistory: vi.fn(),
    onLogin: vi.fn(),
    ...patch,
  };
}

const settings = () =>
  editorSettingsMenu(
    DEFAULT_EDITOR_PREFERENCES,
    { onFontSize: vi.fn(), onTabSize: vi.fn(), onWhitespace: vi.fn(), onWrap: vi.fn(), onSymbolBar: vi.fn(), onFormat: vi.fn() },
    'Python'
  );

describe('the editor header spec', () => {
  it('given the classic layout, when built, then it is the one pill of Info and a More menu with Format, Version history and Login, each handler the screen\'s', () => {
    const input = inputs({ historyCount: 2 });

    const spec = editorHeaderSpec(input);

    expect(spec).toMatchObject({ testID: 'ProblemEditorHeader', title: 'Two Sum', subtitle: 'Arrays · Easy', backAccessibilityLabel: 'Back to the problem list' });
    expect(spec.groups).toBeUndefined();
    expect(spec.busy).toBeUndefined();
    expect(spec.actions?.map((action) => action.testID)).toEqual(['ProblemEditorInfoButton']);
    expect(spec.menu?.testID).toBe('ProblemEditorMenuButton');
    expect(spec.menu?.accessibilityLabel).toBe('More actions');
    const rows = (spec.menu?.items ?? []).map((row) => (row.kind === undefined || row.kind === 'action' ? [row.testID, row.label, row.detail] : []));
    expect(rows).toEqual([
      ['ProblemEditorMenuFormat', 'Format', undefined],
      ['ProblemEditorMenuHistory', 'Version history', '2 saved'],
      ['ProblemEditorMenuLogin', 'Login', 'LeetCode'],
    ]);
    spec.onBack?.();
    spec.actions?.[0]?.onPress();
    expect(input.onBack).toHaveBeenCalledTimes(1);
    expect(input.onInfo).toHaveBeenCalledTimes(1);
  });

  it('given a signed-in person, when built, then the login row is Log out with the username', () => {
    const spec = editorHeaderSpec(inputs({ isLoggedIn: true, username: 'flow-user' }));

    const login = spec.menu?.items.find((row) => row.testID === 'ProblemEditorMenuLogin');

    expect(login).toMatchObject({ label: 'Log out', detail: 'flow-user' });
  });

  it('given the glass layout, when built, then busy rides along and the groups are the settings piece alone and then Info with More, and More has no Format row', () => {
    const spec = editorHeaderSpec(inputs({ glass: true, busy: true, settings: settings() }));

    expect(spec.busy).toBe(true);
    expect(spec.subtitle).toBe('Arrays · Easy');
    expect(spec.actions).toBeUndefined();
    expect(spec.groups?.map((group) => group.map((member) => member.testID))).toEqual([['EditorSettingsButton'], ['ProblemEditorInfoButton', 'ProblemEditorMenuButton']]);
    const more = spec.groups?.[1]?.[1];
    expect(more && 'items' in more ? more.items.map((row) => row.testID) : []).toEqual(['ProblemEditorMenuHistory', 'ProblemEditorMenuLogin']);
  });

  it('given the glass layout and no run in flight, when built, then busy is false', () => {
    expect(editorHeaderSpec(inputs({ glass: true, settings: settings() })).busy).toBe(false);
  });
});
