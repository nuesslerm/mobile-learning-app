import type { HomeSettingsScreenProps } from '@lib/skin';
import { Icon, Text } from '@lib/tamagui-design-system';
import { ChevronLeft } from '@lib/tamagui-design-system/icons';
import {
  ScrollView,
  XStack,
  YStack,
} from '@lib/tamagui-design-system/primitives';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

/** A settings screen: the bar with its back button and title, then its rows. */
export function HomeSettingsScreen(props: HomeSettingsScreenProps) {
  const { bottom } = useSafeAreaInsets();
  return (
    <YStack flex={1} bg="$background">
      <XStack height={52} items="center" justify="center">
        <XStack
          position="absolute"
          l="$3"
          width={36}
          height={36}
          rounded={18}
          items="center"
          justify="center"
          role="button"
          aria-label="Back"
          onPress={props.onBack}
          pressStyle={{ bg: '$color3' }}
          cursor="pointer"
        >
          <Icon as={ChevronLeft} size={22} />
        </XStack>
        <Text variant="strong" role="heading" maxW="62%" numberOfLines={1}>
          {props.title}
        </Text>
      </XStack>
      <ScrollView
        showsVerticalScrollIndicator={false}
        contentContainerStyle={{
          px: '$4',
          pt: '$2',
          pb: bottom + 24,
        }}
      >
        {props.children}
      </ScrollView>
    </YStack>
  );
}
