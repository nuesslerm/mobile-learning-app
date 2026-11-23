import { YStack } from '@lib/tamagui-design-system/primitives';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { ConsoleActions, type ConsoleActionsProps } from './ConsoleActions';

/** The bar under the editor while the keyboard is down and the sheet is closed, above the bottom safe-area inset. */
export function ConsoleDock(props: Omit<ConsoleActionsProps, 'variant'>) {
  const insets = useSafeAreaInsets();
  return (
    <YStack px="$3" pt="$1.5" bg="$background" borderTopWidth={1} borderColor="$borderColor" style={{ paddingBottom: Math.max(insets.bottom, 8) }} testID="ConsoleDock">
      <ConsoleActions variant="bar" {...props} />
    </YStack>
  );
}
