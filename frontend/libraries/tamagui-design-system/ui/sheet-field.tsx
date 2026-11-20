import { forwardRef, type ElementRef } from 'react';
import type { TextInputProps } from 'react-native';

import { tokens } from '../theme/tokens';
import { useTheme } from '../primitives';

import { SheetTextInput } from './sheet-text-input';

export type SheetFieldInputProps = TextInputProps & {
  /** `input` is `TextField`'s look, `textarea` is `Textarea`'s. */
  look: 'input' | 'textarea';
  invalid?: boolean;
};

/**
 * What `TextField` and `Textarea` render inside a `Sheet`: the sheet's own input, so the sheet lifts above the keyboard, dressed in the
 * same theme tokens as the Tamagui field it replaces. Not `styled(SheetTextInput)`: Tamagui's `styled()` over a React Native host
 * component other than a plain View is the shape that crashed on a device (see `spinner.tsx`), so the look is a plain RN style.
 * Tamagui style props (`bg`, `mt`, ...) have no effect here; pass `style`.
 */
export const SheetFieldInput = forwardRef<ElementRef<typeof SheetTextInput>, SheetFieldInputProps>(function SheetFieldInput(
  { look, invalid, style, placeholderTextColor, ...rest },
  ref
) {
  const theme = useTheme();
  const token = (key: string, fallback: string) => (theme[key] as { val?: string } | undefined)?.val ?? fallback;
  const placeholder =
    typeof placeholderTextColor === 'string' && placeholderTextColor.startsWith('$')
      ? token(placeholderTextColor.slice(1), '#999')
      : (placeholderTextColor ?? token('color11', '#999'));

  return (
    <SheetTextInput
      ref={ref}
      placeholderTextColor={placeholder}
      style={[
        {
          borderWidth: 1,
          borderColor: invalid ? token('red8', '#e5484d') : token('borderColor', '#8884'),
          borderRadius: tokens.radius.md,
          color: token('color', '#000'),
          fontSize: tokens.size.lg,
          paddingHorizontal: tokens.space[3],
          paddingVertical: tokens.space[2],
          ...(look === 'textarea'
            ? { backgroundColor: 'transparent', minHeight: tokens.space[16], textAlignVertical: 'top' as const }
            : { backgroundColor: token('background', 'transparent'), minHeight: tokens.space[11] }),
        },
        style,
      ]}
      {...rest}
    />
  );
});
