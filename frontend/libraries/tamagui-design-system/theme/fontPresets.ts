// Font presets.
export type FontWeightToken = 'regular' | 'semibold' | 'bold' | 'extrabold';

export type FontPresetConfig = {
  id: string;
  name: string;
  description: string;
  /** Weight token → loaded font-family name. Empty = OS system font. */
  faces: Partial<Record<FontWeightToken, string>>;
};

// The `fontFamily` values below must match a name loaded via `useFonts` in the
// app's root layout (Expo Font requires fonts to be pre-loaded before a
// `style={{ fontFamily }}` referencing them will render correctly).
export const FONT_PRESETS: FontPresetConfig[] = [
  {
    id: 'system',
    name: 'System',
    description: 'The OS default — clean and native-feeling',
    faces: {},
  },
  {
    id: 'nunito',
    name: 'Nunito',
    description: 'Rounded, friendly sans-serif',
    faces: {
      regular: 'Nunito-Regular',
      semibold: 'Nunito-SemiBold',
      bold: 'Nunito-Bold',
      extrabold: 'Nunito-ExtraBold',
    },
  },
  {
    id: 'inter',
    name: 'Inter',
    description: 'Modern, highly legible UI sans-serif',
    faces: {
      regular: 'Inter_400Regular',
      semibold: 'Inter_600SemiBold',
      bold: 'Inter_700Bold',
      extrabold: 'Inter_800ExtraBold',
    },
  },
  {
    id: 'poppins',
    name: 'Poppins',
    description: 'Geometric, friendly sans-serif',
    faces: {
      regular: 'Poppins_400Regular',
      semibold: 'Poppins_600SemiBold',
      bold: 'Poppins_700Bold',
      extrabold: 'Poppins_800ExtraBold',
    },
  },
];

export function getFontPresetById(id: string): FontPresetConfig | undefined {
  return FONT_PRESETS.find((p) => p.id === id);
}

export function getDefaultFontPreset(): FontPresetConfig {
  return FONT_PRESETS[0];
}
