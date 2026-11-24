import { YStack } from '@lib/tamagui-design-system/primitives';

import { ConsoleActions, type ConsoleActionsProps } from './ConsoleActions';

export function ConsoleFooter({ canvas, ...props }: Omit<ConsoleActionsProps, 'variant'>) {
  return (
    <YStack pt="$2" pb={canvas ? 11 : "$3"} borderTopWidth={canvas ? 0 : 1} borderColor="$borderColor">
      <ConsoleActions variant="footer" canvas={canvas} {...props} />
    </YStack>
  );
}
