import { Logger } from '@lib/logging';
import {
  getDefaultTheme,
  getPresetThemeById,
  InsetGroup,
  ScreenHeader,
  Text,
  usePresetThemeStore,
  View,
} from '@lib/tamagui-design-system';
import {
  Bell,
  HardDrive,
  Info,
  Palette,
  User,
} from '@lib/tamagui-design-system/icons';
import { ScrollView } from 'react-native';

import { SettingsRow } from './SettingsRow';
import { backTo } from '@lib/tamagui-design-system/lib/backTo';

export function SettingsIndexScreen() {
  const activeThemeId = usePresetThemeStore((s) => s.activeThemeId);
  const activePreset =
    (activeThemeId ? getPresetThemeById(activeThemeId) : undefined) ??
    getDefaultTheme();

  return (
    <View flex={1} bg="$background" testID="SettingsIndexScreen">
      <ScreenHeader
        testID="SettingsIndexScreenHeader"
        title="Settings"
        onBack={() => {
          Logger.info('settings.index.back', { to: '/' });
          backTo('/');
        }}
      />
      <ScrollView
        testID="SettingsIndexScreen-ScrollView"
        contentContainerStyle={{ paddingVertical: 16, gap: 12 }}
      >
        <InsetGroup testID="SettingsIndexScreen-GroupProfile">
          <SettingsRow
            icon={User}
            label="Profile"
            value="Ada Lovelace"
            href="/settings/profile"
            testID="SettingsRow-Profile"
          />
        </InsetGroup>
        <Text
          variant="muted"
          textTransform="uppercase"
          letterSpacing={0.5}
          mx="$4"
          testID="SettingsIndexScreen-GroupLabelPreferences"
        >
          Preferences
        </Text>
        <InsetGroup testID="SettingsIndexScreen-GroupPreferences">
          <SettingsRow
            icon={Palette}
            label="Appearance"
            value={activePreset.name}
            href="/settings/appearance"
            testID="SettingsRow-Appearance"
          />
          <SettingsRow
            icon={Bell}
            label="Notifications"
            href="/settings/notifications"
            testID="SettingsRow-Notifications"
          />
        </InsetGroup>
        <Text
          variant="muted"
          textTransform="uppercase"
          letterSpacing={0.5}
          mx="$4"
          testID="SettingsIndexScreen-GroupLabelSystem"
        >
          System
        </Text>
        <InsetGroup testID="SettingsIndexScreen-GroupSystem">
          <SettingsRow
            icon={HardDrive}
            label="Storage"
            href="/settings/storage"
            testID="SettingsRow-Storage"
          />
          <SettingsRow
            icon={Info}
            label="About"
            value="1.0.0"
            href="/settings/about"
            testID="SettingsRow-About"
          />
        </InsetGroup>
      </ScrollView>
    </View>
  );
}
