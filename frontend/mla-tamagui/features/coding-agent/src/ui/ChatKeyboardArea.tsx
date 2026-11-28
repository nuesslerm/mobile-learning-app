import { useKeyboardFrame } from '@lib/keyboard';
import type { ReactNode } from 'react';
import { KeyboardAvoidingView, Platform, View } from 'react-native';

import { useSafeAreaInsets } from 'react-native-safe-area-context';

export function ChatKeyboardArea({
  children,
  restingBottom,
  alignToScreenBottom = false,
}: {
  children: ReactNode;
  restingBottom?: number;
  alignToScreenBottom?: boolean;
}) {
  const { bottom } = useSafeAreaInsets();
  const frame = useKeyboardFrame();
  if (alignToScreenBottom && Platform.OS === 'ios') {
    return (
      <View style={{ flex: 1, minHeight: 0, paddingBottom: frame.visible ? frame.height : (restingBottom ?? bottom) }}>
        {children}
      </View>
    );
  }
  if (Platform.OS === 'android') {
    // the window does not shrink for the keyboard here (edge to edge), and Android's keyboard height leaves out the navigation bar it also covers
    return (
      <View style={{ flex: 1, minHeight: 0, paddingBottom: frame.visible ? frame.height + bottom : (restingBottom ?? bottom) }}>
        {children}
      </View>
    );
  }
  return (
    <KeyboardAvoidingView
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
      style={{
        flex: 1,
        minHeight: 0,
        paddingBottom: Platform.OS === 'web' ? frame.height : 0,
      }}
    >
      <View
        style={{
          flex: 1,
          minHeight: 0,
          paddingBottom: frame.height > 0 ? 0 : (restingBottom ?? bottom),
        }}
      >
        {children}
      </View>
    </KeyboardAvoidingView>
  );
}
