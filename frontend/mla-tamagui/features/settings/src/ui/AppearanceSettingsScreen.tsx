import { Logger } from '@lib/logging';
import {
  Icon,
  ScreenHeader,
  SegmentedControl,
  Text,
  View,
  getPresetThemeById,
  usePresetThemeStore,
} from '@lib/tamagui-design-system';
import { Check } from '@lib/tamagui-design-system/icons';
import { PRESET_COLORS } from '@lib/tamagui-design-system/theme/presetColors';
import * as React from 'react';
import { ScrollView, useColorScheme } from 'react-native';

import { backTo } from '@lib/tamagui-design-system/lib/backTo';

function ThemeTile({
  presetId,
  name,
  scheme,
  isActive,
  onPress,
  testID,
}: {
  presetId: string;
  name: string;
  scheme: 'light' | 'dark';
  isActive: boolean;
  onPress: () => void;
  testID: string;
}) {
  return (
    <View theme={`${presetId}_${scheme}`} width="48%" testID={testID}>
      <View
        onPress={onPress}
        role="radio"
        aria-selected={isActive}
        aria-label={`${name}${isActive ? ', active' : ''}`}
        height={92}
        gap="$1.5"
        p="$2"
        rounded="$xl"
        bg="$card"
        borderWidth={isActive ? 1.5 : 1}
        borderColor={isActive ? '$primary' : '$border'}
        pressStyle={{ opacity: 0.8 }}
        cursor="pointer"
        testID={`AppearanceSettingsScreen-ThemeTilePress-${presetId}`}
      >
        <View
          flexDirection="row"
          gap="$1"
          testID={`AppearanceSettingsScreen-ThemeTileSwatches-${presetId}`}
        >
          <View
            flex={1}
            height={30}
            rounded="$md"
            bg="$primary"
            testID={`AppearanceSettingsScreen-ThemeTilePrimary-${presetId}`}
          />
          <View
            flex={1}
            height={30}
            rounded="$md"
            bg="$accent"
            testID={`AppearanceSettingsScreen-ThemeTileAccent-${presetId}`}
          />
          <View
            flex={1}
            height={30}
            rounded="$md"
            bg="$color6"
            testID={`AppearanceSettingsScreen-ThemeTileColor6-${presetId}`}
          />
        </View>
        <View
          flexDirection="row"
          items="center"
          gap="$1"
          testID={`AppearanceSettingsScreen-ThemeTileLabelRow-${presetId}`}
        >
          <Text
            variant="small"
            numberOfLines={1}
            ellipsizeMode="tail"
            flex={1}
            color="$foreground"
            testID={`AppearanceSettingsScreen-ThemeTileLabel-${presetId}`}
          >
            {name}
          </Text>
          {isActive && (
            <View
              items="center"
              justify="center"
              rounded="$pill"
              p="$0.5"
              bg="$primary"
              testID={`AppearanceSettingsScreen-ThemeTileCheck-${presetId}`}
            >
              <Icon
                as={Check}
                size={12}
                color="$primary-foreground"
                testID={`AppearanceSettingsScreen-ThemeTileCheckIcon-${presetId}`}
              />
            </View>
          )}
        </View>
      </View>
    </View>
  );
}

export function AppearanceSettingsScreen() {
  const themeMode = usePresetThemeStore((s) => s.themeMode);
  const setThemeMode = usePresetThemeStore((s) => s.setThemeMode);
  const activeThemeId = usePresetThemeStore((s) => s.activeThemeId);
  const setActiveTheme = usePresetThemeStore((s) => s.setActiveTheme);
  const systemScheme = useColorScheme();
  const scheme: 'light' | 'dark' =
    themeMode === 'system'
      ? systemScheme === 'dark'
        ? 'dark'
        : 'light'
      : themeMode;

  const handleModeChange = (value: string) => {
    if (value !== 'system' && value !== 'light' && value !== 'dark') return;
    Logger.info('settings.themeMode.changed', { mode: value });
    setThemeMode(value);
  };

  return (
    <View flex={1} bg="$background" testID="AppearanceSettingsScreen">
      <ScreenHeader
        testID="AppearanceSettingsScreenHeader"
        title="Appearance"
        onBack={() => {
          Logger.info('settings.appearance.back', { to: '/settings' });
          backTo('/settings');
        }}
      />
      <ScrollView
        testID="AppearanceSettingsScreen-ScrollView"
        contentContainerStyle={{
          paddingVertical: 16,
          paddingHorizontal: 16,
          gap: 16,
        }}
      >
        <SegmentedControl
          testID="AppearanceSettingsScreen-ModeControl"
          options={[
            { value: 'system', label: 'System' },
            { value: 'light', label: 'Light' },
            { value: 'dark', label: 'Dark' },
          ]}
          value={themeMode}
          onChange={handleModeChange}
        />
        <View
          flexDirection="row"
          flexWrap="wrap"
          gap="$2"
          testID="AppearanceSettingsScreen-ThemeGrid"
        >
          {Object.keys(PRESET_COLORS).map((presetId) => (
            <ThemeTile
              key={presetId}
              presetId={presetId}
              name={getPresetThemeById(presetId)?.name ?? presetId}
              scheme={scheme}
              isActive={activeThemeId === presetId}
              onPress={() => {
                Logger.info('settings.presetThemeSelected', {
                  themeId: presetId,
                  themeName: getPresetThemeById(presetId)?.name ?? presetId,
                });
                setActiveTheme(presetId);
              }}
              testID={`AppearanceSettingsScreen-ThemeTile-${presetId}`}
            />
          ))}
        </View>
      </ScrollView>
    </View>
  );
}
