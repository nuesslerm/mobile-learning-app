import {
  containerRelativeFrame,
  frame,
  presentationBackgroundInteraction,
} from '@expo/ui/swift-ui/modifiers';

export const expoSheetWidth = [
  containerRelativeFrame({ axes: 'horizontal', alignment: 'leading' }),
];

export const expoSheetFill = [
  frame({ maxWidth: Infinity, maxHeight: Infinity, alignment: 'topLeading' }),
];

export function expoSheetModifiers(interactiveBackground: boolean) {
  return [
    ...expoSheetFill,
    ...(interactiveBackground
      ? [presentationBackgroundInteraction('enabled')]
      : []),
  ];
}
