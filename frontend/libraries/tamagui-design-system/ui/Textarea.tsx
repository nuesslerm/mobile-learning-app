import { Platform, type TextInputProps } from 'react-native';

import { useInSheet } from '../lib/inSheet';
import {
  TextArea as TamaguiTextArea,
  type TextAreaProps as TamaguiTextAreaProps,
} from '../primitives';

import { SheetFieldInput } from './sheet-field';

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
 *
 * Inside a `Sheet` it renders the sheet's own input (`SheetFieldInput`), so the sheet lifts above the keyboard.
 */
export function Textarea({
  multiline = true,
  numberOfLines = Platform.select({ web: 2, native: 8 }),
  placeholderClassName: _placeholderClassName,
  editable,
  ...props
}: TextareaProps) {
  const inSheet = useInSheet();
  if (inSheet) {
    return (
      <SheetFieldInput
        look="textarea"
        multiline={multiline}
        numberOfLines={typeof numberOfLines === 'number' ? numberOfLines : undefined}
        editable={editable}
        {...(props as TextInputProps)}
      />
    );
  }
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
