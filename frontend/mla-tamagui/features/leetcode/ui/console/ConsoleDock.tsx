import { YStack } from '@lib/tamagui-design-system/primitives';
import { Platform } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { ConsoleActions, type ConsoleActionsProps } from './ConsoleActions';

/** The bar under the editor while the keyboard is down and the sheet is closed, above the bottom safe-area inset. */
export function ConsoleDock({
  backgroundColor,
  canvas,
  ...props
}: Omit<ConsoleActionsProps, 'variant'> & { backgroundColor?: string }) {
  const insets = useSafeAreaInsets();
  const ios = Platform.OS === 'ios';
  const bottomSpacing = ios ? (canvas ? 10 : 12) : 0;
  return (
    <YStack px="$3" pt="$1.5" bg="$background" borderTopWidth={ios ? 0 : 1} borderColor="$borderColor" style={{ backgroundColor, paddingBottom: Math.max(insets.bottom, 8) + bottomSpacing }} testID="ConsoleDock">
      <ConsoleActions variant="bar" canvas={canvas} {...props} />
    </YStack>
  );
}
