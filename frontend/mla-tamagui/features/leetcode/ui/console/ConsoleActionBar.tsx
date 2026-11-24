import { YStack } from '@lib/tamagui-design-system/primitives';

import { ConsoleActions, type ConsoleActionsProps } from './ConsoleActions';

/** The bar above the keyboard's symbol toolbar: its own row, not a rail (a rail is a horizontal scroller). */
export function ConsoleActionBar(props: Omit<ConsoleActionsProps, 'variant'>) {
  return (
    <YStack px="$3" py="$1.5" bg="$color2" borderTopWidth={1} borderColor="$borderColor" testID="ConsoleActionBar">
      <ConsoleActions variant="bar" {...props} />
    </YStack>
  );
}
