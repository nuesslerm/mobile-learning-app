import { describe, expect, it } from 'vitest';

import { toConfigPatch, toEditorConfig, toLegacyPatch, type LegacyEditorConfig } from '../configPatch';

const ALL = { wordWrap: true, isDark: true, cursorColor: '#a', selectionColor: '#b', backgroundColor: '#c', gutterColor: '#d', borderColor: '#e', fontSize: 15, lineHeight: 22, tabSize: 2 };

describe('the config patch mapping', () => {
  it('given the ten keys EditorInput sends, when mapped to the typed API and back, then nothing is lost or invented', () => {
    // Given an old patch with every mappable key
    // When it goes to the typed form and back
    const typed = toConfigPatch(ALL);

    // Then the typed form is nested the way the contract says and the round trip is exact
    expect(typed).toEqual({ wrap: true, tabSize: 2, fontSize: 15, lineHeight: 22, palette: { dark: true, cursor: '#a', selection: '#b', background: '#c', gutter: '#d', border: '#e' } });
    expect(toLegacyPatch(typed ?? {})).toEqual(ALL);
  });

  it('given a partial patch, when mapped, then only the named keys appear', () => {
    expect(toConfigPatch({ fontSize: 12 })).toEqual({ fontSize: 12 });
    expect(toConfigPatch({ isDark: false })).toEqual({ palette: { dark: false } });
    expect(toLegacyPatch({ palette: { gutter: '#1' } })).toEqual({ gutterColor: '#1' });
  });

  it.each([['initialText', { initialText: 'x' }], ['language', { language: 'cpp' }], ['an unknown key', { noAutoFocus: true } as never]])('given a patch holding %s, when mapped, then it stays on the old path', (_name, patch) => {
    expect(toConfigPatch(patch)).toBeNull();
  });

  it('given the page config, when shown to the typed API, then the language is python or cpp and the palette is nested', () => {
    const legacy: LegacyEditorConfig = { initialText: '', language: 'cpp', ...ALL };
    expect(toEditorConfig(legacy)).toEqual({ language: 'cpp', wrap: true, tabSize: 2, fontSize: 15, lineHeight: 22, palette: { dark: true, cursor: '#a', selection: '#b', background: '#c', gutter: '#d', border: '#e' } });
  });
});
