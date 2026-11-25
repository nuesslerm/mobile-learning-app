import { useKeyboardFrame } from '@lib/tamagui-design-system';
import type { ReactNode } from 'react';
import { KeyboardAvoidingView, Platform } from 'react-native';

export function ChatKeyboardArea({ children }: { children: ReactNode }) {
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
      {children}
    </KeyboardAvoidingView>
  );
}
