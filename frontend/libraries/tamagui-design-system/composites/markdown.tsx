import * as React from 'react';
import MarkdownDisplay, {
  type MarkdownProps as MarkdownPropsBase,
} from 'react-native-markdown-display';

import { useTheme } from '../primitives';
import { CodeBlock } from '../ui/code-block';

export type MarkdownProps = {
  content: string;
  onLinkPress?: MarkdownPropsBase['onLinkPress'];
};

/**
 * Tamagui-styled markdown renderer for assistant messages.
 *
 * Wraps `react-native-markdown-display` (pure-JS, renders RN `Text`/`View`,
 * so it works under react-native-web AND Expo Go). Colours come from
 * `useTheme()` because markdown styles are RN style objects, not classNames.
 *
 * Fenced code blocks are replaced with the DS `CodeBlock` so header rows,
 * language labels and copy buttons are Tamagui primitives.
 */
export function Markdown({
  content,
  onLinkPress,
}: MarkdownProps): React.ReactElement {
  const theme = useTheme();
  const fg = theme.color?.val ?? '#ffffff';
  const muted = theme.color11?.val ?? 'rgba(160,160,160,1)';
  const codeBg = theme.color3?.val ?? '#1c1c1e';
  const border = theme.borderColor?.val ?? 'rgba(128,128,128,0.35)';
  const link = theme.blue10?.val ?? '#4dabf7';

  const styles = React.useMemo(
    () => ({
      body: { color: fg, fontSize: 15, lineHeight: 22 },
      heading1: {
        color: fg,
        fontSize: 20,
        fontWeight: '700' as const,
        marginTop: 8,
        marginBottom: 4,
      },
      heading2: {
        color: fg,
        fontSize: 18,
        fontWeight: '700' as const,
        marginTop: 8,
        marginBottom: 4,
      },
      heading3: {
        color: fg,
        fontSize: 16,
        fontWeight: '600' as const,
        marginTop: 6,
        marginBottom: 2,
      },
      strong: { fontWeight: '700' as const },
      em: { fontStyle: 'italic' as const },
      bullet_list: { marginVertical: 4 },
      ordered_list: { marginVertical: 4 },
      list_item: { marginVertical: 2 },
      code_inline: {
        color: fg,
        backgroundColor: codeBg,
        fontFamily: 'Menlo, Courier, monospace',
        fontSize: 13,
        paddingHorizontal: 4,
        borderRadius: 4,
      },
      fence: {
        color: fg,
        backgroundColor: codeBg,
        borderColor: border,
        borderWidth: 1,
        borderRadius: 8,
        padding: 10,
        fontFamily: 'Menlo, Courier, monospace',
        fontSize: 13,
      },
      code_block: {
        color: fg,
        backgroundColor: codeBg,
        borderColor: border,
        borderWidth: 1,
        borderRadius: 8,
        padding: 10,
        fontFamily: 'Menlo, Courier, monospace',
        fontSize: 13,
      },
      link: { color: link },
      blockquote: {
        backgroundColor: codeBg,
        borderColor: border,
        borderLeftWidth: 3,
        paddingHorizontal: 8,
        marginVertical: 4,
      },
      hr: { backgroundColor: border, height: 1, marginVertical: 8 },
      table: { borderColor: border, borderWidth: 1 },
      th: { padding: 6, borderColor: border, borderWidth: 1 },
      td: { padding: 6, borderColor: border, borderWidth: 1 },
      text: { color: muted },
    }),
    [fg, muted, codeBg, border, link]
  );

  const rules = React.useMemo(
    () => ({
      fence: (
        node: { content: string; attributes?: { lang?: string } },
        _children: React.ReactNode[]
      ) => (
        <CodeBlock
          code={(node.content || '').replace(/\n$/, '')}
          language={node.attributes?.lang}
        />
      ),
      code_block: (node: { content: string }, _children: React.ReactNode[]) => (
        <CodeBlock code={(node.content || '').replace(/\n$/, '')} />
      ),
    }),
    []
  );

  return (
    <MarkdownDisplay style={styles} rules={rules} onLinkPress={onLinkPress}>
      {content}
    </MarkdownDisplay>
  );
}
