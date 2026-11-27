// Ported from `references/orca/mobile/src/home/tabs/MobileHomeTabBar.tsx` at
// `2c0cbbc279b61afe80a954fbf43de4f34ef2c32c` (Orca, MIT, Lovecast Inc.).
import type { LucideIcon } from 'lucide-react-native';
import BotIcon from 'lucide-react-native/dist/esm/icons/bot.mjs';
import CalendarClockIcon from 'lucide-react-native/dist/esm/icons/calendar-clock.mjs';
import GaugeIcon from 'lucide-react-native/dist/esm/icons/gauge.mjs';
import LayoutListIcon from 'lucide-react-native/dist/esm/icons/layout-list.mjs';
import SquareTerminalIcon from 'lucide-react-native/dist/esm/icons/square-terminal.mjs';
import { useEffect, useState } from 'react';
import { Platform, Pressable, Text, View } from 'react-native';
import Animated, {
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withSequence,
  withTiming,
} from 'react-native-reanimated';
import type { HomeTabBarProps, HomeTabId } from '@lib/skin';

import { homePalettes, homeSpacing, useOrcaScheme } from './appearance';
import { useHomeTabBarStyles } from './homeTabBarStyles';
import { useReducedMotionEnabled } from './useReducedMotion';

/** The Orca home's glyphs. */
const TAB_ICONS: Record<HomeTabId, LucideIcon> = {
  feed: LayoutListIcon,
  bots: BotIcon,
  agents: SquareTerminalIcon,
  usage: GaugeIcon,
  automations: CalendarClockIcon,
};

/** Strong ease-out for things entering or reacting to a touch. */
const HOME_EASE_OUT = Easing.bezier(0.23, 1, 0.32, 1);

/** Android has no 'tabbar' role and crashes on mount; iOS keeps its tab-bar trait. */
function homeTabBarRole(os: typeof Platform.OS): 'tabbar' | 'tablist' {
  return os === 'ios' ? 'tabbar' : 'tablist';
}

export function HomeTabBar(props: HomeTabBarProps) {
  const styles = useHomeTabBarStyles();
  return (
    <View
      style={[styles.bar, { paddingBottom: Math.max(props.bottomInset, homeSpacing.sm) }]}
      accessibilityRole={homeTabBarRole(Platform.OS)}
    >
      {props.tabs.map((tab) => (
        <TabButton
          key={tab.id}
          icon={TAB_ICONS[tab.id]}
          label={tab.label}
          selected={tab.id === props.active}
          badge={tab.badge ?? 0}
          onPress={() => props.onSelect(tab.id)}
        />
      ))}
    </View>
  );
}

function TabButton(props: {
  icon: LucideIcon;
  label: string;
  selected: boolean;
  badge: number;
  onPress: () => void;
}) {
  const styles = useHomeTabBarStyles();
  const colors = homePalettes[useOrcaScheme()];
  const [pressed, setPressed] = useState(false);
  const Icon = props.icon;
  const tint = props.selected ? colors.textPrimary : colors.textMuted;
  return (
    <Pressable
      style={styles.tab}
      accessibilityRole="tab"
      accessibilityLabel={props.badge > 0 ? `${props.label}, ${props.badge}` : props.label}
      accessibilityState={{ selected: props.selected }}
      onPressIn={() => setPressed(true)}
      onPressOut={() => setPressed(false)}
      onPress={props.onPress}
    >
      {/* Press feedback on the glyph only, so the label never jitters. */}
      <Animated.View
        style={[
          styles.icon,
          {
            transform: [{ scale: pressed ? 0.88 : 1 }],
            transitionProperty: 'transform',
            transitionDuration: pressed ? 90 : 160,
            transitionTimingFunction: 'ease-out',
          },
        ]}
      >
        <Icon size={25} strokeWidth={props.selected ? 2.2 : 1.8} color={tint} />
        {props.badge > 0 ? <TabBadge count={props.badge} /> : null}
      </Animated.View>
      <Text style={[styles.label, { color: tint }]} numberOfLines={1}>
        {props.label}
      </Text>
    </Pressable>
  );
}

/** Pops in when it first appears, and nudges when the count changes so a new ask is noticed. */
function TabBadge({ count }: { count: number }) {
  const styles = useHomeTabBarStyles();
  const reducedMotion = useReducedMotionEnabled() === true;
  const scale = useSharedValue(reducedMotion ? 1 : 0.6);
  const opacity = useSharedValue(reducedMotion ? 1 : 0);

  useEffect(() => {
    if (reducedMotion) {
      return;
    }
    opacity.value = withTiming(1, { duration: 140, easing: HOME_EASE_OUT });
    scale.value = withSequence(
      withTiming(1.18, { duration: 140, easing: HOME_EASE_OUT }),
      withTiming(1, { duration: 160, easing: HOME_EASE_OUT })
    );
  }, [count, opacity, reducedMotion, scale]);

  const style = useAnimatedStyle(
    () => ({ opacity: opacity.value, transform: [{ scale: scale.value }] }),
    [opacity, scale]
  );
  return (
    <Animated.View style={[styles.badge, style]} pointerEvents="none">
      <Text style={styles.badgeText}>{count > 99 ? '99+' : count}</Text>
    </Animated.View>
  );
}
