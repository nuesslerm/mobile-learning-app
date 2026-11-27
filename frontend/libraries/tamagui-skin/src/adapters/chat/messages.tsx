import { useChatText, type ChatMessageMetaProps, type ChatMessageTextProps, type ChatToolActivityProps, type MessageBubbleProps } from '@lib/skin';
import { Icon, Markdown, Text } from '@lib/tamagui-design-system';
import { Check, ChevronDown, ChevronRight, Copy, Wrench } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Platform, ScrollView } from 'react-native';

export function ChatSurface({ children }: { children: ReactNode }) {
  return (
    <YStack flex={1} bg="$background">
      {children}
    </YStack>
  );
}

/** A message: the reader's own in a filled bubble on the right, the agent's as plain text across the width. */
export function MessageBubble({ role, children, footer, onLongPress }: MessageBubbleProps) {
  const user = role === 'user';
  return (
    <YStack px="$4" py="$2" items={user ? 'flex-end' : 'stretch'}>
      <YStack
        gap="$2"
        maxW={user ? '88%' : '100%'}
        {...(user ? { bg: '$color3', rounded: '$4', px: '$3', py: '$2' } : {})}
        {...(onLongPress ? { onLongPress } : {})}
      >
        {children}
      </YStack>
      {footer}
    </YStack>
  );
}

const SPACING = { compact: 0.9, default: 1, relaxed: 1.15 } as const;
const MONO = Platform.OS === 'ios' ? 'Menlo' : 'monospace';
const SERIF = Platform.select({ ios: 'ui-serif', web: 'ui-serif, serif', default: 'serif' });

/** A message's text as the app renders markdown, in the size, spacing and face the reader chose. */
export function ChatMessageText({ text, copyCode }: ChatMessageTextProps) {
  const { value } = useChatText();
  const bodyStyle = useMemo(() => {
    const scale = value.size / 14;
    return {
      fontSize: 15 * scale,
      lineHeight: 22 * scale * SPACING[value.lineSpacing],
      fontFamily: value.font === 'mono' ? MONO : value.font === 'serif' ? SERIF : undefined,
    };
  }, [value.size, value.lineSpacing, value.font]);
  const onCopyCode = useMemo(() => (copyCode ? (code: string) => void copyCode(code) : undefined), [copyCode]);
  return <Markdown content={text} bodyStyle={bodyStyle} onCopyCode={onCopyCode} />;
}

export function ChatMessageMeta({ time, accessibleTime, onCopy }: ChatMessageMetaProps) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  return (
    <XStack items="center" gap="$1.5">
      {time ? (
        <Text variant="muted" aria-label={accessibleTime}>
          {time}
        </Text>
      ) : null}
      {onCopy ? (
        <XStack
          role="button"
          aria-label={copied ? 'Copied' : 'Copy message'}
          width={24}
          height={24}
          items="center"
          justify="center"
          pressStyle={{ opacity: 0.7 }}
          cursor="pointer"
          onPress={async () => {
            if (!(await onCopy())) return;
            setCopied(true);
            clearTimeout(timer.current);
            timer.current = setTimeout(() => setCopied(false), 1500);
          }}
        >
          <Icon as={copied ? Check : Copy} size={14} color="$color10" />
        </XStack>
      ) : null}
    </XStack>
  );
}

/** What the agent did between two messages: one line that opens into its steps, each of which opens into its input and output. */
export function ChatToolActivity({ summary, working, rows, children }: ChatToolActivityProps) {
  const [open, setOpen] = useState(false);
  const [detail, setDetail] = useState<string | null>(null);
  return (
    <YStack gap="$2" mt="$1">
      <XStack
        role="button"
        aria-label={summary}
        aria-expanded={open}
        items="center"
        gap="$2"
        pressStyle={{ opacity: 0.7 }}
        cursor="pointer"
        onPress={() => setOpen(!open)}
      >
        <Icon as={Wrench} size={15} color="$color10" />
        <Text variant="muted">{summary}</Text>
        {open ? <Icon as={ChevronDown} size={13} color="$color10" /> : working ? null : <Icon as={Check} size={13} color="$color10" />}
      </XStack>
      {open ? (
        <ScrollView nestedScrollEnabled style={{ maxHeight: 280 }}>
          <YStack pl="$3" gap="$1.5" borderLeftWidth={1} borderColor="$borderColor">
            {rows.map((row) => (
              <YStack key={row.id} gap="$1">
                <XStack
                  role="button"
                  aria-expanded={detail === row.id}
                  items="center"
                  gap="$1.5"
                  pressStyle={{ opacity: 0.7 }}
                  cursor="pointer"
                  onPress={() => setDetail(detail === row.id ? null : row.id)}
                >
                  <Icon as={ChevronRight} size={14} color="$color10" />
                  <Text variant="muted">{row.name}</Text>
                  <Text variant="muted" numberOfLines={1} shrink={1}>
                    {row.input}
                  </Text>
                </XStack>
                {detail === row.id ? (
                  <YStack pl="$4" gap="$1">
                    <Text variant="code" color="$color11">
                      {row.input}
                    </Text>
                    {row.output ? (
                      <Text variant="code" color="$color11">
                        {row.output}
                      </Text>
                    ) : null}
                  </YStack>
                ) : null}
              </YStack>
            ))}
          </YStack>
        </ScrollView>
      ) : null}
      {children}
    </YStack>
  );
}
