import type { ChatToolActivityProps } from '@lib/agent-ui-contracts';
import { Check, ChevronDown, ChevronRight, Wrench } from 'lucide-react-native';
import { useState } from 'react';
import { Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { useChatPalette } from './chatPalette';

export function ChatToolActivity({
  summary,
  working,
  rows,
  children,
}: ChatToolActivityProps) {
  const colors = useChatPalette();
  const [open, setOpen] = useState(false);
  const [detail, setDetail] = useState<string | null>(null);
  const text = { fontSize: 14, color: colors.textMuted };
  return (
    <View style={styles.run}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={summary}
        accessibilityState={{ expanded: open }}
        onPress={() => setOpen(!open)}
        hitSlop={5}
        style={styles.header}
      >
        <Wrench size={15} color={colors.textMuted} />
        <Text style={text}>{summary}</Text>
        {open ? (
          <ChevronDown size={13} color={colors.textMuted} />
        ) : working ? null : (
          <Check size={13} color={colors.textMuted} />
        )}
      </Pressable>
      {open && (
        <ScrollView
          nestedScrollEnabled
          style={[styles.members, { borderLeftColor: colors.borderSubtle }]}
        >
          {rows.map((row) => (
            <View key={row.id}>
              <Pressable
                onPress={() => setDetail(detail === row.id ? null : row.id)}
                accessibilityRole="button"
                accessibilityState={{ expanded: detail === row.id }}
                style={styles.line}
              >
                <ChevronRight size={14} color={colors.textMuted} />
                <Text style={text}>{row.name}</Text>
                <Text numberOfLines={1} style={[text, { flexShrink: 1 }]}>
                  {row.input}
                </Text>
              </Pressable>
              {detail === row.id && (
                <View style={styles.detail}>
                  <Text selectable style={text}>
                    {row.input}
                  </Text>
                  {row.output ? (
                    <Text selectable style={text}>
                      {row.output}
                    </Text>
                  ) : null}
                  <Text style={text}>{row.status}</Text>
                </View>
              )}
            </View>
          ))}
        </ScrollView>
      )}
      {children}
    </View>
  );
}
const styles = StyleSheet.create({
  run: { paddingHorizontal: 16, marginTop: 4 },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingVertical: 3,
  },
  members: {
    maxHeight: 288,
    marginLeft: 7,
    marginTop: 2,
    paddingLeft: 13,
    borderLeftWidth: StyleSheet.hairlineWidth,
  },
  line: { flexDirection: 'row', alignItems: 'center', gap: 8, minHeight: 26 },
  detail: { paddingVertical: 4, gap: 4 },
});
