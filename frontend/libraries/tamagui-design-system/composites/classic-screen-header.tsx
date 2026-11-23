import type { LucideIcon } from 'lucide-react-native';
import { useContext } from 'react';
import { SafeAreaInsetsContext } from 'react-native-safe-area-context';

import { ChevronLeft } from '../icons';
import { useHeaderHeightStore } from '../lib/headerHeightStore';
import { XStack, YStack } from '../primitives';
import { Icon } from '../ui/icon';
import { Spinner } from '../ui/spinner';
import { Text } from '../ui/text';

import { MenuPanel } from './menu-panel';
import {
  HEADER_BUSY_LABEL,
  headerGroups,
  isMenuSpec,
  type HeaderGroup,
  type ScreenHeaderSpec,
} from './native-header/header-spec';

// Both flanks are the same width so the title is genuinely centred. Letting
// them size to content drifts the title whenever the action count changes --
// which reads as a wobble when navigating between screens. The width grows with
// the control count rather than being a constant, because the old fixed 96 fit
// exactly two controls and silently clipped a third.
const FLANK_MIN_WIDTH = 96;
const ACTION_SIZE = 34;
const PILL_PADDING = 12;
const GROUP_GAP = 8;

function pillWidth(controlCount: number): number {
  return controlCount * ACTION_SIZE + (controlCount - 1) * 4 + PILL_PADDING;
}

function flankWidth(pillWidths: number[]): number {
  if (pillWidths.length === 0) return FLANK_MIN_WIDTH;
  const gaps = (pillWidths.length - 1) * GROUP_GAP;
  return Math.max(
    FLANK_MIN_WIDTH,
    pillWidths.reduce((sum, width) => sum + width, 0) + gaps
  );
}
// Used when no SafeAreaProvider is above this header (see below). Roughly a
// modern phone's status bar, so the header never sits under the clock.
export const DEFAULT_TOP_INSET = 44;

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

function GroupPill({ group, testID }: { group: HeaderGroup; testID: string }) {
  return (
    <XStack rounded={9999} bg="$color3" p="$0.5" gap="$0.5" testID={testID}>
      {group.map((member) =>
        isMenuSpec(member) ? (
          <MenuPanel
            key={member.testID}
            icon={member.icon}
            accessibilityLabel={member.accessibilityLabel}
            items={member.items}
            testID={member.testID}
            size={ACTION_SIZE}
          />
        ) : (
          <XStack
            key={member.testID}
            testID={member.testID}
            role="button"
            aria-label={member.accessibilityLabel}
            width={ACTION_SIZE}
            height={ACTION_SIZE}
            items="center"
            justify="center"
            rounded={9999}
            pressStyle={{ bg: '$color5' }}
            cursor="pointer"
            onPress={member.onPress}
          >
            <Icon as={member.icon} size={19} />
          </XStack>
        )
      )}
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
 *
 * `actions` and `menu` make today's single pill (`<testID>Actions`); `groups`
 * makes one pill each (`<testID>Group<n>`), and `busy` puts a spinner before them.
 */
export function ClassicScreenHeader({
  title,
  subtitle,
  onBack,
  backAccessibilityLabel = 'Go back',
  busy,
  testID,
  ...spec
}: ScreenHeaderSpec) {
  const groups = headerGroups({ title, testID, ...spec });
  const pillIds = groups.map((_, index) =>
    spec.groups === undefined ? `${testID}Actions` : `${testID}Group${index}`
  );
  const widths = groups.map((group) => pillWidth(group.length));
  if (busy) widths.unshift(ACTION_SIZE);
  const flank = flankWidth(widths);

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

      <XStack width={flank} items="center" justify="flex-end" gap={GROUP_GAP}>
        {busy ? (
          <XStack
            width={ACTION_SIZE}
            height={ACTION_SIZE}
            items="center"
            justify="center"
          >
            <Spinner
              testID={`${testID}Busy`}
              accessibilityLabel={HEADER_BUSY_LABEL}
            />
          </XStack>
        ) : null}
        {groups.map((group, index) => (
          <GroupPill
            key={pillIds[index]}
            group={group}
            testID={pillIds[index]}
          />
        ))}
      </XStack>
    </XStack>
  );
}
