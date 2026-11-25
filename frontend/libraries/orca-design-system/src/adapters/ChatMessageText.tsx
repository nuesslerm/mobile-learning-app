import { useMemo } from 'react';
import Markdown from 'react-native-markdown-display';
import type { ChatMessageTextProps } from '@lib/agent-ui-contracts';
import { useChatPalette } from './chatPalette';

export function ChatMessageText({ text }: ChatMessageTextProps) {
  const colors = useChatPalette();
  const styles = useMemo(() => ({
    body: { gap: 12, color: colors.textPrimary, fontSize: 16.25, lineHeight: 26.25 },
    paragraph: { marginTop: 0, marginBottom: 0 },
    link: { color: colors.accentBlue },
    fence: { color: colors.textPrimary, backgroundColor: colors.bgPanel, borderColor: colors.borderSubtle },
    code_inline: { color: colors.textPrimary, backgroundColor: colors.bgRaised },
  }), [colors]);
  return <Markdown style={styles}>{text.trim()}</Markdown>;
}
