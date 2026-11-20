import { useEffect } from 'react';
import { create } from 'zustand';

import { useKeyboardFrame } from './keyboard/useKeyboardFrame';

/**
 * The web twin of gorhom's keyboard handling, under the same rule: a sheet rises above the keyboard only while a field it knows
 * (`Sheet.TextInput`) holds focus. gorhom reads the native keyboard events, which react-native-web never sends, so without this a
 * plain TextInput in a sheet looks fine in a browser and flow test and is covered by the keyboard on a phone. The height is the
 * frame the keyboard source reports (the harness's drawn keyboard, or the visual viewport).
 */
const useSheetInputFocusStore = create<{ focused: boolean; setFocused: (focused: boolean) => void }>()((set) => ({
  focused: false,
  setFocused: (focused) => set({ focused }),
}));

export function useSheetKeyboardLift(): number {
  const keyboard = useKeyboardFrame();
  const focused = useSheetInputFocusStore((s) => s.focused);
  return keyboard.visible && focused ? keyboard.height : 0;
}

export function useReportSheetInputFocus(): (focused: boolean) => void {
  const setFocused = useSheetInputFocusStore((s) => s.setFocused);
  // a field that unmounts while focused sends no blur
  useEffect(() => () => setFocused(false), [setFocused]);
  return setFocused;
}
