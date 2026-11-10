import { Logger } from '@lib/logging';
import { create } from 'zustand';
type KeyboardTransition = {
  duration?: number;
  easing?: string | null;
};
type KeyboardState = {
  visible: boolean;
  height: number;
  duration: number;
  easing: string | null;
  setKeyboardShown: (height: number, transition?: KeyboardTransition) => void;
  setKeyboardHidden: (transition?: KeyboardTransition) => void;
};
export const useKeyboardStore = create<KeyboardState>()(set => ({
  visible: false,
  height: 0,
  duration: 250,
  easing: null,
  setKeyboardShown: (height, transition) => {
    Logger.info('keyboard.show', {
      height,
      duration: transition?.duration ?? 250,
      easing: transition?.easing ?? null
    });
    set({
      visible: true,
      height,
      duration: transition?.duration ?? 250,
      easing: transition?.easing ?? null
    });
  },
  setKeyboardHidden: transition => {
    Logger.info('keyboard.hide', {
      duration: transition?.duration ?? 250,
      easing: transition?.easing ?? null
    });
    set({
      visible: false,
      height: 0,
      duration: transition?.duration ?? 250,
      easing: transition?.easing ?? null
    });
  }
}));