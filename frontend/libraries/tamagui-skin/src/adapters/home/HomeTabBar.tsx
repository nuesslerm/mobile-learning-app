import type { HomeTabBarProps, HomeTabId } from '@lib/skin';
import { Badge, Icon, Text } from '@lib/tamagui-design-system';
import { Bot, CalendarClock, Gauge, LayoutList, SquareTerminal } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import type { LucideIcon } from 'lucide-react-native';

const TAB_ICONS: Record<HomeTabId, LucideIcon> = {
  feed: LayoutList,
  bots: Bot,
  agents: SquareTerminal,
  usage: Gauge,
  automations: CalendarClock,
};

/** The home's tabs as the app draws its own dock: one capsule, the picked tab filled. */
export function HomeTabBar(props: HomeTabBarProps) {
  return (
    <YStack px={21} pt="$2" pb={Math.max(props.bottomInset - 14, 10)} bg="$background">
      <XStack
        role="tablist"
        testID="skin-home-tab-bar"
        items="center"
        px="$2"
        minH={64}
        rounded={32}
        bg="$color2"
        borderWidth={1}
        borderColor="$borderColor"
      >
        {props.tabs.map((tab) => {
          const selected = tab.id === props.active;
          const badge = tab.badge ?? 0;
          const ink = selected ? '$color' : '$color10';
          return (
            <YStack
              key={tab.id}
              testID={`HomeTab-${tab.id}`}
              role="tab" accessible
              aria-label={badge > 0 ? `${tab.label}, ${badge}` : tab.label}
              aria-selected={selected}
              flex={1}
              minH={52}
              rounded={26}
              items="center"
              justify="center"
              gap="$0.5"
              bg={selected ? '$color4' : 'transparent'}
              pressStyle={{ opacity: 0.7 }}
              cursor="pointer"
              onPress={() => props.onSelect(tab.id)}
            >
              <YStack>
                <Icon as={TAB_ICONS[tab.id]} size={23} color={ink} />
                {badge > 0 ? (
                  <Badge variant="destructive" position="absolute" t={-6} l={14} px="$1.5" py={0}>
                    {badge > 99 ? '99+' : badge}
                  </Badge>
                ) : null}
              </YStack>
              <Text fontSize={11} lineHeight={14} fontWeight="500" color={ink} numberOfLines={1}>
                {tab.label}
              </Text>
            </YStack>
          );
        })}
      </XStack>
    </YStack>
  );
}
