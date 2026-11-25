// The theme the device uses, as the original's Appearance screen offers it.
//
// The choice is kept on the device and applied at once: the design system
// reads it, so the whole app follows the pick.

import {
  HOME_SETTINGS_SEPARATOR_INSET_PLAIN,
  useAgentComponents,
} from '@lib/agent-ui-contracts';
import { useAppearancePreference } from '@lib/orca-design-system';

import type { AppearancePreference } from '../../domain/ports';
import { OrcaSettingsScreen } from './OrcaSettingsScreen';

const THEMES: readonly { value: AppearancePreference; label: string }[] = [
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

export function AppearanceSettingsScreen() {
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
    </OrcaSettingsScreen>
  );
}
