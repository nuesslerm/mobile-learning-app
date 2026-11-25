import { YStack } from '@lib/tamagui-design-system/primitives';
import { Platform } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { ConsoleActions, type ConsoleActionsProps } from './ConsoleActions';

/** The bar below the editor reserves its space while Console overlays it. */
export function ConsoleDock({
  backgroundColor,
  canvas,
  covered,
  ...props
}: Omit<ConsoleActionsProps, 'variant'> & { backgroundColor?: string; covered: boolean }) {
  const insets = useSafeAreaInsets();
  const ios = Platform.OS === 'ios';
  const bottomSpacing = ios ? (canvas ? 10 : 12) : 0;
  return (
    <YStack
      px="$3"
      pt="$1.5"
      bg="$background"
      borderTopWidth={ios ? 0 : 1}
      borderColor="$borderColor"
      aria-hidden={covered}
      importantForAccessibility={covered ? 'no-hide-descendants' : 'auto'}
      pointerEvents={covered ? 'none' : 'auto'}
      style={{
        backgroundColor,
        paddingBottom: Math.max(insets.bottom, 8) + bottomSpacing,
        opacity: covered ? 0 : 1,
      }}
      testID="ConsoleDock"
    >
      <ConsoleActions variant="bar" canvas={canvas} {...props} />
    </YStack>
  );
}
