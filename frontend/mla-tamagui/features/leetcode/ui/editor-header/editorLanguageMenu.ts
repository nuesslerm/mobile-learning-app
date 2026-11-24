import type { HeaderMenuSpec } from '@lib/tamagui-design-system';
import { Code } from '@lib/tamagui-design-system/icons';

import { languageLabel, type EditorLanguage } from '../LanguagePicker';

const languages: readonly { label: string; language?: EditorLanguage }[] = [
  { label: 'C++' },
  { label: 'Java' },
  { label: 'Python3', language: 'python3' },
  { label: 'Python' },
  { label: 'JavaScript', language: 'javascript' },
  { label: 'TypeScript' },
  { label: 'C#' },
  { label: 'C' },
  { label: 'Go' },
  { label: 'Kotlin' },
  { label: 'Swift' },
  { label: 'Rust' },
];

export function editorLanguageMenu(
  selected: EditorLanguage,
  available: readonly EditorLanguage[],
  onChange: (language: EditorLanguage) => void
): HeaderMenuSpec {
  return {
    testID: 'ProblemEditorLanguageMenu',
    accessibilityLabel: 'Language',
    icon: Code,
    label: languages.find(({ language }) => language === selected)?.label,
    items: languages.map(({ label, language }) => {
      const enabled = language !== undefined && available.includes(language);
      return {
        testID: language
          ? `ProblemEditorLanguagePicker-${languageLabel(language)}`
          : `ProblemEditorLanguageUnavailable-${label}`,
        label,
        checked: language === selected,
        disabled: !enabled,
        onPress: () => {
          if (enabled && language !== selected) onChange(language);
        },
      };
    }),
  };
}
