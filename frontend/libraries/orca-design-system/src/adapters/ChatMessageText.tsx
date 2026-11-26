import { Platform } from 'react-native';
import { useChatText } from './chatText';
import { useMemo } from 'react';
import Markdown from 'react-native-markdown-display';
import type { ChatMessageTextProps } from '@lib/agent-ui-contracts';
import { useChatPalette } from './chatPalette';

export function ChatMessageText({ text }: ChatMessageTextProps) {
  const colors = useChatPalette();
  const { value } = useChatText();
  const scale = value.size / 14;
  const spacing = { compact: 0.9, default: 1, relaxed: 1.15 }[
    value.lineSpacing
  ];
  const mono = Platform.OS === 'ios' ? 'Menlo' : 'monospace';
  const serif = Platform.select({
    ios: 'ui-serif',
    web: 'ui-serif, serif',
    default: 'serif',
  });
  const fontFamily =
    value.font === 'mono' ? mono : value.font === 'serif' ? serif : undefined;
  const styles = useMemo(
    () => ({
      body: {
        gap: 12,
        color: colors.textPrimary,
        fontSize: 16.25 * scale,
        lineHeight: 26.25 * scale * spacing,
        fontFamily,
      },
      paragraph: { marginTop: 0, marginBottom: 0 },
      link: { color: colors.accentBlue },
      fence: {
        fontFamily: mono,
        fontSize: 14 * scale,
        lineHeight: 20 * scale * spacing,
        color: colors.textPrimary,
        backgroundColor: colors.bgPanel,
        borderColor: colors.borderSubtle,
      },
      code_inline: {
        fontFamily: mono,
        color: colors.textPrimary,
        backgroundColor: colors.bgRaised,
      },
    }),
    [colors, scale, spacing, fontFamily, mono]
  );
  return <Markdown style={styles}>{text.trim()}</Markdown>;
}
