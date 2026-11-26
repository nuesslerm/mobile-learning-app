import { useEffect, useRef, useState } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import Copy from 'lucide-react-native/dist/esm/icons/copy.mjs';
import Check from 'lucide-react-native/dist/esm/icons/check.mjs';
import type { ChatMessageMetaProps } from '@lib/agent-ui-contracts';
import { useChatPalette } from './chatPalette';

export function ChatMessageMeta({ time, accessibleTime, onCopy }: ChatMessageMetaProps) {
  const colors = useChatPalette();
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  return <View style={styles.meta}>
    {time && <Text accessibilityLabel={accessibleTime} style={[styles.time, {color: colors.textMuted}]}>{time}</Text>}
    {onCopy && <Pressable accessibilityRole="button" accessibilityLabel={copied ? 'Copied' : 'Copy message'}
      hitSlop={6} style={styles.copy} onPress={async () => {
        if (!await onCopy()) return;
        setCopied(true);
        clearTimeout(timer.current);
        timer.current = setTimeout(() => setCopied(false), 1500);
      }}>
      {copied ? <Check size={14} color={colors.textMuted}/> : <Copy size={14} color={colors.textMuted}/>}
    </Pressable>}
  </View>;
}
const styles = StyleSheet.create({
  meta: {flexDirection: 'row', alignItems: 'center', gap: 6},
  time: {fontSize: 12, lineHeight: 18, fontVariant: ['tabular-nums']},
  copy: {width: 24, height: 24, marginHorizontal: -5, alignItems:'center', justifyContent:'center'},
});
