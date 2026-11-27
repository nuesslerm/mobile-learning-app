// The theme the device uses, as the original's Appearance screen offers it.
//
// The choice is kept on the device and applied at once: the design system
// reads it, so the whole app follows the pick.

import {
  HOME_SETTINGS_SEPARATOR_INSET_PLAIN,
  useAgentComponents,
  useAppearancePreference,
} from '@lib/agent-ui-contracts';

import type {
  AppearancePreference,
  ComponentSetPreference,
} from '../../domain/ports';
import { useComponentSet } from '../../state/componentSet';
import { OrcaSettingsScreen } from './OrcaSettingsScreen';

const THEMES: readonly { value: AppearancePreference; label: string }[] = [
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

const DESIGNS: readonly { value: ComponentSetPreference; label: string }[] = [
  { value: 'tamagui', label: 'App design' },
  { value: 'orca', label: 'Orca design' },
];

export function AppearanceSettingsScreen() {
  const design = useComponentSet();
  const { HomeSettingsGroup, HomeSettingsRow, HomeSettingsNote } = useAgentComponents();
  const { preference, select } = useAppearancePreference();
  return (
    <OrcaSettingsScreen title="Appearance">
      <HomeSettingsNote>
        System follows this device&apos;s light or dark setting.
      </HomeSettingsNote>
      <HomeSettingsGroup separatorInset={HOME_SETTINGS_SEPARATOR_INSET_PLAIN}>
        {THEMES.map((theme) => (
          <HomeSettingsRow
            key={theme.value}
            label={theme.label}
            selected={preference === theme.value}
            onPress={() => select(theme.value)}
          />
        ))}
      </HomeSettingsGroup>
      <HomeSettingsNote>
        App design matches the rest of this app; Orca design mirrors the Orca app.
      </HomeSettingsNote>
      <HomeSettingsGroup separatorInset={HOME_SETTINGS_SEPARATOR_INSET_PLAIN}>
        {DESIGNS.map((option) => (
          <HomeSettingsRow
            key={option.value}
            label={option.label}
            selected={design.preference === option.value}
            onPress={() => design.select(option.value)}
          />
        ))}
      </HomeSettingsGroup>
    </OrcaSettingsScreen>
  );
}
