export type NavigationThemes<T> = { light: T; dark: T };

/** The theme the navigator's chrome gets: react-navigation's light one, which its container uses when none is given, unless the native bar is in play, then the app's scheme. */
export function navigationThemeFor<T>(
  active: boolean,
  scheme: 'light' | 'dark',
  themes: NavigationThemes<T>
): T {
  return active && scheme === 'dark' ? themes.dark : themes.light;
}

/** What the OS chrome is told to follow: the person's Light or Dark choice, and `auto` (the system's own setting) for System; React Native 0.88 has no null for it. */
export function chromeSchemeFor(
  mode: 'system' | 'light' | 'dark'
): 'light' | 'dark' | 'auto' {
  return mode === 'system' ? 'auto' : mode;
}
