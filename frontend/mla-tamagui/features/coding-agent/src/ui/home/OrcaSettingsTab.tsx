import {
  HOME_SETTINGS_SEPARATOR_INSET,
  useSkin,
} from '@lib/skin';
import { router } from 'expo-router';
import { OrcaSettingsScreen } from '../settings/OrcaSettingsScreen';

import {
  ORCA_SETTINGS_EXTERNAL,
  ORCA_SETTINGS_MENU,
} from '../../domain/settingsMenu';
import { useCodingAgentPorts } from '../../state/ports';
import { OrcaAppUpdateSection } from './OrcaAppUpdateSection';

export function OrcaSettingsTab() {
  const { HomeSettingsGroup, HomeSettingsRow, HomeSettingsSpacer } = useSkin();
  const { externalLinks } = useCodingAgentPorts();

  return (
    <OrcaSettingsScreen title="Settings">
      <HomeSettingsGroup separatorInset={HOME_SETTINGS_SEPARATOR_INSET}>
        {ORCA_SETTINGS_MENU.map((item) => (
          <HomeSettingsRow
            key={item.id}
            icon={item.icon}
            label={item.label}
            navigates
            onPress={() => router.push(item.destination)}
          />
        ))}
      </HomeSettingsGroup>
      <HomeSettingsSpacer />
      <OrcaAppUpdateSection />
      <HomeSettingsSpacer />
      <HomeSettingsGroup separatorInset={HOME_SETTINGS_SEPARATOR_INSET}>
        {ORCA_SETTINGS_EXTERNAL.map((item) => (
          <HomeSettingsRow
            key={item.id}
            icon={item.icon}
            label={item.label}
            onPress={() => void externalLinks.open(item.url)}
          />
        ))}
      </HomeSettingsGroup>
    </OrcaSettingsScreen>
  );
}
