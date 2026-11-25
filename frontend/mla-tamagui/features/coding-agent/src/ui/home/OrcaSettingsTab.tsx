import {
  HOME_SETTINGS_SEPARATOR_INSET,
  useAgentComponents,
} from '@lib/agent-ui-contracts';
import { router } from 'expo-router';
import { ScrollView, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import {
  ORCA_SETTINGS_EXTERNAL,
  ORCA_SETTINGS_MENU,
} from '../../domain/settingsMenu';
import { useCodingAgentPorts } from '../../state/ports';
import { OrcaAppUpdateSection } from './OrcaAppUpdateSection';

/**
 * The staged home's settings tab: the settings menu, as the original carries it.
 *
 * The rows open the coding agent's own settings screens, named as the original
 * names them. Nothing here opens the app's own settings: those are a different
 * feature, with different screens.
 */
export function OrcaSettingsTab() {
  const { HomeTabHeader, HomeSettingsGroup, HomeSettingsRow } = useAgentComponents();
  const { externalLinks } = useCodingAgentPorts();
  const { bottom } = useSafeAreaInsets();

  return (
    <>
      <HomeTabHeader title="Settings" />
      <ScrollView
        contentContainerStyle={{ paddingHorizontal: 16, paddingTop: 8, paddingBottom: bottom }}
        showsVerticalScrollIndicator={false}
      >
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
      </ScrollView>
    </>
  );
}
