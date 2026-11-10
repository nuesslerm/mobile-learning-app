import { Logger } from '@lib/logging';
import {
  Icon,
  InsetGroup,
  ListItem,
  ScreenHeader,
  Text,
  ToggleGroup,
  ToggleGroupItem,
  View,
} from '@lib/tamagui-design-system';
import { MessageSquare, Sparkles, Zap } from '@lib/tamagui-design-system/icons';
import * as React from 'react';
import { ScrollView } from 'react-native';

import { backTo } from './backTo';

type CategoryId = 'push' | 'digest' | 'updates';

const CATEGORIES: { id: CategoryId; label: string; icon: typeof Zap }[] = [
  { id: 'push', label: 'Push notifications', icon: Zap },
  { id: 'digest', label: 'Digest email', icon: MessageSquare },
  { id: 'updates', label: 'Product updates', icon: Sparkles },
];

export function NotificationSettingsScreen() {
  const [prefs, setPrefs] = React.useState<Record<CategoryId, 'on' | 'off'>>({
    push: 'on',
    digest: 'off',
    updates: 'on',
  });

  const setCategory = (id: CategoryId, value: string) => {
    setPrefs((prev) => ({
      ...prev,
      [id]: value === 'on' ? 'on' : 'off',
    }));
  };

  return (
    <View flex={1} bg="$background" testID="NotificationSettingsScreen">
      <ScreenHeader
        testID="NotificationSettingsScreenHeader"
        title="Notifications"
        onBack={() => {
          Logger.info('settings.notifications.back', { to: '/settings' });
          backTo('/settings');
        }}
      />
      <ScrollView
        testID="NotificationSettingsScreen-ScrollView"
        contentContainerStyle={{ paddingVertical: 16, gap: 12 }}
      >
        <InsetGroup testID="NotificationSettingsScreen-Group">
          {CATEGORIES.map((category) => (
            <ListItem
              key={category.id}
              variant="grouped"
              pressLog="off"
              testID={`NotificationSettingsScreen-Row-${category.id}`}
            >
              <View
                flexDirection="row"
                items="center"
                justify="space-between"
                gap="$3"
                flex={1}
                testID={`NotificationSettingsScreen-RowContent-${category.id}`}
              >
                <View
                  flexDirection="row"
                  items="center"
                  gap="$3"
                  flex={1}
                  testID={`NotificationSettingsScreen-RowLeading-${category.id}`}
                >
                  <Icon
                    as={category.icon}
                    size={18}
                    color="$color11"
                    testID={`NotificationSettingsScreen-Icon-${category.id}`}
                  />
                  <Text
                    testID={`NotificationSettingsScreen-Label-${category.id}`}
                  >
                    {category.label}
                  </Text>
                </View>
                <ToggleGroup
                  type="single"
                  size="sm"
                  value={prefs[category.id]}
                  onValueChange={(value) => {
                    Logger.info('settings.notifications.categoryChanged', {
                      category: category.id,
                      value,
                    });
                    setCategory(category.id, value);
                  }}
                  testID={`NotificationSettingsScreen-Toggle-${category.id}`}
                >
                  <ToggleGroupItem
                    value="off"
                    testID={`NotificationSettingsScreen-Toggle-${category.id}-Off`}
                  >
                    <Text
                      variant="small"
                      testID={`NotificationSettingsScreen-Toggle-${category.id}-OffLabel`}
                    >
                      Off
                    </Text>
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="on"
                    testID={`NotificationSettingsScreen-Toggle-${category.id}-On`}
                  >
                    <Text
                      variant="small"
                      testID={`NotificationSettingsScreen-Toggle-${category.id}-OnLabel`}
                    >
                      On
                    </Text>
                  </ToggleGroupItem>
                </ToggleGroup>
              </View>
            </ListItem>
          ))}
        </InsetGroup>
        <Text variant="muted" mx="$4" testID="NotificationSettingsScreen-Note">
          Local only — this screen is not connected to an account service.
        </Text>
      </ScrollView>
    </View>
  );
}
