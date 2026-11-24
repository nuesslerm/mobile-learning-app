import { Icon, Text, View } from '@lib/tamagui-design-system';
import {
  DrawerContentScrollView,
  useDrawerStatus,
  type DrawerContentComponentProps,
} from 'expo-router/drawer';
import { router, usePathname } from 'expo-router';
import {
  Code,
  Palette,
  Bot,
  PenLine,
  Settings,
  Cpu,
} from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { Platform } from 'react-native';

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

export function AppDrawerContent(props: DrawerContentComponentProps) {
  const pathname = usePathname();
  const open = useDrawerStatus() === 'open';

  return (
    <View flex={1} bg="$background" testID="AppDrawer" aria-hidden={!open}>
      <DrawerContentScrollView {...props}>
        <YStack px="$3" pt="$3" pb="$5">
          <Text variant="h3" role="heading" testID="AppDrawerTitle">
            Learning workspace
          </Text>
        </YStack>
        <YStack gap="$1">
          {DESTINATIONS.map(({ href, icon, label, testID }) => {
            const selected =
              pathname === href || (href !== '/' && pathname.startsWith(`${href}/`));
            const selection = Platform.OS === 'web'
              ? { 'aria-current': selected ? ('page' as const) : undefined }
              : { accessibilityState: { selected } };

            return (
              <XStack
                {...selection}
                key={href}
                testID={testID}
                role="button"
                tabIndex={open ? 0 : -1}
                items="center"
                gap="$3"
                px="$3"
                minH={48}
                rounded="$6"
                bg={selected ? '$color3' : 'transparent'}
                hoverStyle={{ bg: '$color3' }}
                pressStyle={{ bg: '$color4' }}
                cursor="pointer"
                onPress={() => {
                  props.navigation.closeDrawer();
                  if (pathname !== href) router.push(href);
                }}
              >
                <Icon as={icon} size={19} />
                <Text testID={`${testID}Label`}>{label}</Text>
              </XStack>
            );
          })}
        </YStack>
      </DrawerContentScrollView>
    </View>
  );
}
