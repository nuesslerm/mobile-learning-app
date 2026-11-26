import { Platform, StyleSheet } from 'react-native';
import { useChatText } from './chatText';
import { useMemo, useRef } from 'react';
import Markdown, { type ASTNode } from 'react-native-markdown-display';
import { ChatCodeBlock } from './ChatCodeBlock';
import type { ChatMessageTextProps } from '@lib/agent-ui-contracts';
import { useChatPalette } from './chatPalette';

export function ChatMessageText({
  text,
  copyCode,
  allowCodeWrap,
  tone,
}: ChatMessageTextProps) {
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
  const choices = useRef(new Set<string>());
  const rules = useMemo(() => {
    function fence(
      node: ASTNode & { sourceInfo?: string },
      _children: unknown,
      parents: ASTNode[]
    ) {
      const blockId = [
        ...parents.map((parent) => parent.tokenIndex),
        node.tokenIndex,
      ].join('.');
      const code = node.content.endsWith('\n')
        ? node.content.slice(0, -1)
        : node.content;
      return (
        <ChatCodeBlock
          key={blockId}
          blockId={blockId}
          code={code}
          language={node.sourceInfo?.trim().split(/\s+/, 1)[0] ?? ''}
          scale={scale * 1.25}
          wrapChoices={allowCodeWrap ? choices.current : undefined}
          copy={copyCode}
        />
      );
    }
    return { fence, code_block: fence };
  }, [allowCodeWrap, copyCode, scale]);
  const styles = useMemo(
    () => ({
      body: {
        gap: 12,
        color: tone === 'user' ? colors.chatForegroundStrong : colors.textPrimary,
        fontSize: 16.25 * scale,
        lineHeight: 26.25 * scale * spacing,
        fontFamily,
      },
      paragraph: { marginTop: 0, marginBottom: 0 },
      link: { color: colors.accentBlue },
      code_inline: {
        fontSize: 15 * scale,
        padding: 0,
        paddingHorizontal: 4,
        borderWidth: StyleSheet.hairlineWidth,
        borderRadius: 4,
        borderColor: colors.borderSubtle,
        fontFamily: mono,
        color: colors.textPrimary,
        backgroundColor: colors.bgRaised,
      },
    }),
    [colors, scale, spacing, fontFamily, mono, tone]
  );
  return (
    <Markdown rules={rules} style={styles}>
      {text}
    </Markdown>
  );
}
