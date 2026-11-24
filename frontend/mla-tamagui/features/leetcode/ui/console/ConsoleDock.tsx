import { YStack } from '@lib/tamagui-design-system/primitives';
import { Platform } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { ConsoleActions, type ConsoleActionsProps } from './ConsoleActions';

/** The bar under the editor while the keyboard is down and the sheet is closed, above the bottom safe-area inset. */
export function ConsoleDock({
  backgroundColor,
  ...props
}: Omit<ConsoleActionsProps, 'variant'> & { backgroundColor?: string }) {
  const insets = useSafeAreaInsets();
  const ios = Platform.OS === 'ios';
  return (
    <YStack px="$3" pt="$1.5" bg="$background" borderTopWidth={ios ? 0 : 1} borderColor="$borderColor" style={{ backgroundColor, paddingBottom: Math.max(insets.bottom, 8) + (ios ? 12 : 0) }} testID="ConsoleDock">
      <ConsoleActions variant="bar" {...props} />
    </YStack>
  );
}
