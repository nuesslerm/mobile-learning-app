import type { BottomSheetProps } from '@expo/ui';

export const expoSheetFill: BottomSheetProps['modifiers'] = undefined;
export const expoSheetWidth: BottomSheetProps['modifiers'] = undefined;

export function expoSheetModifiers(
  _interactiveBackground: boolean
): BottomSheetProps['modifiers'] {
  return undefined;
}
