import { Logger } from '@lib/logging';
import {
  Icon,
  InsetGroup,
  ListItem,
  ScreenHeader,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Code, Globe, Info, Zap } from '@lib/tamagui-design-system/icons';
import * as React from 'react';
import { Linking, ScrollView } from 'react-native';

import { backTo } from './backTo';

const REPOSITORY_URL = 'https://github.com/nuesslerm/mobile-learning-app';
const LICENCES_URL = `${REPOSITORY_URL}/blob/main/LICENSE`;
const NOT_AVAILABLE = 'not available in this build';

type AppInfo = {
  version: string | null;
  channel: string | null;
};

async function loadAppInfo(): Promise<AppInfo> {
  try {
    const [constants, updates] = await Promise.all([
      import('expo-constants'),
      import('expo-updates'),
    ]);
    const config = constants.default.expoConfig ?? null;
    const info: AppInfo = {
      version: config?.version ?? null,
      channel: updates.channel ?? null,
    };
    Logger.withMetadata({
      version: info.version,
      channel: info.channel,
    }).info('settings.about.app_info_loaded');
    return info;
  } catch (err) {
    Logger.withError(err).warn('settings.about.app_info_load_failed');
    return { version: null, channel: null };
  }
}

type AboutValueRowProps = {
  icon: typeof Info;
  label: string;
  value: string | null | undefined;
  testID: string;
};

function AboutValueRow({ icon, label, value, testID }: AboutValueRowProps) {
  return (
    <View
      flexDirection="row"
      items="center"
      justify="space-between"
      gap="$3"
      px="$4"
      py="$3"
      testID={testID}
    >
      <View
        flexDirection="row"
        items="center"
        gap="$3"
        flex={1}
        testID={`${testID}Leading`}
      >
        <Icon as={icon} size={18} color="$color11" testID={`${testID}Icon`} />
        <Text color="$color11" testID={`${testID}Label`}>
          {label}
        </Text>
      </View>
      <View flex={1} testID={`${testID}ValueWrap`}>
        <Text testID={`${testID}Value`}>
          {value?.trim() ? value : NOT_AVAILABLE}
        </Text>
      </View>
    </View>
  );
}

function openUrl(href: string, event: string): () => void {
  return () => {
    Logger.info(event, { href });
    void Linking.openURL(href);
  };
}

export function AboutSettingsScreen() {
  const [info, setInfo] = React.useState<AppInfo>({
    version: null,
    channel: null,
  });

  React.useEffect(() => {
    let alive = true;
    void loadAppInfo().then((next) => {
      if (alive) setInfo(next);
    });
    return () => {
      alive = false;
    };
  }, []);

  return (
    <View flex={1} bg="$background" testID="AboutSettingsScreen">
      <ScreenHeader
        testID="AboutSettingsScreenHeader"
        title="About"
        onBack={() => {
          Logger.info('settings.about.back', { to: '/settings' });
          backTo('/settings');
        }}
      />
      <ScrollView
        testID="AboutSettingsScreen-ScrollView"
        contentContainerStyle={{ paddingVertical: 16, gap: 12 }}
      >
        <InsetGroup testID="AboutSettingsScreen-GroupApp">
          <AboutValueRow
            icon={Info}
            label="App version"
            value={info.version}
            testID="AboutSettingsScreen-Version"
          />
          <AboutValueRow
            icon={Zap}
            label="Update channel"
            value={info.channel}
            testID="AboutSettingsScreen-Channel"
          />
        </InsetGroup>
        <Text
          variant="muted"
          textTransform="uppercase"
          letterSpacing={0.5}
          mx="$4"
          testID="AboutSettingsScreen-GroupLabelLinks"
        >
          Resources
        </Text>
        <InsetGroup testID="AboutSettingsScreen-GroupLinks">
          <ListItem
            variant="grouped"
            onPress={openUrl(REPOSITORY_URL, 'settings.about.open_repository')}
            testID="AboutSettingsScreen-Repository"
          >
            <View
              flexDirection="row"
              items="center"
              gap="$3"
              testID="AboutSettingsScreen-RepositoryContent"
            >
              <Icon
                as={Code}
                size={18}
                color="$color11"
                testID="AboutSettingsScreen-RepositoryIcon"
              />
              <Text testID="AboutSettingsScreen-RepositoryLabel">
                Repository
              </Text>
            </View>
          </ListItem>
          <ListItem
            variant="grouped"
            onPress={openUrl(LICENCES_URL, 'settings.about.open_licences')}
            testID="AboutSettingsScreen-Licences"
          >
            <View
              flexDirection="row"
              items="center"
              gap="$3"
              testID="AboutSettingsScreen-LicencesContent"
            >
              <Icon
                as={Globe}
                size={18}
                color="$color11"
                testID="AboutSettingsScreen-LicencesIcon"
              />
              <Text testID="AboutSettingsScreen-LicencesLabel">
                Open-source licences
              </Text>
            </View>
          </ListItem>
        </InsetGroup>
      </ScrollView>
    </View>
  );
}
