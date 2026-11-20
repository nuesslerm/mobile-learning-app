import { forwardRef, type ElementRef } from 'react';
import { TextInput, type TextInputProps } from 'react-native';

import { useReportSheetInputFocus } from '../lib/sheetKeyboardLift';

/**
 * The web twin of `sheet-text-input.tsx`. gorhom's own input asks `findNodeHandle` for a scrollable on every focus, and its web shim
 * answers with a console warning (a red browser run: no test may log a warning), so the web input is the plain one that reports its
 * focus: the web twin of the lift (`lib/sheetKeyboardLift.web.ts`) follows gorhom's rule, a sheet rises for its own fields only.
 */
export const SheetTextInput = forwardRef<ElementRef<typeof TextInput>, TextInputProps>(function SheetTextInput({ onFocus, onBlur, ...rest }, ref) {
  const report = useReportSheetInputFocus();
  return (
    <TextInput
      ref={ref}
      {...rest}
      onFocus={(event) => {
        report(true);
        onFocus?.(event);
      }}
      onBlur={(event) => {
        report(false);
        onBlur?.(event);
      }}
    />
  );
});
