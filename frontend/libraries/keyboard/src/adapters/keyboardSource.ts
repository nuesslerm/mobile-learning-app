import { Logger } from '@lib/logging';
import { Keyboard, Platform, type KeyboardEvent } from 'react-native';

import { useKeyboardStore } from '../core/keyboardStore';

/**
 * Native keyboard source: React Native's own Keyboard events into the store.
 *
 * iOS gets `WillShow`/`WillHide`, which carry the animation duration and curve
 * so UI can move WITH the keyboard rather than after it. Android's `Will*`
 * events do not fire at all, so it uses `DidShow`/`DidHide`.
 */
export function startKeyboardSource(): () => void {
  const { setKeyboardShown, setKeyboardHidden } = useKeyboardStore.getState();

  const showEvent = Platform.OS === 'ios' ? 'keyboardWillShow' : 'keyboardDidShow';
  const hideEvent = Platform.OS === 'ios' ? 'keyboardWillHide' : 'keyboardDidHide';

  const onShow = (event: KeyboardEvent) => {
    setKeyboardShown(event.endCoordinates.height, {
      duration: event.duration,
      easing: event.easing ?? null,
    });
  };
  const onHide = (event: KeyboardEvent) => {
    setKeyboardHidden({ duration: event.duration, easing: event.easing ?? null });
  };

  // a source that starts while the keyboard is up (a screen that mounts then) has no event to hear
  if (Keyboard.isVisible()) setKeyboardShown(Keyboard.metrics()?.height ?? 0, { duration: 0 });

  const show = Keyboard.addListener(showEvent, onShow);
  const change = Platform.OS === 'ios' ? Keyboard.addListener('keyboardWillChangeFrame', (event) => {
    if (useKeyboardStore.getState().visible) onShow(event);
  }) : undefined;
  const hide = Keyboard.addListener(hideEvent, onHide);
  Logger.info('keyboard.source.started', { platform: Platform.OS, showEvent, hideEvent });

  return () => {
    show.remove();
    hide.remove();
    change?.remove();
  };
}
