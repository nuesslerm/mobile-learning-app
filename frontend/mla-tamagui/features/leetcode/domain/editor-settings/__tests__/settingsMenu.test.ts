import { describe, expect, it } from 'vitest';

import { DEFAULT_EDITOR_PREFERENCES } from '../editorPreferences';
import { settingsMenuRows, type SettingsRow } from '../settingsMenu';

const submenu = (rows: SettingsRow[], id: 'fontSize' | 'tabSize') => {
  const row = rows.find((candidate) => candidate.kind === 'submenu' && candidate.id === id);
  if (row?.kind !== 'submenu') throw new Error(`no ${id} row`);
  return row;
};

describe('settingsMenuRows', () => {
  it('Given the defaults, When the rows are built, Then there are two sections in the order of the recording', () => {
    const sections = settingsMenuRows(DEFAULT_EDITOR_PREFERENCES);

    expect(sections.map((section) => section.id)).toEqual(['view', 'tools']);
    expect(sections[0].rows.map((row) => `${row.kind}:${row.label}`)).toEqual([
      'submenu:Font size: 14px',
      'toggle:Show Whitespace',
      'toggle:Line wrap',
    ]);
    expect(sections[1].rows.map((row) => `${row.kind}:${row.label}`)).toEqual([
      'submenu:Tabs: 4',
      'action:Symbol Bar',
      'confirm:Format Code',
    ]);
  });

  it('Given the defaults, When the rows are built, Then the toggles show wrap on and whitespace off and 14px and 4 are the checked options', () => {
    const [view, tools] = settingsMenuRows(DEFAULT_EDITOR_PREFERENCES);

    expect(view.rows.filter((row) => row.kind === 'toggle').map((row) => (row.kind === 'toggle' ? [row.id, row.checked] : []))).toEqual([
      ['whitespace', false],
      ['wrap', true],
    ]);
    const font = submenu(view.rows, 'fontSize');
    expect(font.options.map((option) => option.label)).toEqual(['12px', '13px', '14px', '15px', '16px', '18px', '20px']);
    expect(font.options.filter((option) => option.checked).map((option) => option.value)).toEqual([14]);
    const tabs = submenu(tools.rows, 'tabSize');
    expect(tabs.options.map((option) => option.label)).toEqual(['2', '4', '8']);
    expect(tabs.options.filter((option) => option.checked).map((option) => option.value)).toEqual([4]);
  });

  it('Given the defaults, When the rows are built, Then Format Code is a confirmation with the recording\'s title, line and button', () => {
    const format = settingsMenuRows(DEFAULT_EDITOR_PREFERENCES)[1].rows.find((row) => row.kind === 'confirm');

    expect(format).toEqual({
      kind: 'confirm',
      id: 'format',
      label: 'Format Code',
      title: 'Format current code?',
      message: 'Your code will be reformatted.',
      confirmLabel: 'Format',
    });
  });

  it('Given 16px, tabs 8, wrap off and whitespace on, When the rows are built, Then the labels and the checks follow', () => {
    const [view, tools] = settingsMenuRows({ fontSize: 16, tabSize: 8, wrap: false, whitespace: true });

    const font = submenu(view.rows, 'fontSize');
    expect(font.label).toBe('Font size: 16px');
    expect(font.options.filter((option) => option.checked).map((option) => option.value)).toEqual([16]);
    expect(view.rows.filter((row) => row.kind === 'toggle').map((row) => (row.kind === 'toggle' ? [row.id, row.checked] : []))).toEqual([
      ['whitespace', true],
      ['wrap', false],
    ]);
    const tabs = submenu(tools.rows, 'tabSize');
    expect(tabs.label).toBe('Tabs: 8');
    expect(tabs.options.filter((option) => option.checked).map((option) => option.value)).toEqual([8]);
  });
});
