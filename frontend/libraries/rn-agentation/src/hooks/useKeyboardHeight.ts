// useKeyboardHeight.ts
import * as React from 'react';
import { Keyboard, Platform } from 'react-native';

export interface KeyboardState {
  height: number;
  visible: boolean;
}

/**
 * Self-contained keyboard tracker for the agentation overlay. The package can't
 * import the app's keyboard store, so it listens to RN's `Keyboard` directly.
 * iOS uses the `Will` events (smoother); Android only fires `Did`.
 */
export function useKeyboardHeight(): KeyboardState {
  const [state, setState] = React.useState<KeyboardState>({ height: 0, visible: false });

  React.useEffect(() => {
    const showEvent = Platform.OS === 'ios' ? 'keyboardWillShow' : 'keyboardDidShow';
    const hideEvent = Platform.OS === 'ios' ? 'keyboardWillHide' : 'keyboardDidHide';

    const showSub = Keyboard.addListener(showEvent, (e) => {
      setState({ height: e.endCoordinates?.height ?? 0, visible: true });
    });
    const hideSub = Keyboard.addListener(hideEvent, () => {
      setState({ height: 0, visible: false });
    });

    return () => {
      showSub.remove();
      hideSub.remove();
    };
  }, []);

  return state;
}
