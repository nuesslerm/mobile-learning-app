import { Logger } from '@lib/logging';
import {
  Button,
  Icon,
  InsetGroup,
  ScreenHeader,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { LogOut, Pencil } from '@lib/tamagui-design-system/icons';
import * as React from 'react';
import { ScrollView } from 'react-native';

import { backTo } from './backTo';

export function ProfileSettingsScreen() {
  const [displayName, setDisplayName] = React.useState('Ada Lovelace');
  const [account, setAccount] = React.useState('ada@example.com');

  const handleSignOut = () => {
    setDisplayName('');
    setAccount('');
  };

  return (
    <View flex={1} bg="$background" testID="ProfileSettingsScreen">
      <ScreenHeader
        testID="ProfileSettingsScreenHeader"
        title="Profile"
        onBack={() => {
          Logger.info('settings.profile.back', { to: '/settings' });
          backTo('/settings');
        }}
      />
      <ScrollView
        testID="ProfileSettingsScreen-ScrollView"
        contentContainerStyle={{ paddingVertical: 16, gap: 12 }}
      >
        <InsetGroup testID="ProfileSettingsScreen-Group">
          <View
            flexDirection="row"
            items="center"
            justify="space-between"
            gap="$3"
            px="$4"
            py="$3"
            testID="ProfileSettingsScreen-DisplayNameRow"
          >
            <View
              flexDirection="row"
              items="center"
              gap="$3"
              flex={1}
              testID="ProfileSettingsScreen-DisplayNameLeading"
            >
              <Icon
                as={Pencil}
                size={18}
                color="$color11"
                testID="ProfileSettingsScreen-DisplayNameIcon"
              />
              <Text
                color="$color11"
                testID="ProfileSettingsScreen-DisplayNameLabel"
              >
                Display name
              </Text>
            </View>
            <Text testID="ProfileSettingsScreen-DisplayNameValue">
              {displayName || 'Not set'}
            </Text>
          </View>
          <View
            flexDirection="row"
            items="center"
            justify="space-between"
            gap="$3"
            px="$4"
            py="$3"
            testID="ProfileSettingsScreen-AccountRow"
          >
            <View
              flexDirection="row"
              items="center"
              gap="$3"
              flex={1}
              testID="ProfileSettingsScreen-AccountLeading"
            >
              <Text
                color="$color11"
                testID="ProfileSettingsScreen-AccountLabel"
              >
                Account
              </Text>
            </View>
            <Text testID="ProfileSettingsScreen-AccountValue">
              {account || 'Not signed in'}
            </Text>
          </View>
          <View p="$3" testID="ProfileSettingsScreen-SignOutRow">
            <Button
              variant="destructive"
              icon={LogOut}
              onPress={handleSignOut}
              testID="ProfileSettingsScreen-SignOutButton"
            >
              Sign out
            </Button>
          </View>
        </InsetGroup>
        <Text variant="muted" mx="$4" testID="ProfileSettingsScreen-Note">
          Local only — this screen is not connected to an account service.
        </Text>
      </ScrollView>
    </View>
  );
}
