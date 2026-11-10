// Preset theme metadata.
// Only the selection-facing metadata (id/name/description) survives the port:
// the actual color values live in the Tamagui themes built by ./themes.ts, so
// there is no ColorThemeConfig, no RawThemeColors, and no CSS-variable plumbing.
export type PresetTheme = {
  id: string;
  name: string;
  description: string;
};

// `PRESET_THEMES[0]` is the default theme (see `getDefaultTheme` below) — the
// iOS Native preset leads the array so the Apple-style shell is the
// out-of-the-box look. Every other preset remains fully selectable.
export const PRESET_THEMES: PresetTheme[] = [
  {
    id: 'ios-native',
    name: 'iOS Native',
    description: 'Near-black surfaces and system tints, styled after iOS 26',
  },
  {
    id: 'monochrome-pro',
    name: 'Monochrome Pro',
    description: 'Professional charcoal, slate, and blue-gray',
  },
  {
    id: 'classic-duolingo',
    name: 'Classic Duolingo',
    description: 'The original vibrant green, orange, and blue',
  },
  {
    id: 'ocean-breeze',
    name: 'Ocean Breeze',
    description: 'Deep blue, teal, and cyan tones',
  },
  {
    id: 'forest-fresh',
    name: 'Forest Fresh',
    description: 'Natural green, lime, and mint palette',
  },
  {
    id: 'desert-dawn',
    name: 'Desert Dawn',
    description: 'Terracotta earth, warm sand, and cool turquoise',
  },
  {
    id: 'pastel-pop',
    name: 'Pastel Pop',
    description: 'Playful bubblegum pink, lilac, and mint',
  },
  {
    id: 'midnight-neon',
    name: 'Midnight Neon',
    description: 'Electric purple, teal, and lime on dark surfaces',
  },
  {
    id: 'royal-purple',
    name: 'Royal Purple',
    description: 'Rich purple, pink, and violet tones',
  },
  {
    id: 'chess-wood',
    name: 'Chess Wood',
    description: 'Classic wooden board with warm earth tones',
  },
  {
    id: 'sunset-vibes',
    name: 'Sunset Vibes',
    description: 'Warm coral, orange, and purple hues',
  },
  {
    id: 'leetcode-slate',
    name: 'LeetCode Slate',
    description: 'Charcoal surfaces with amber primary and cyan accents',
  },
];

export type PresetThemeCategory = {
  label: string;
  ids: string[];
};

/** Display groupings for the preset grid in SettingsScreen. */
export const PRESET_THEME_CATEGORIES: PresetThemeCategory[] = [
  {
    label: 'Default',
    ids: ['ios-native', 'monochrome-pro', 'classic-duolingo'],
  },
  {
    label: 'Light Palettes',
    ids: ['ocean-breeze', 'forest-fresh', 'desert-dawn', 'pastel-pop'],
  },
  {
    label: 'Dark & Dramatic',
    ids: ['midnight-neon', 'royal-purple', 'chess-wood'],
  },
  {
    label: 'Warm',
    ids: ['sunset-vibes', 'leetcode-slate'],
  },
];

export function getPresetThemeById(id: string): PresetTheme | undefined {
  return PRESET_THEMES.find((t) => t.id === id);
}

export function getDefaultTheme(): PresetTheme {
  return PRESET_THEMES[0];
}
