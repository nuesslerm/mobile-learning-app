import { Children, type ReactNode } from 'react';

import { Separator, YStack } from '../primitives';

export type InsetGroupProps = {
  children: ReactNode;
  testID: string;
};

/**
 * The grouped inset list -- the iOS signature app #1's design system has no
 * answer for, where every row is its own bordered rounded card.
 *
 * One rounded surface, hairline separators between rows and none after the
 * last, inset past the row's leading padding.
 */
export function InsetGroup({ children, testID }: InsetGroupProps) {
  const rows = Children.toArray(children);
  return (
    <YStack testID={testID} bg="$color2" rounded="$4" overflow="hidden" mx="$4">
      {rows.map((row, i) => (
        <YStack key={i}>
          {row}
          {i < rows.length - 1 ? (
            <Separator ml="$4" borderColor="$borderColor" />
          ) : null}
        </YStack>
      ))}
    </YStack>
  );
}
