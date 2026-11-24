import { Children, type ReactNode } from 'react';

import { Separator, YStack } from '../primitives';

export type InsetGroupProps = {
  children: ReactNode;
  testID: string;
  surface?: 'solid' | 'transparent';
  inset?: boolean;
};

/**
 * The grouped inset list -- the iOS signature app #1's design system has no
 * answer for, where every row is its own bordered rounded card.
 *
 * One rounded surface, hairline separators between rows and none after the
 * last, inset past the row's leading padding.
 */
export function InsetGroup({
  children,
  testID,
  surface = 'solid',
  inset = true,
}: InsetGroupProps) {
  const rows = Children.toArray(children);
  const transparent = surface === 'transparent';
  return (
    <YStack
      testID={testID}
      bg={transparent ? 'transparent' : '$color2'}
      rounded={transparent ? 0 : '$4'}
      overflow="hidden"
      mx={inset ? '$4' : 0}
    >
      {rows.map((row, i) => (
        <YStack key={i}>
          {row}
          {i < rows.length - 1 ? (
            <Separator
              ml="$4"
              borderColor={
                transparent ? 'rgba(127, 127, 127, 0.22)' : '$borderColor'
              }
            />
          ) : null}
        </YStack>
      ))}
    </YStack>
  );
}
