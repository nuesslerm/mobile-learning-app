import type { LucideIcon } from 'lucide-react-native';
import { useContext } from 'react';
import { SafeAreaInsetsContext } from 'react-native-safe-area-context';

import { ChevronLeft } from '../icons';
import { useHeaderHeightStore } from '../lib/headerHeightStore';
import { XStack, YStack } from '../primitives';
import { Icon } from '../ui/icon';
import { Text } from '../ui/text';

import { HeaderMenu, type HeaderMenuProps } from './header-menu';

export type ScreenHeaderAction = {
  icon: LucideIcon;
  accessibilityLabel: string;
  onPress: () => void;
  testID: string;
};

export type ScreenHeaderProps = {
  title: string;
  /** Secondary line under the title. Keep it to context, not restatement. */
  subtitle?: string;
  onBack?: () => void;
  backAccessibilityLabel?: string;
  /** Grouped into one pill on the right, the way a chat header groups its controls. */
  actions?: ScreenHeaderAction[];
  /** Overflow control, rendered last in the same pill as `actions`. */
  menu?: Omit<HeaderMenuProps, 'size'>;
  testID: string;
};

// Both flanks are the same width so the title is genuinely centred. Letting
// them size to content drifts the title whenever the action count changes --
// which reads as a wobble when navigating between screens. The width grows with
// the control count rather than being a constant, because the old fixed 96 fit
// exactly two controls and silently clipped a third.
const FLANK_MIN_WIDTH = 96;
const ACTION_SIZE = 34;
const PILL_PADDING = 12;

function flankWidth(controlCount: number): number {
  if (controlCount === 0) return FLANK_MIN_WIDTH;
  return Math.max(
    FLANK_MIN_WIDTH,
    controlCount * ACTION_SIZE + (controlCount - 1) * 4 + PILL_PADDING
  );
}
// Used when no SafeAreaProvider is above this header (see below). Roughly a
// modern phone's status bar, so the header never sits under the clock.
const DEFAULT_TOP_INSET = 44;

const CONTROL_SIZE = 40;

function CircleButton({
  icon,
  onPress,
  accessibilityLabel,
  testID,
}: {
  icon: LucideIcon;
  onPress?: () => void;
  accessibilityLabel: string;
  testID: string;
}) {
  return (
    <XStack
      testID={testID}
      role="button"
      aria-label={accessibilityLabel}
      width={CONTROL_SIZE}
      height={CONTROL_SIZE}
      items="center"
      justify="center"
      rounded={9999}
      bg="$color3"
      pressStyle={{ bg: '$color5' }}
      cursor="pointer"
      onPress={onPress}
    >
      <Icon as={icon} size={20} />
    </XStack>
  );
}

/**
 * The app's screen header: a circular back control, a stacked title/subtitle,
 * and the screen's actions grouped into one pill on the right.
 *
 * Modelled on a chat header rather than a platform navigation bar. The
 * difference that matters is the second line: a nav bar has one slot, so
 * context ("Array & Hashing · Easy") has to go somewhere else or be dropped,
 * and the back button ends up carrying it as a long text label that crowds the
 * title. Stacking it keeps the back control to a chevron.
 */
export function ScreenHeader({
  title,
  subtitle,
  onBack,
  backAccessibilityLabel = 'Go back',
  actions = [],
  menu,
  testID,
}: ScreenHeaderProps) {
  const controlCount = actions.length + (menu ? 1 : 0);
  const flank = flankWidth(controlCount);

  // `useContext`, not `useSafeAreaInsets`: that hook THROWS ("No safe area
  // value available") when no provider is above it, and a custom navigator
  // `header` renders outside the app's SafeAreaProvider subtree. It took the
  // whole screen down to a blank white tree -- with the app crashed there were
  // no testIDs left, so the harness could not even find the app frame to report
  // the error from. The context accessor returns null instead, so the header
  // degrades to a sane inset rather than exploding.
  const setHeaderHeight = useHeaderHeightStore((state) => state.setHeight);
  const insets = useContext(SafeAreaInsetsContext);
  const topInset = insets?.top ?? DEFAULT_TOP_INSET;

  return (
    <XStack
      testID={testID}
      // Publishes what it actually laid out, so the toast host can sit clear of
      // it without predicting its height. See lib/headerHeightStore.
      onLayout={(e) => setHeaderHeight(e.nativeEvent.layout.height)}
      items="center"
      px="$3"
      pb="$2.5"
      pt={topInset + 8}
      bg="$background"
      borderBottomWidth={1}
      borderColor="$borderColor"
    >
      <XStack width={flank} items="center">
        {onBack ? (
          <CircleButton
            icon={ChevronLeft}
            onPress={onBack}
            accessibilityLabel={backAccessibilityLabel}
            testID={`${testID}Back`}
          />
        ) : null}
      </XStack>

      <YStack flex={1} items="center" gap="$0.5">
        <Text variant="large" numberOfLines={1} testID={`${testID}Title`}>
          {title}
        </Text>
        {subtitle ? (
          <Text variant="muted" numberOfLines={1} testID={`${testID}Subtitle`}>
            {subtitle}
          </Text>
        ) : null}
      </YStack>

      <XStack width={flank} items="center" justify="flex-end">
        {controlCount > 0 ? (
          <XStack
            rounded={9999}
            bg="$color3"
            p="$0.5"
            gap="$0.5"
            testID={`${testID}Actions`}
          >
            {actions.map((action) => (
              <XStack
                key={action.testID}
                testID={action.testID}
                role="button"
                aria-label={action.accessibilityLabel}
                width={ACTION_SIZE}
                height={ACTION_SIZE}
                items="center"
                justify="center"
                rounded={9999}
                pressStyle={{ bg: '$color5' }}
                cursor="pointer"
                onPress={action.onPress}
              >
                <Icon as={action.icon} size={19} />
              </XStack>
            ))}
            {menu ? <HeaderMenu {...menu} size={ACTION_SIZE} /> : null}
          </XStack>
        ) : null}
      </XStack>
    </XStack>
  );
}
