import { Logger } from '@lib/logging';
import type { ReactNode } from 'react';

import { ChevronRight } from '../icons';
import { XStack, YStack } from '../primitives';

import { Icon } from './icon';
import { Text } from './text';

export type ListItemProps = {
  children?: ReactNode;
  /** Trailing chevron, for rows that navigate. */
  showChevron?: boolean;
  onPress?: () => void;
  /** Suppress the automatic press log -- e.g. a log-viewer row, which would recurse. */
  pressLog?: 'off';
  testID: string;
  /** Grouped rows sit flush inside an InsetGroup; standalone rows carry their own surface. */
  variant?: 'standalone' | 'grouped';
};

/**
 * A list row, deliberately much smaller than app #1's 329-line version.
 *
 * Dropped: the depth/underlay wiring (dead there -- DEFAULT_DEPTH is all
 * zero), and `Section`, whose 48 usages are layout slots that Tamagui's
 * XStack already expresses. Kept: the chevron and the press log.
 *
 * Press fills the row rather than moving it, which is how a native list
 * behaves.
 */
export function ListItem({
  children,
  showChevron = false,
  onPress,
  pressLog,
  testID,
  variant = 'standalone',
}: ListItemProps) {
  const handlePress = () => {
    if (!onPress) return;
    if (pressLog !== 'off') Logger.info('ds.listItem.press', { testID });
    onPress();
  };

  return (
    <XStack
      testID={testID}
      items="center"
      gap="$3"
      px="$4"
      py="$3"
      bg={variant === 'grouped' ? 'transparent' : '$color2'}
      rounded={variant === 'grouped' ? 0 : '$4'}
      {...(onPress
        ? {
            onPress: handlePress,
            pressStyle: { bg: '$color4' },
            cursor: 'pointer',
          }
        : {})}
    >
      <YStack flex={1} gap="$1">
        {children}
      </YStack>
      {showChevron ? (
        <Icon as={ChevronRight} size={18} color="$color11" />
      ) : null}
    </XStack>
  );
}

export { Text as ListItemText };
