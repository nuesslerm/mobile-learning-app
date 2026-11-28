// Adapted from Orca mobile at 2c0cbbc (MIT, Lovecast Inc.).
import type { ChatMessageTextProps } from '@lib/skin';
import { Check, Copy, WrapText } from 'lucide-react-native';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { highlightCode } from './codeSyntax';
import { useChatPalette } from './chatPalette';

const mono = Platform.select({
  ios: 'Menlo',
  web: 'ui-monospace, "SF Mono", SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace',
  default: 'monospace',
});

function CopyCode({
  code,
  copy,
}: {
  code: string;
  copy: NonNullable<ChatMessageTextProps['copyCode']>;
}) {
  const colors = useChatPalette();
  const [status, setStatus] = useState<'copied' | 'failed'>();
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const latest = useRef(0);
  useEffect(
    () => () => {
      latest.current++;
      clearTimeout(timer.current);
    },
    []
  );
  async function press() {
    const attempt = ++latest.current;
    clearTimeout(timer.current);
    const ok = await copy(code).catch(() => false);
    if (attempt !== latest.current) return;
    setStatus(ok ? 'copied' : 'failed');
    timer.current = setTimeout(() => setStatus(undefined), 1500);
  }
  const Icon = status === 'copied' ? Check : Copy;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={status === 'failed' ? "Couldn't copy" : 'Copy code'}
      accessibilityValue={status === 'copied' ? { text: 'Copied' } : undefined}
      hitSlop={10}
      onPress={press}
      style={styles.control}
    >
      <Icon
        size={14}
        color={
          status === 'failed'
            ? colors.statusRed
            : status === 'copied'
              ? colors.statusGreen
              : colors.textMuted
        }
      />
    </Pressable>
  );
}

export function ChatCodeBlock({
  code,
  language,
  scale,
  copy,
  wrapChoices,
  blockId,
}: {
  code: string;
  language: string;
  scale: number;
  copy?: ChatMessageTextProps['copyCode'];
  wrapChoices?: Set<string>;
  blockId: string;
}) {
  const colors = useChatPalette();
  const [wrapped, setWrapped] = useState(() => !!wrapChoices?.has(blockId));
  const segments = useMemo(
    () =>
      language
        ? highlightCode(code, language.toLowerCase()).segments
        : undefined,
    [code, language]
  );
  const lineNumbers = useMemo(
    () =>
      Array.from({ length: code.split('\n').length }, (_, i) =>
        String(i + 1)
      ).join('\n'),
    [code]
  );
  const metrics = {
    fontFamily: mono,
    fontSize: 12 * scale,
    lineHeight: 17 * scale,
  };
  const lines = (
    <Text
      selectable
      style={[
        metrics,
        { color: colors.textPrimary, paddingRight: 12 },
        wrapped && { flex: 1, paddingLeft: 12 },
      ]}
    >
      {segments
        ? segments.map(({ text, kind }, index) => (
            <Text
              key={index}
              style={{
                color:
                  kind === 'plain' ? colors.textPrimary : colors.syntax[kind],
              }}
            >
              {text}
            </Text>
          ))
        : code}
    </Text>
  );
  function toggleWrap() {
    if (wrapped) wrapChoices?.delete(blockId);
    else wrapChoices?.add(blockId);
    setWrapped(!wrapped);
  }
  return (
    <View
      style={[
        styles.block,
        { backgroundColor: colors.bgRaised, borderColor: colors.borderSubtle },
      ]}
    >
      <View style={[styles.header, { borderColor: colors.borderSubtle }]}>
        <Text
          numberOfLines={1}
          style={[styles.lang, { color: colors.textSecondary }]}
        >
          {language || 'Code'}
        </Text>
        <View style={styles.actions}>
          {!!code && wrapChoices && (
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={wrapped ? 'Disable line wrap' : 'Wrap lines'}
              accessibilityState={{ selected: wrapped }}
              hitSlop={10}
              onPress={toggleWrap}
              style={[
                styles.control,
                wrapped && { backgroundColor: colors.translucentFill },
              ]}
            >
              <WrapText
                size={14}
                color={wrapped ? colors.textPrimary : colors.textMuted}
              />
            </Pressable>
          )}
          {!!code && copy && <CopyCode key={code} code={code} copy={copy} />}
        </View>
      </View>
      <View style={styles.body}>
        {!!code && !wrapped && (
          <Text
            selectable={false}
            aria-hidden
            style={[metrics, styles.gutter, { color: colors.textMuted }]}
          >
            {lineNumbers}
          </Text>
        )}
        {wrapped ? (
          lines
        ) : (
          <ScrollView horizontal showsHorizontalScrollIndicator={false}>
            {lines}
          </ScrollView>
        )}
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  block: {
    borderWidth: 1,
    borderRadius: 16,
    borderCurve: 'continuous',
    overflow: 'hidden',
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 8,
    minHeight: 32,
    paddingLeft: 12,
    paddingRight: 4,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  lang: { flexShrink: 1, fontSize: 12, textTransform: 'uppercase' },
  actions: { flexDirection: 'row', alignItems: 'center', gap: 4 },
  control: {
    width: 24,
    height: 24,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 12,
  },
  body: { flexDirection: 'row', paddingVertical: 8 },
  gutter: { paddingHorizontal: 8, textAlign: 'right' },
});
