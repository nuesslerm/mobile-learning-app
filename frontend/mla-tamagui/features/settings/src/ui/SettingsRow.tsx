import { Logger } from '@lib/logging';
import { Icon, ListItem, Text, View } from '@lib/tamagui-design-system';
import { router, type Href } from 'expo-router';
import type { LucideIcon } from 'lucide-react-native';

export type SettingsRowProps = {
  icon: LucideIcon;
  label: string;
  /** Optional right-hand current value, rendered in `$color11`. */
  value?: string;
  href: Href;
  testID: string;
};

export function SettingsRow({
  icon,
  label,
  value,
  href,
  testID,
}: SettingsRowProps) {
  return (
    <ListItem
      variant="grouped"
      showChevron
      pressLog="off"
      onPress={() => {
        Logger.info('settings.row.pressed', { href, label });
        router.push(href);
      }}
      testID={testID}
    >
      <View
        flexDirection="row"
        items="center"
        gap="$3"
        flex={1}
        testID={`${testID}Content`}
      >
        <Icon as={icon} size={18} color="$color11" testID={`${testID}Icon`} />
        <Text flex={1} testID={`${testID}Label`}>
          {label}
        </Text>
        {value ? (
          <Text color="$color11" testID={`${testID}Value`}>
            {value}
          </Text>
        ) : null}
      </View>
    </ListItem>
  );
}
