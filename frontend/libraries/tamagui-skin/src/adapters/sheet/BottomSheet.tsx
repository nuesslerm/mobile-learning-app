import type { BottomSheetProps } from '@lib/skin';
import { YStack } from '@lib/tamagui-design-system/primitives';

import { ScreenSheet } from './ScreenSheet';

const SHORT = [40];

/** A plain sheet for a caller's own content. Its surface is the app's sheet: the style a caller gives is the other skin's. */
export function BottomSheet({ visible = true, children, onClose, onDismiss, label }: BottomSheetProps) {
  if (!visible) return null;
  const dismiss = (after?: () => void) => {
    onClose();
    onDismiss?.();
    after?.();
  };
  return (
    <ScreenSheet onClose={() => dismiss()} snapPoints={SHORT} handleLabel={label ?? 'Dismiss'} testID="skin-bottom-sheet">
      <YStack px="$4" pb="$4" gap="$2">
        {children(dismiss)}
      </YStack>
    </ScreenSheet>
  );
}
