import { Logger } from '@lib/logging';
import {
  Button,
  InsetGroup,
  ScreenHeader,
  StatTile,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Cpu, Square, Trash2 } from '@lib/tamagui-design-system/icons';
import * as React from 'react';
import { ScrollView } from 'react-native';

import { backTo } from '@lib/tamagui-design-system/lib/backTo';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const i = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1
  );
  const value = bytes / 1024 ** i;
  return `${value.toFixed(value >= 10 ? 1 : 2)} ${units[i]}`;
}

const INITIAL_BUNDLE_BYTES = 50_510_000;
const INITIAL_IMAGE_BYTES = 134_900_000;

export function StorageSettingsScreen() {
  const [bundleBytes, setBundleBytes] = React.useState(INITIAL_BUNDLE_BYTES);
  const [imageBytes, setImageBytes] = React.useState(INITIAL_IMAGE_BYTES);

  const handleClearCache = () => {
    setBundleBytes(0);
    setImageBytes(0);
  };

  return (
    <View flex={1} bg="$background" testID="StorageSettingsScreen">
      <ScreenHeader
        testID="StorageSettingsScreenHeader"
        title="Storage"
        onBack={() => {
          Logger.info('settings.storage.back', { to: '/settings' });
          backTo('/settings');
        }}
      />
      <ScrollView
        testID="StorageSettingsScreen-ScrollView"
        contentContainerStyle={{ paddingVertical: 16, gap: 12 }}
      >
        <InsetGroup testID="StorageSettingsScreen-Group">
          <View
            flexDirection="row"
            gap="$2"
            p="$3"
            testID="StorageSettingsScreen-Tiles"
          >
            <StatTile
              icon={Cpu}
              value={formatBytes(bundleBytes)}
              label="Bundle cache"
            />
            <StatTile
              icon={Square}
              value={formatBytes(imageBytes)}
              label="Image cache"
            />
          </View>
          <View p="$3" testID="StorageSettingsScreen-ClearRow">
            <Button
              variant="destructive"
              icon={Trash2}
              onPress={handleClearCache}
              testID="StorageSettingsScreen-ClearCacheButton"
            >
              Clear cache
            </Button>
          </View>
        </InsetGroup>
        <Text variant="muted" mx="$4" testID="StorageSettingsScreen-Note">
          Local only — this screen is not connected to an account service.
        </Text>
      </ScrollView>
    </View>
  );
}
