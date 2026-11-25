import { useKeyboardFrame } from '@lib/tamagui-design-system';
import type { ReactNode } from 'react';
import { KeyboardAvoidingView, Platform, View } from 'react-native';

import { useSafeAreaInsets } from 'react-native-safe-area-context';

export function ChatKeyboardArea({ children }: { children: ReactNode }) {
  const { bottom } = useSafeAreaInsets();
  const frame = useKeyboardFrame();
  return (
    <KeyboardAvoidingView
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
      style={{
        flex: 1,
        minHeight: 0,
        paddingBottom: Platform.OS === 'web' ? frame.height : 0,
      }}
    >
      <View style={{ flex: 1, minHeight: 0, paddingBottom: frame.height > 0 ? 0 : bottom }}>{children}</View>
    </KeyboardAvoidingView>
  );
}
