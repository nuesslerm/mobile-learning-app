import { Icon, Text, View } from '@lib/tamagui-design-system';
import {
  DrawerContentScrollView,
  useDrawerProgress,
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
import { useEffect } from 'react';
import { Keyboard, Platform, Pressable, StyleSheet } from 'react-native';
import Animated, { useAnimatedStyle } from 'react-native-reanimated';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { useResolvedScheme } from './NativeChromeBridge';

const DESTINATIONS = [
  { label: 'LeetCode Library', href: '/', icon: Code, testID: 'DrawerItemHome' },
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
    label: 'Tamil',
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
  const progress = useDrawerProgress();

  const dark = useResolvedScheme() === 'dark';
  const surfaceStyle = useAnimatedStyle(() => {
    const shade = Math.floor(14 + 4 * progress.value + 0.05);
    return { backgroundColor: `rgb(${shade}, ${shade}, ${shade})` };
  });
  useEffect(() => {
    if (open) Keyboard.dismiss();
  }, [open]);

  const openCodingAgent = () => {
    props.navigation.closeDrawer();
    if (pathname !== '/coding-agent') router.push('/coding-agent');
  };

  return (
    <View flex={1} bg="$background" testID="AppDrawer" aria-hidden={!open}>
      {dark && Platform.OS === 'ios' && (
        <Animated.View pointerEvents="none" style={[StyleSheet.absoluteFill, surfaceStyle]} />
      )}
      <DrawerContentScrollView {...props} style={styles.scroll}>
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
              : { 'aria-selected': selected };

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
      <DrawerFooter
        dark={dark}
        open={open}
        onCodingAgent={openCodingAgent}
      />
    </View>
  );
}

function DrawerFooter({
  dark,
  open,
  onCodingAgent,
}: {
  dark: boolean;
  open: boolean;
  onCodingAgent: () => void;
}) {
  const { bottom } = useSafeAreaInsets();
  return (
    <View
      testID="AppDrawerFooter"
      flexDirection="row"
      items="center"
      justify="flex-end"
      px={28}
      pt={8}
      pb={Math.max(28, bottom - 5)}
    >
      <Pressable
        testID="AppDrawerCodingAgent"
        accessibilityRole="button"
        accessibilityLabel="Open Coding agent"
        tabIndex={open ? 0 : -1}
        onPress={onCodingAgent}
        style={[styles.shortcut, { backgroundColor: dark ? '#FAFAFA' : '#24242A' }]}
      >
        <Icon as={Bot} size={16} color={dark ? '#222222' : '#FAFAFA'} />
        <Text color={dark ? '#222222' : '#FAFAFA'} fontSize={13} numberOfLines={1}>Coding agent</Text>
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  scroll: { flex: 1 },
  shortcut: { width: 122, height: 48, borderRadius: 24, paddingHorizontal: 7, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 6 },
});
