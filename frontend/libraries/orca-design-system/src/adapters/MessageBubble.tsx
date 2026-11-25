import type { MessageBubbleProps } from '@lib/agent-ui-contracts';
import { Platform, Pressable, StyleSheet, View } from 'react-native';
import { useChatPalette } from './chatPalette';

export function MessageBubble({ role, children, footer, onLongPress }: MessageBubbleProps) {
  const colors = useChatPalette();
  const user = role === 'user';
  const content = [
    styles.content,
    user && styles.bubble,
    user && { backgroundColor: colors.bgRaised, borderColor: colors.userBorder },
  ];
  return (
    <View style={[styles.row, user && styles.userRow]}>
      {Platform.OS === 'android' && onLongPress ? (
        <Pressable accessible={false} style={content} onLongPress={onLongPress}>
          {children}
        </Pressable>
      ) : (
        <View style={content}>{children}</View>
      )}
      {footer}
    </View>
  );
}

const styles = StyleSheet.create({
  row: { paddingHorizontal: 16, paddingVertical: 8 },
  userRow: { alignItems: 'flex-end' },
  content: { maxWidth: '100%', gap: 8 },
  bubble: {
    maxWidth: '88%', borderWidth: 1, borderRadius: 16,
    paddingHorizontal: 12, paddingVertical: 8,
  },
});
