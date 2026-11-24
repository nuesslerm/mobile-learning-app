import { describe, expect, it } from 'vitest';

import { chromeSchemeFor, navigationThemeFor } from '../nativeChromeTheme';

const themes = { light: 'DefaultTheme', dark: 'DarkTheme' };

describe('navigationThemeFor', () => {
  it('Given the native bar is not in play, When either scheme is asked for, Then the navigator keeps the light theme it has today', () => {
    // Given an inactive native path
    // When both schemes are asked for
    // Then both answer the light theme
    expect(navigationThemeFor(false, 'light', themes)).toBe('DefaultTheme');
    expect(navigationThemeFor(false, 'dark', themes)).toBe('DefaultTheme');
  });

  it('Given the native bar is in play, When each scheme is asked for, Then the app scheme picks the theme', () => {
    // Given an active native path
    // When each scheme is asked for
    // Then dark gets the dark theme and light the light one
    expect(navigationThemeFor(true, 'dark', themes)).toBe('DarkTheme');
    expect(navigationThemeFor(true, 'light', themes)).toBe('DefaultTheme');
  });
});

describe('chromeSchemeFor', () => {
  it('Given each theme mode, When the chrome scheme is read, Then System hands the choice back to the OS and the others force theirs', () => {
    // Given the three modes
    // When the scheme for each is read
    // Then System is auto
    expect(chromeSchemeFor('system')).toBe('auto');
    expect(chromeSchemeFor('light')).toBe('light');
    expect(chromeSchemeFor('dark')).toBe('dark');
  });
});
