import { describe, expect, it } from 'vitest';

import {
  DEFAULT_EDITOR_PREFERENCES,
  FONT_SIZES,
  TAB_SIZES,
  fontSizeLabel,
  isFontSize,
  isTabSize,
  lineHeightFor,
  parseEditorPreferences,
} from '../editorPreferences';

describe('editor preferences', () => {
  it('Given the constants, When the defaults are read, Then they are today\'s editor: 14px, 4 spaces, wrap on, whitespace off', () => {
    expect(DEFAULT_EDITOR_PREFERENCES).toEqual({ fontSize: 14, tabSize: 4, wrap: true, whitespace: false });
    expect(FONT_SIZES).toEqual([12, 13, 14, 15, 16, 18, 20]);
    expect(TAB_SIZES).toEqual([2, 4, 8]);
  });

  it('Given each listed font size, When its line height is taken, Then it is the size times 1.5, rounded', () => {
    expect(FONT_SIZES.map(lineHeightFor)).toEqual([18, 20, 21, 23, 24, 27, 30]);
    expect(FONT_SIZES.map((size) => lineHeightFor(size))).toEqual(FONT_SIZES.map((size) => Math.round(size * 1.5)));
  });

  it('Given a size, When it is labelled, Then it reads like the menu row, 16px', () => {
    expect(fontSizeLabel(16)).toBe('16px');
  });

  it('Given a number outside the lists or not a number, When it is checked, Then it is refused', () => {
    expect(isFontSize(17)).toBe(false);
    expect(isFontSize('16')).toBe(false);
    expect(isFontSize(16)).toBe(true);
    expect(isTabSize(3)).toBe(false);
    expect(isTabSize(8)).toBe(true);
  });

  it('Given stored values that are all listed, When they are parsed, Then they hold', () => {
    expect(parseEditorPreferences({ fontSize: 16, tabSize: 2, wrap: false, whitespace: true })).toEqual({
      fontSize: 16,
      tabSize: 2,
      wrap: false,
      whitespace: true,
    });
  });

  it('Given 17px as the font size and a string as the tab size, When parsed, Then those two take their defaults and wrap and whitespace survive', () => {
    expect(parseEditorPreferences({ fontSize: 17, tabSize: '4', wrap: false, whitespace: true })).toEqual({
      fontSize: 14,
      tabSize: 4,
      wrap: false,
      whitespace: true,
    });
  });

  it('Given a value that is not an object, When parsed, Then every field is its default', () => {
    expect(parseEditorPreferences(undefined)).toEqual(DEFAULT_EDITOR_PREFERENCES);
    expect(parseEditorPreferences('16px')).toEqual(DEFAULT_EDITOR_PREFERENCES);
    expect(parseEditorPreferences(null)).toEqual(DEFAULT_EDITOR_PREFERENCES);
  });
});
