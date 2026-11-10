import { Platform } from 'react-native';

import {
  TextArea as TamaguiTextArea,
  type TextAreaProps as TamaguiTextAreaProps,
} from '../primitives';

export type TextareaProps = TamaguiTextAreaProps & {
  /** Web-only class hook from app #1's surface; accepted and ignored. */
  placeholderClassName?: string;
  /** RN TextInput parity; mapped to Tamagui's `disabled` plus dimmed opacity. */
  editable?: boolean;
};

/**
 * Multiline input on Tamagui's TextArea, with the prop surface (testID, placeholder, value, onChangeText,
 * numberOfLines, editable) so a ported screen swaps the import path and
 * nothing else.
 *
 * App #1's field look -- transparent background, hairline input border,
 * muted placeholder, 0.5 opacity when not editable -- expressed as theme
 * tokens only. numberOfLines keeps app #1's platform split: initial height
 * on web, maximum height on native.
 */
export function Textarea({
  multiline = true,
  numberOfLines = Platform.select({ web: 2, native: 8 }),
  placeholderClassName: _placeholderClassName,
  editable,
  ...props
}: TextareaProps) {
  return (
    <TamaguiTextArea
      multiline={multiline}
      numberOfLines={numberOfLines}
      textAlignVertical="top"
      bg="transparent"
      borderWidth={1}
      borderColor="$borderColor"
      rounded="$md"
      minH="$16"
      px="$3"
      py="$2"
      color="$color"
      placeholderTextColor="$color11"
      disabled={editable === false}
      opacity={editable === false ? 0.5 : undefined}
      {...props}
    />
  );
}
