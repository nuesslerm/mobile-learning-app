import { BottomSheetTextInput } from '@gorhom/bottom-sheet';

/**
 * A text field in a sheet must be this one: gorhom only lifts the sheet above the keyboard for the inputs it knows. A plain RN TextInput
 * takes focus, the keyboard rises over the sheet's lower half and covers the control the sheet exists for (a Send button, say).
 * On web this file is replaced by `sheet-text-input.web.tsx`.
 */
export const SheetTextInput = BottomSheetTextInput;
