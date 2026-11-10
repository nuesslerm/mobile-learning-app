import { Text, View, Icon } from '@lib/tamagui-design-system';
// SDK 57's expo-router vendors the drawer navigator and its types, so the
// component props must come from there: @react-navigation/drawer's own copy is
// a structurally different type and the two do not assign to each other.
import {
  DrawerContentScrollView,
  type DrawerContentComponentProps,
} from 'expo-router/drawer';
import { router } from 'expo-router';
import {
  Code,
  Palette,
  Bot,
  PenLine,
  Settings,
  Cpu,
} from '@lib/tamagui-design-system/icons';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

const DESTINATIONS = [
  { label: 'Blind 75', href: '/', icon: Code, testID: 'DrawerItemHome' },
  {
    label: 'Design system',
    href: '/showcase',
    icon: Palette,
    testID: 'DrawerItemShowcase',
  },
  {
    label: 'Coding agent',
    href: '/coding-agent',
    icon: Bot,
    testID: 'DrawerItemCodingAgent',
  },
  {
    label: 'Tamil writing',
    href: '/tamil',
    icon: PenLine,
    testID: 'DrawerItemTamil',
  },
  {
    label: 'Settings',
    href: '/settings',
    icon: Settings,
    testID: 'DrawerItemSettings',
  },
  {
    label: 'Dashboards',
    href: '/dashboards',
    icon: Cpu,
    testID: 'DrawerItemDashboards',
  },
] as const;

/** A handful of destinations and a title. App #1's drawer earns more; this one does not. */
export function AppDrawerContent(props: DrawerContentComponentProps) {
  const insets = useSafeAreaInsets();
  const activeRoute = props.state.routes[props.state.index]?.name;

  return (
    <View flex={1} bg="$background" testID="AppDrawer">
      <DrawerContentScrollView {...props} scrollEnabled={false}>
        <YStack px="$4" pt={insets.top} pb="$3">
          <Text variant="h3" testID="AppDrawerTitle">
            mla-tamagui
          </Text>
          <Text variant="muted" testID="AppDrawerSubtitle">
            App #2 · Tamagui
          </Text>
        </YStack>
        <YStack px="$2" gap="$1">
          {DESTINATIONS.map((destination) => (
            <XStack
              key={destination.href}
              testID={destination.testID}
              items="center"
              gap="$3"
              px="$3"
              py="$2.5"
              rounded="$4"
              bg={activeRoute === destination.href ? '$color4' : 'transparent'}
              pressStyle={{ bg: '$color4' }}
              cursor="pointer"
              onPress={() => {
                props.navigation.closeDrawer();
                router.push(destination.href);
              }}
            >
              <Icon as={destination.icon} size={18} />
              <Text testID={`${destination.testID}Label`}>
                {destination.label}
              </Text>
            </XStack>
          ))}
        </YStack>
      </DrawerContentScrollView>
    </View>
  );
}
