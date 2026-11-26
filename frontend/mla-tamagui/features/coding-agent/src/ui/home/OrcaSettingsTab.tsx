import {
  HOME_SETTINGS_SEPARATOR_INSET,
  useAgentComponents,
} from '@lib/agent-ui-contracts';
import { router } from 'expo-router';
import { View } from 'react-native';
import { OrcaSettingsScreen } from '../settings/OrcaSettingsScreen';

import {
  ORCA_SETTINGS_EXTERNAL,
  ORCA_SETTINGS_MENU,
} from '../../domain/settingsMenu';
import { useCodingAgentPorts } from '../../state/ports';
import { OrcaAppUpdateSection } from './OrcaAppUpdateSection';

export function OrcaSettingsTab() {
  const { HomeSettingsGroup, HomeSettingsRow } = useAgentComponents();
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
      <View style={{ height: 16 }} />
      <OrcaAppUpdateSection />
      <View style={{ height: 16 }} />
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
